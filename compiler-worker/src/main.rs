use base64::Engine;
use mimium_lang::ast::{Expr, Literal};
use mimium_lang::compiler::{EvalStage, IoChannelInfo};
use mimium_lang::function;
use mimium_lang::interner::{Symbol, ToSymbol, TypeNodeId};
use mimium_lang::interpreter::Value;
use mimium_lang::numeric;
use mimium_lang::plugin::{SysPluginSignature, SystemPlugin, SystemPluginFnType, SystemPluginMacroType};
use mimium_lang::string_t;
use mimium_lang::types::{Type, TypeSchemeId};
use mimium_lang::utils::error::ReportableError;
use mimium_lang::{Config, ExecContext};
use mimium_plugin_macros::mimium_plugin_fn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Write};

const GET_SLIDER_FN: &str = "__get_slider";
const KNOB_COUNT: usize = 8;

#[derive(Deserialize)]
struct CompileRequest {
    source: String,
    library_path: Option<String>,
    initial_knob_names: Vec<String>,
}

#[derive(Serialize)]
struct CompileResponse {
    ok: bool,
    message: String,
    wasm_base64: Option<String>,
    io_channels: Option<SerializableIoChannelInfo>,
    ext_fns: Vec<SerializableExtFunTypeInfo>,
    resolved_knob_names: Vec<String>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct SerializableIoChannelInfo {
    input: u32,
    output: u32,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct SerializableExtFunTypeInfo {
    name: Symbol,
    ty: TypeNodeId,
    stage: EvalStage,
}

#[derive(Clone)]
struct ControlCompileResult {
    slot_names: [String; KNOB_COUNT],
    overflow_labels: Vec<String>,
}

struct ControlSystemPlugin {
    slot_names: [String; KNOB_COUNT],
    claimed_slots: HashSet<usize>,
    overflow_labels: HashSet<String>,
}

impl ControlSystemPlugin {
    fn new(slot_names: [String; KNOB_COUNT]) -> Self {
        Self {
            slot_names,
            claimed_slots: HashSet::new(),
            overflow_labels: HashSet::new(),
        }
    }

    fn compile_result(&self) -> ControlCompileResult {
        let mut overflow_labels = self.overflow_labels.iter().cloned().collect::<Vec<_>>();
        overflow_labels.sort();

        ControlCompileResult {
            slot_names: self.slot_names.clone(),
            overflow_labels,
        }
    }

    fn claim_or_assign_slot(&mut self, label: &str) -> Option<usize> {
        if let Some(index) = self
            .slot_names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(label))
        {
            self.claimed_slots.insert(index);
            return Some(index);
        }

        if let Some(index) = (0..KNOB_COUNT).find(|index| {
            !self.claimed_slots.contains(index)
                && self.slot_names[*index].starts_with("Knob ")
                && self.slot_names[*index]
                    .strip_prefix("Knob ")
                    .and_then(|raw| raw.parse::<usize>().ok())
                    == Some(*index + 1)
        }) {
            self.slot_names[index] = normalize_name(index, label);
            self.claimed_slots.insert(index);
            return Some(index);
        }

        if let Some(index) = (0..KNOB_COUNT).find(|index| !self.claimed_slots.contains(index)) {
            self.slot_names[index] = normalize_name(index, label);
            self.claimed_slots.insert(index);
            return Some(index);
        }

        self.overflow_labels.insert(label.to_string());
        None
    }

    fn slider_expr(index: usize) -> mimium_lang::interner::ExprNodeId {
        Expr::Apply(
            Expr::Var(GET_SLIDER_FN.to_symbol()).into_id_without_span(),
            vec![Expr::Literal(Literal::Float(index.to_string().to_symbol())).into_id_without_span()],
        )
        .into_id_without_span()
    }

    fn make_control(&mut self, values: &[(Value, TypeNodeId)]) -> Value {
        let Some((Value::String(label), _)) = values.first() else {
            return Value::Code(Self::slider_expr(0));
        };

        let index = self.claim_or_assign_slot(label.as_str()).unwrap_or(KNOB_COUNT - 1);
        Value::Code(Self::slider_expr(index))
    }

    #[mimium_plugin_fn]
    fn get_slider(&mut self, _slider_idx: f64) -> f64 {
        0.0
    }
}

impl SystemPlugin for ControlSystemPlugin {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn gen_interfaces(&self) -> Vec<SysPluginSignature> {
        let controlf: SystemPluginMacroType<Self> = Self::make_control;
        let generic_type = Type::TypeScheme(TypeSchemeId(10)).into_id();
        let make_control = SysPluginSignature::new_macro(
            "Control",
            controlf,
            function!(vec![string_t!(), generic_type], mimium_lang::code!(generic_type)),
        );

        let get_sliderf: SystemPluginFnType<Self> = Self::get_slider;
        let get_slider = SysPluginSignature::new(
            GET_SLIDER_FN,
            get_sliderf,
            function!(vec![numeric!()], numeric!()),
        );

        vec![make_control, get_slider]
    }
}

fn normalize_name(index: usize, name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return format!("Knob {}", index + 1);
    }

