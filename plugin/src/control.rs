use crate::params::{normalize_name, KnobBank, KNOB_COUNT};
use mimium_lang::ast::{Expr, Literal};
use mimium_lang::code;
use mimium_lang::function;
use mimium_lang::interner::{ExprNodeId, ToSymbol, TypeNodeId};
use mimium_lang::interpreter::Value;
use mimium_lang::numeric;
use mimium_lang::plugin::{SysPluginSignature, SystemPlugin, SystemPluginFnType, SystemPluginMacroType};
use mimium_lang::string_t;
use mimium_lang::types::{Type, TypeSchemeId};
use mimium_plugin_macros::mimium_plugin_fn;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const GET_SLIDER_FN: &str = "__get_slider";

#[derive(Clone)]
pub(crate) struct ControlCompileResult {
    pub(crate) slot_names: [String; KNOB_COUNT],
    pub(crate) overflow_labels: Vec<String>,
}

pub(crate) struct ControlSystemPlugin {
    knobs: Arc<KnobBank>,
    slot_names: [String; KNOB_COUNT],
    claimed_slots: HashSet<usize>,
    overflow_labels: HashSet<String>,
}

impl ControlSystemPlugin {
    pub(crate) fn new(knobs: Arc<KnobBank>, slot_names: [String; KNOB_COUNT]) -> Self {
        Self {
            knobs,
            slot_names,
            claimed_slots: HashSet::new(),
            overflow_labels: HashSet::new(),
        }
    }

    pub(crate) fn compile_result(&self) -> ControlCompileResult {
        let mut overflow_labels = self.overflow_labels.iter().cloned().collect::<Vec<_>>();
        overflow_labels.sort();

        ControlCompileResult {
            slot_names: self.slot_names.clone(),
            overflow_labels,
        }
    }

    fn claim_or_assign_slot(&mut self, label: &str, init: f64) -> Option<usize> {
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
            self.knobs.set_value(index, init);
            self.claimed_slots.insert(index);
            return Some(index);
        }

        if let Some(index) = (0..KNOB_COUNT).find(|index| !self.claimed_slots.contains(index)) {
            self.slot_names[index] = normalize_name(index, label);
            self.knobs.set_value(index, init);
            self.claimed_slots.insert(index);
            return Some(index);
        }

        self.overflow_labels.insert(label.to_string());
        None
    }

    fn slider_expr(index: usize) -> ExprNodeId {
        Expr::Apply(
            Expr::Var(GET_SLIDER_FN.to_symbol()).into_id_without_span(),
            vec![Expr::Literal(Literal::Float(index.to_string().to_symbol())).into_id_without_span()],
        )
        .into_id_without_span()
    }

    fn extract_numeric_default(value: &Value) -> Option<f64> {
        match value {
            Value::Number(number) => Some(*number),
            Value::Code(expr) => match expr.to_expr() {
                Expr::Literal(Literal::Float(raw)) => raw.as_str().parse::<f64>().ok(),
                Expr::Literal(Literal::Int(raw)) => Some(raw as f64),
                _ => None,
            },
            _ => None,
        }
    }

    fn make_control(&mut self, values: &[(Value, TypeNodeId)]) -> Value {
        let Some((Value::String(label), _)) = values.first() else {
            return Value::Code(Self::slider_expr(0));
        };

        let init = values
            .get(1)
            .and_then(|(value, _)| Self::extract_numeric_default(value))
            .unwrap_or(0.5);

        let index = self
            .claim_or_assign_slot(label.as_str(), init)
            .unwrap_or(KNOB_COUNT - 1);
        Value::Code(Self::slider_expr(index))
    }

    #[mimium_plugin_fn]
    fn get_slider(&mut self, slider_idx: f64) -> f64 {
        self.knobs.value(slider_idx as usize)
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
            function!(vec![string_t!(), generic_type], code!(generic_type)),
        );

        let get_sliderf: SystemPluginFnType<Self> = Self::get_slider;
        let get_slider = SysPluginSignature::new(
            GET_SLIDER_FN,
            get_sliderf,
            function!(vec![numeric!()], numeric!()),
        );

        vec![make_control, get_slider]
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn freeze_for_wasm(&mut self) -> Option<mimium_lang::runtime::wasm::WasmPluginFnMap> {
        let knobs = Arc::clone(&self.knobs);
        let mut map = HashMap::new();
        map.insert(
            GET_SLIDER_FN.to_string(),
            Arc::new(move |args: &[f64]| {
                let index = args.first().copied().unwrap_or_default() as usize;
                Some(knobs.value(index))
            }) as mimium_lang::runtime::wasm::WasmPluginFn,
        );

        Some(map)
    }
}
