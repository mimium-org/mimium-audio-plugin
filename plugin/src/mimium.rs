use crate::params::{KnobBank, KNOB_COUNT};
use base64::Engine;
use mimium_lang::compiler::{EvalStage, IoChannelInfo};
use mimium_lang::interner::ToSymbol;
use mimium_lang::plugin::ExtFunTypeInfo;
use mimium_lang::runtime::wasm::WasmPluginFnMap;
use mimium_lang::runtime::wasm::WasmPluginFn;
use mimium_lang::runtime::wasm::engine::{WasmDspRuntime, WasmEngine};
use mimium_lang::types::{PType, Type};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;

pub(crate) const DEFAULT_SOURCE: &str = r#"let twopi = 6.283185307179586

fn phasor(freq: float) {
  (self + freq / samplerate) % 1.0
}

fn dsp() {
    let left_freq = Control!("Left Freq", 220.0)
    let right_freq = Control!("Right Freq", 330.0)
    let gain = Control!("Gain", 0.18)
    let left = sin(phasor(left_freq) * twopi) * gain
    let right = sin(phasor(right_freq) * twopi) * gain
  (left, right)
}"#;

#[derive(Clone)]
pub(crate) struct CompileFeedback {
    pub(crate) ok: bool,
    pub(crate) message: String,
}

impl CompileFeedback {
    pub(crate) fn success(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }

    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

pub(crate) struct PreparedProgram {
    wasm_bytes: Vec<u8>,
    io_channels: Option<IoChannelInfo>,
    ext_fns: Vec<ExtFunTypeInfo>,
    wasm_plugin_fns: Option<WasmPluginFnMap>,
    pub(crate) resolved_knob_names: [String; KNOB_COUNT],
}

pub(crate) struct CompileTaskResult {
    pub(crate) source: String,
    pub(crate) feedback: CompileFeedback,
    pub(crate) program: Option<PreparedProgram>,
}

struct CompileTask {
    source: String,
    knobs: Arc<KnobBank>,
    library_path: Option<String>,
    result_sink: Arc<Mutex<Vec<CompileTaskResult>>>,
    pending_host_callback: Arc<AtomicBool>,
}

pub(crate) fn compile_program(
    source: &str,
    knobs: Arc<KnobBank>,
    library_path: Option<&str>,
) -> Result<PreparedProgram, String> {
    compile_program_subprocess(source, knobs, library_path)
}

pub(crate) fn enqueue_compile_task(
    source: String,
    knobs: Arc<KnobBank>,
    library_path: Option<String>,
    result_sink: Arc<Mutex<Vec<CompileTaskResult>>>,
    pending_host_callback: Arc<AtomicBool>,
) -> Result<(), String> {
    static DISPATCHER: OnceLock<mpsc::Sender<CompileTask>> = OnceLock::new();
    let tx = DISPATCHER.get_or_init(spawn_compile_dispatcher).clone();
    tx.send(CompileTask {
        source,
        knobs,
        library_path,
        result_sink,
        pending_host_callback,
    })
    .map_err(|error| format!("failed to queue compile task: {error}"))
}

fn spawn_compile_dispatcher() -> mpsc::Sender<CompileTask> {
    let (tx, rx) = mpsc::channel::<CompileTask>();
    let _ = thread::Builder::new()
        .name("mimium-compile-dispatcher".to_string())
        .spawn(move || run_compile_dispatcher(rx));
    tx
}

fn run_compile_dispatcher(rx: mpsc::Receiver<CompileTask>) {
    for task in rx {
        let result = compile_program_subprocess(
            &task.source,
            Arc::clone(&task.knobs),
            task.library_path.as_deref(),
        );

        let outcome = match result {
            Ok(program) => CompileTaskResult {
                source: task.source,
                feedback: CompileFeedback::success(
                    "Compiled via worker process. The audio thread will swap the program on the next block.",
                ),
                program: Some(program),
            },
            Err(error) => CompileTaskResult {
                source: task.source,
                feedback: CompileFeedback::error(error),
                program: None,
            },
        };

        if let Ok(mut sink) = task.result_sink.lock() {
            sink.push(outcome);
        }
        task.pending_host_callback.store(true, Ordering::SeqCst);
    }
}

fn compile_program_subprocess(
    source: &str,
    knobs: Arc<KnobBank>,
    library_path: Option<&str>,
) -> Result<PreparedProgram, String> {
    let request = WorkerCompileRequest {
        source: source.to_string(),
        library_path: library_path.map(|path| path.to_string()),
        initial_knob_names: knobs.snapshot_names().to_vec(),
    };

    let response = worker_pool_request(&request)?;

    if !response.ok {
        return Err(response.message);
    }

    let wasm_base64 = response
        .wasm_base64
        .ok_or_else(|| "compiler worker did not return wasm output".to_string())?;
    let io_channels = response.io_channels.map(|io| IoChannelInfo {
        input: io.input,
        output: io.output,
    });
    validate_channels(io_channels)?;

    let mut resolved = response.resolved_knob_names;
    if resolved.len() < KNOB_COUNT {
        resolved.resize_with(KNOB_COUNT, || "".to_string());
    }
    let resolved_knob_names: [String; KNOB_COUNT] = resolved
        .into_iter()
        .take(KNOB_COUNT)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "invalid worker knob name count".to_string())?;