    let mut normalized = String::new();
    let mut prev_space = false;

    for ch in trimmed.chars() {
        if ch.is_ascii_control() {
            continue;
        }

        let next = if ch.is_whitespace() { ' ' } else { ch };
        if next == ' ' {
            if prev_space {
                continue;
            }
            prev_space = true;
        } else {
            prev_space = false;
        }
        normalized.push(next);
    }

    let final_name = normalized.trim();
    if final_name.is_empty() {
        format!("Knob {}", index + 1)
    } else {
        final_name.to_string()
    }
}

fn format_compile_errors(errors: Vec<Box<dyn ReportableError>>) -> String {
    errors
        .into_iter()
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate_channels(io: Option<IoChannelInfo>) -> Result<(), String> {
    let Some(info) = io else {
        return Err("mimium source must define dsp() with stereo output".to_string());
    };

    if (info.input != 0 && info.input != 2) || info.output != 2 {
        return Err(format!(
            "dsp() must expose 0 or 2 input / 2 output channels, but got {} input / {} output",
            info.input, info.output
        ));
    }

    Ok(())
}

fn compile(req: CompileRequest) -> Result<CompileResponse, String> {
    let _lib_path_guard = EnvVarGuard::set("MIMIUM_LIB_PATH", req.library_path.as_deref());

    let mut initial_names = req.initial_knob_names;
    initial_names.resize_with(KNOB_COUNT, || "".to_string());
    let initial_names: [String; KNOB_COUNT] = initial_names
        .into_iter()
        .take(KNOB_COUNT)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "invalid initial knob name count".to_string())?;

    let mut ctx = ExecContext::new([].into_iter(), None, Config::default());
    ctx.add_system_plugin(ControlSystemPlugin::new(initial_names));
    ctx.prepare_compiler();

    let compiler = ctx
        .get_compiler()
        .ok_or_else(|| "mimium compiler initialization failed".to_string())?;
    let wasm_output = compiler
        .emit_wasm(&req.source)
        .map_err(format_compile_errors)?;

    validate_channels(wasm_output.io_channels)?;

    let compile_result = ctx
        .get_system_plugins_mut()
        .find_map(|plugin| {
            let mut plugin_ref = plugin.borrow_inner_mut();
            plugin_ref
                .as_any_mut()
                .downcast_mut::<ControlSystemPlugin>()
                .map(|control| control.compile_result())
        })
        .ok_or_else(|| "failed to read Control! compile result".to_string())?;

    if !compile_result.overflow_labels.is_empty() {
        return Err(format!(
            "Control! labels exceeded {} knobs: {}",
            KNOB_COUNT,
            compile_result.overflow_labels.join(", ")
        ));
    }

    let ext_fns = wasm_output
        .ext_fns
        .into_iter()
        .map(|info| SerializableExtFunTypeInfo {
            name: info.name,
            ty: info.ty,
            stage: info.stage,
        })
        .collect::<Vec<_>>();

    Ok(CompileResponse {
        ok: true,
        message: "Compiled in isolated process.".to_string(),
        wasm_base64: Some(base64::engine::general_purpose::STANDARD.encode(wasm_output.bytes)),
        io_channels: wasm_output.io_channels.map(|io| SerializableIoChannelInfo {
            input: io.input,
            output: io.output,
        }),
        ext_fns,
        resolved_knob_names: compile_result.slot_names.to_vec(),
    })
}

fn main() {
    let mut input = String::new();
    let result = std::io::stdin().read_to_string(&mut input);
    if result.is_err() {
        let _ = write_response(CompileResponse {
            ok: false,
            message: "failed to read compile request".to_string(),
            wasm_base64: None,
            io_channels: None,
            ext_fns: Vec::new(),
            resolved_knob_names: Vec::new(),
        });
        std::process::exit(1);
    }

    let request = serde_json::from_str::<CompileRequest>(&input);
    let response = match request {
        Ok(req) => match compile(req) {
            Ok(ok) => ok,
            Err(error) => CompileResponse {
                ok: false,
                message: error,
                wasm_base64: None,
                io_channels: None,
                ext_fns: Vec::new(),
                resolved_knob_names: Vec::new(),
            },
        },
        Err(error) => CompileResponse {
            ok: false,
            message: format!("invalid compile request: {error}"),
            wasm_base64: None,
            io_channels: None,
            ext_fns: Vec::new(),
            resolved_knob_names: Vec::new(),
        },
    };

    let ok = response.ok;
    let _ = write_response(response);
    if !ok {
        std::process::exit(1);
    }
}

fn write_response(response: CompileResponse) -> Result<(), String> {
    let serialized = serde_json::to_string(&response)
        .map_err(|error| format!("failed to serialize response: {error}"))?;
    std::io::stdout()
        .write_all(serialized.as_bytes())
        .map_err(|error| format!("failed to write response: {error}"))
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let previous = std::env::var_os(key);
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some(next) => std::env::set_var(key, next),
            None => std::env::remove_var(key),
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.clone() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}
