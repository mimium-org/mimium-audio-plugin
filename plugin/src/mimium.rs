use crate::control::{ControlCompileResult, ControlSystemPlugin};
use crate::params::{KnobBank, KNOB_COUNT};
use mimium_lang::compiler::WasmOutput;
use mimium_lang::runtime::wasm::WasmPluginFnMap;
use mimium_lang::runtime::wasm::engine::{WasmDspRuntime, WasmEngine};
use mimium_lang::utils::error::ReportableError;
use mimium_lang::{Config, ExecContext};
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
    wasm_output: WasmOutput,
    wasm_plugin_fns: Option<WasmPluginFnMap>,
    pub(crate) resolved_knob_names: [String; KNOB_COUNT],
}

pub(crate) fn compile_program(source: &str, knobs: Arc<KnobBank>) -> Result<PreparedProgram, String> {
    let initial_names = knobs.snapshot_names();
    let mut ctx = ExecContext::new([].into_iter(), None, Config::default());
    ctx.add_system_plugin(ControlSystemPlugin::new(Arc::clone(&knobs), initial_names));
    ctx.prepare_compiler();

    let compiler = ctx
        .get_compiler()
        .ok_or_else(|| "mimium compiler initialization failed".to_string())?;
    let wasm_output = compiler.emit_wasm(source).map_err(format_compile_errors)?;
    let compile_result = read_control_compile_result(&mut ctx).unwrap_or_else(|| ControlCompileResult {
        slot_names: knobs.snapshot_names(),
        overflow_labels: Vec::new(),
    });

    if !compile_result.overflow_labels.is_empty() {
        return Err(format!(
            "Control! labels exceeded {} knobs: {}",
            KNOB_COUNT,
            compile_result.overflow_labels.join(", ")
        ));
    }

    let wasm_plugin_fns = ctx.freeze_wasm_plugin_fns();
    validate_channels(&wasm_output)?;

    Ok(PreparedProgram {
        wasm_output,
        wasm_plugin_fns,
        resolved_knob_names: compile_result.slot_names,
    })
}

pub(crate) fn build_runtime(
    mut program: PreparedProgram,
    sample_rate: f64,
) -> Result<WasmDspRuntime, String> {
    let mut engine = WasmEngine::new(&program.wasm_output.ext_fns, program.wasm_plugin_fns.take())
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

fn read_control_compile_result(ctx: &mut ExecContext) -> Option<ControlCompileResult> {
    ctx.get_system_plugins_mut().find_map(|plugin| {
        let mut plugin_ref = plugin.borrow_inner_mut();
        plugin_ref
            .as_any_mut()
            .downcast_mut::<ControlSystemPlugin>()
            .map(|control| control.compile_result())
    })
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