    Ok(PreparedProgram {
        wasm_bytes: base64::engine::general_purpose::STANDARD
            .decode(wasm_base64)
            .map_err(|error| format!("failed to decode worker wasm: {error}"))?,
        io_channels,
        ext_fns: decode_ext_fns(&response.ext_fns),
        wasm_plugin_fns: Some(make_wasm_plugin_fns(knobs)),
        resolved_knob_names,
    })
}

fn decode_ext_fns(worker_ext_fns: &[SerializableExtFunTypeInfo]) -> Vec<ExtFunTypeInfo> {
    worker_ext_fns
        .iter()
        .map(|info| {
            let arg_ty = arg_type_from_signature(&info.params);
            let ret_ty = return_type_from_signature(&info.returns);
            let fn_ty = Type::Function {
                arg: arg_ty.into_id(),
                ret: ret_ty.into_id(),
            }
            .into_id();

            ExtFunTypeInfo::new(info.name.as_str().to_symbol(), fn_ty, info.stage)
        })
        .collect()
}

fn arg_type_from_signature(parts: &[SerializableWasmValType]) -> Type {
    match parts {
        [] => Type::Tuple(Vec::new()),
        [single] => wasm_valtype_to_mimium_type(*single),
        _ => Type::Tuple(
            parts
                .iter()
                .map(|part| wasm_valtype_to_mimium_type(*part).into_id())
                .collect(),
        ),
    }
}

fn return_type_from_signature(parts: &[SerializableWasmValType]) -> Type {
    match parts {
        [] => Type::Primitive(PType::Unit),
        [single] => wasm_valtype_to_mimium_type(*single),
        _ => Type::Tuple(
            parts
                .iter()
                .map(|part| wasm_valtype_to_mimium_type(*part).into_id())
                .collect(),
        ),
    }
}

fn wasm_valtype_to_mimium_type(valtype: SerializableWasmValType) -> Type {
    match valtype {
        SerializableWasmValType::I32 => Type::Primitive(PType::Int),
        SerializableWasmValType::I64 => Type::Primitive(PType::Int),
        SerializableWasmValType::F32 => Type::Primitive(PType::Numeric),
        SerializableWasmValType::F64 => Type::Primitive(PType::Numeric),
    }
}

fn worker_pool_request(request: &WorkerCompileRequest) -> Result<WorkerCompileResponse, String> {
    static WORKER_POOL: OnceLock<Mutex<Option<PersistentWorkerClient>>> = OnceLock::new();
    let pool = WORKER_POOL.get_or_init(|| Mutex::new(None));

    let mut guard = pool
        .lock()
        .map_err(|_| "compiler worker mutex was poisoned".to_string())?;

    if guard.is_none() {
        *guard = Some(PersistentWorkerClient::spawn()?);
    }

    let first_try = guard
        .as_mut()
        .ok_or_else(|| "compiler worker client is not available".to_string())?
        .request(request);

    match first_try {
        Ok(response) => Ok(response),
        Err(error) => {
            eprintln!("[mimium] compiler worker request failed once: {error}; restarting worker");
            *guard = Some(PersistentWorkerClient::spawn()?);
            guard
                .as_mut()
                .ok_or_else(|| "compiler worker client is not available after restart".to_string())?
                .request(request)
                .map_err(|retry_error| {
                    format!(
                        "compiler worker request failed after restart: {retry_error}"
                    )
                })
        }
    }
}

