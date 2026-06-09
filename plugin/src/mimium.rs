use crate::params::{KnobBank, KNOB_COUNT};
use base64::Engine;
use mimium_lang::compiler::{EvalStage, IoChannelInfo};
use mimium_lang::interner::{Symbol, TypeNodeId};
use mimium_lang::plugin::ExtFunTypeInfo;
use mimium_lang::runtime::wasm::WasmPluginFnMap;
use mimium_lang::runtime::wasm::WasmPluginFn;
use mimium_lang::runtime::wasm::engine::{WasmDspRuntime, WasmEngine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

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

pub(crate) fn compile_program(
    source: &str,
    knobs: Arc<KnobBank>,
    library_path: Option<&str>,
) -> Result<PreparedProgram, String> {
    compile_program_subprocess(source, knobs, library_path)
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

    let mut child = Command::new(worker_command())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn compiler worker: {error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let payload = serde_json::to_vec(&request)
            .map_err(|error| format!("failed to serialize worker request: {error}"))?;
        stdin
            .write_all(&payload)
            .map_err(|error| format!("failed to write worker request: {error}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to wait for compiler worker: {error}"))?;

    let response: WorkerCompileResponse = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("failed to decode worker response: {error}"))?;

    if !output.status.success() || !response.ok {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let worker_error = if stderr.trim().is_empty() {
            response.message
        } else {
            format!("{}\n{}", response.message, stderr.trim())
        };
        return Err(worker_error);
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
        ext_fns: response
            .ext_fns
            .into_iter()
            .map(|info| ExtFunTypeInfo::new(info.name, info.ty, info.stage))
            .collect(),
        wasm_plugin_fns: Some(make_wasm_plugin_fns(knobs)),
        resolved_knob_names,
    })
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

#[derive(Clone, Copy, Deserialize)]
struct SerializableExtFunTypeInfo {
    name: Symbol,
    ty: TypeNodeId,
    stage: EvalStage,
}

