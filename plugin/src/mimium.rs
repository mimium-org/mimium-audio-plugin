use mimium_lang::compiler::WasmOutput;
use mimium_lang::runtime::wasm::engine::{WasmDspRuntime, WasmEngine};
use mimium_lang::utils::error::ReportableError;
use mimium_lang::{Config, ExecContext};

pub(crate) const DEFAULT_SOURCE: &str = r#"let twopi = 6.283185307179586

fn phasor(freq: float) {
  (self + freq / samplerate) % 1.0
}

fn dsp() {
  let left = sin(phasor(220.0) * twopi) * 0.18
  let right = sin(phasor(330.0) * twopi) * 0.18
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
    wasm_output: WasmOutput,
}

pub(crate) fn compile_program(source: &str) -> Result<PreparedProgram, String> {
    let mut ctx = ExecContext::new([].into_iter(), None, Config::default());
    ctx.prepare_compiler();

    let compiler = ctx
        .get_compiler()
        .ok_or_else(|| "mimium compiler initialization failed".to_string())?;
    let wasm_output = compiler.emit_wasm(source).map_err(format_compile_errors)?;
    validate_channels(&wasm_output)?;

    Ok(PreparedProgram { wasm_output })
}

pub(crate) fn build_runtime(
    mut program: PreparedProgram,
    sample_rate: f64,
) -> Result<WasmDspRuntime, String> {
    let mut engine = WasmEngine::new(&program.wasm_output.ext_fns, None)
        .map_err(|error| format!("failed to create mimium Wasm engine: {error}"))?;
    engine
        .load_module(&program.wasm_output.bytes)
        .map_err(|error| format!("failed to load mimium Wasm module: {error}"))?;

    let mut runtime = WasmDspRuntime::new(
        engine,
        program.wasm_output.io_channels,
        program.wasm_output.dsp_state_skeleton.take(),
    );
    runtime.set_sample_rate(sample_rate);
    runtime
        .run_main()
        .map_err(|error| format!("mimium main initialization failed: {error}"))?;

    Ok(runtime)
}

fn validate_channels(output: &WasmOutput) -> Result<(), String> {
    let Some(io) = output.io_channels else {
        return Err("mimium source must define dsp() with stereo output".to_string());
    };

    if io.input != 0 || io.output != 2 {
        return Err(format!(
            "dsp() must expose 0 input / 2 output channels, but got {} input / {} output",
            io.input, io.output
        ));
    }

    Ok(())
}

fn format_compile_errors(errors: Vec<Box<dyn ReportableError>>) -> String {
    errors
        .into_iter()
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