struct PersistentWorkerClient {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl PersistentWorkerClient {
    fn spawn() -> Result<Self, String> {
        let mut child = Command::new(worker_command())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("failed to spawn compiler worker: {error}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "compiler worker stdin is unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "compiler worker stdout is unavailable".to_string())?;

        Ok(Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn request(&mut self, request: &WorkerCompileRequest) -> Result<WorkerCompileResponse, String> {
        let payload = serde_json::to_string(request)
            .map_err(|error| format!("failed to serialize worker request: {error}"))?;
        self.stdin
            .write_all(payload.as_bytes())
            .map_err(|error| format!("failed to write worker request: {error}"))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|error| format!("failed to terminate worker request: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("failed to flush worker request: {error}"))?;

        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .map_err(|error| format!("failed to read worker response: {error}"))?;
        if read == 0 {
            return Err("compiler worker closed stdout".to_string());
        }

        serde_json::from_str::<WorkerCompileResponse>(line.trim_end())
            .map_err(|error| format!("failed to decode worker response: {error}"))
    }
}

pub(crate) fn build_runtime(
    mut program: PreparedProgram,
    sample_rate: f64,
) -> Result<WasmDspRuntime, String> {
    let mut engine = WasmEngine::new(&program.ext_fns, program.wasm_plugin_fns.take())
        .map_err(|error| format!("failed to create mimium Wasm engine: {error}"))?;
    engine
        .load_module(&program.wasm_bytes)
        .map_err(|error| format!("failed to load mimium Wasm module: {error}"))?;

    let mut runtime = WasmDspRuntime::new(
        engine,
        program.io_channels,
        None,
    );
    runtime.set_sample_rate(sample_rate);
    runtime
        .run_main()
        .map_err(|error| format!("mimium main initialization failed: {error}"))?;

    Ok(runtime)
}

fn validate_channels(io: Option<IoChannelInfo>) -> Result<(), String> {
    let Some(io) = io else {
        return Err("mimium source must define dsp() with stereo output".to_string());
    };

    if (io.input != 0 && io.input != 2) || io.output != 2 {
        return Err(format!(
            "dsp() must expose 0 or 2 input / 2 output channels, but got {} input / {} output",
            io.input, io.output
        ));
    }

    Ok(())
}

fn make_wasm_plugin_fns(knobs: Arc<KnobBank>) -> WasmPluginFnMap {
    let mut map = HashMap::new();
    map.insert(
        "__get_slider".to_string(),
        Arc::new(move |args: &[f64]| {
            let index = args.first().copied().unwrap_or_default() as usize;
            Some(knobs.value(index))
        }) as WasmPluginFn,
    );
    map
}

fn worker_command() -> String {
    if let Some(path) = std::env::var("MIMIUM_COMPILER_WORKER_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return path;
    }

    if let Some(path) = discover_worker_near_plugin() {
        return path;
    }

    if let Ok(cwd) = std::env::current_dir() {
        let debug_path = cwd.join("target/debug/mimium-compiler-worker");
        if debug_path.exists() {
            return debug_path.to_string_lossy().to_string();
        }

        let release_path = cwd.join("target/release/mimium-compiler-worker");
        if release_path.exists() {
            return release_path.to_string_lossy().to_string();
        }
    }

    "mimium-compiler-worker".to_string()
}

#[cfg(target_os = "macos")]
fn discover_worker_near_plugin() -> Option<String> {
    use std::ffi::CStr;
    use std::path::PathBuf;

    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::zeroed();
    let marker = discover_worker_near_plugin as *const () as *const libc::c_void;
    let ok = unsafe { libc::dladdr(marker, info.as_mut_ptr()) };
    if ok == 0 {
        return None;
    }

    let info = unsafe { info.assume_init() };
    if info.dli_fname.is_null() {
        return None;
    }

    let dylib_path = PathBuf::from(unsafe { CStr::from_ptr(info.dli_fname) }.to_string_lossy().to_string());
    let dylib_dir = dylib_path.parent()?.to_path_buf();
    let candidates = [
        dylib_dir.join("mimium-compiler-worker"),
        dylib_dir.join("../Resources/mimium-compiler-worker"),
    ];

    candidates
        .into_iter()
        .find(|path| path.exists())
        .map(|path| path.to_string_lossy().to_string())
}

#[cfg(not(target_os = "macos"))]
fn discover_worker_near_plugin() -> Option<String> {
    None
}

#[derive(Serialize)]
struct WorkerCompileRequest {
    source: String,
    library_path: Option<String>,
    initial_knob_names: Vec<String>,
}

#[derive(Deserialize)]
struct WorkerCompileResponse {
    ok: bool,
    message: String,
    wasm_base64: Option<String>,
    io_channels: Option<SerializableIoChannelInfo>,
    ext_fns: Vec<SerializableExtFunTypeInfo>,
    resolved_knob_names: Vec<String>,
}

#[derive(Clone, Copy, Deserialize)]
struct SerializableIoChannelInfo {
    input: u32,
    output: u32,
}

#[derive(Clone, Deserialize)]
struct SerializableExtFunTypeInfo {
    name: String,
    stage: EvalStage,
    params: Vec<SerializableWasmValType>,
    returns: Vec<SerializableWasmValType>,
}

#[derive(Clone, Copy, Deserialize)]
enum SerializableWasmValType {
    I32,
    I64,
    F32,
    F64,
}

