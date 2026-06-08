use crate::MimiumPluginMainThread;
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginMainThreadParams,
};
use clack_plugin::events::event_types::ParamValueEvent;
use clack_plugin::events::io::OutputEvents;
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::ClapId;
use std::array;
use std::ffi::CStr;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const KNOB_COUNT: usize = 8;
pub const KNOB_MIN_VALUE: f64 = 0.0;
pub const KNOB_MAX_VALUE: f64 = 1.0;

pub struct KnobBank {
    values: [AtomicU64; KNOB_COUNT],
    names: Mutex<[String; KNOB_COUNT]>,
}

impl KnobBank {
    pub fn new() -> Self {
        Self {
            values: array::from_fn(|_| AtomicU64::new((0.5f64).to_bits())),
            names: Mutex::new(array::from_fn(|index| format!("Knob {}", index + 1))),
        }
    }

    pub fn snapshot_values(&self) -> [f64; KNOB_COUNT] {
        array::from_fn(|index| self.value(index))
    }

    pub fn snapshot_names(&self) -> [String; KNOB_COUNT] {
        self.names
            .lock()
            .map(|names| names.clone())
            .unwrap_or_else(|_| array::from_fn(|index| format!("Knob {}", index + 1)))
    }

    pub fn set_names(&self, names: [String; KNOB_COUNT]) {
        if let Ok(mut current) = self.names.lock() {
            *current = names;
        }
    }

    pub fn set_name(&self, index: usize, name: String) {
        if index >= KNOB_COUNT {
            return;
        }

        if let Ok(mut names) = self.names.lock() {
            names[index] = normalize_name(index, &name);
        }
    }

    pub fn name(&self, index: usize) -> String {
        if index >= KNOB_COUNT {
            return String::new();
        }

        self.names
            .lock()
            .map(|names| names[index].clone())
            .unwrap_or_else(|_| format!("Knob {}", index + 1))
    }

    pub fn set_value(&self, index: usize, value: f64) {
        if index >= KNOB_COUNT {
            return;
        }

        let clamped = value.clamp(KNOB_MIN_VALUE, KNOB_MAX_VALUE);
        self.values[index].store(clamped.to_bits(), Ordering::Relaxed);
    }

    pub fn value(&self, index: usize) -> f64 {
        if index >= KNOB_COUNT {
            return 0.0;
        }

        f64::from_bits(self.values[index].load(Ordering::Relaxed))
    }

    pub fn set_from_param_event(&self, event: &ParamValueEvent) -> bool {
        let Some(param_id) = event.param_id() else {
            return false;
        };

        let Some(index) = param_index_from_id(param_id) else {
            return false;
        };

        self.set_value(index, event.value());
        true
    }

}

pub fn normalize_name(index: usize, candidate: &str) -> String {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        format!("Knob {}", index + 1)
    } else {
        trimmed.to_string()
    }
}

pub fn param_id_for_index(index: usize) -> ClapId {
    ClapId::new((index + 1) as u32)
}

pub fn param_index_from_id(id: ClapId) -> Option<usize> {
    let raw = id.get();
    if raw == 0 || raw > KNOB_COUNT as u32 {
        None
    } else {
        Some((raw - 1) as usize)
    }
}

impl PluginMainThreadParams for MimiumPluginMainThread<'_> {
    fn count(&mut self) -> u32 {
        KNOB_COUNT as u32
    }

    fn get_info(&mut self, param_index: u32, info: &mut ParamInfoWriter) {
        let index = param_index as usize;
        if index >= KNOB_COUNT {
            return;
        }

        let name = self.shared.knobs.name(index);
        info.set(&ParamInfo {
            id: param_id_for_index(index),
            flags: ParamInfoFlags::IS_AUTOMATABLE,
            cookie: Default::default(),
            name: name.as_bytes(),
            module: b"Controls",
            min_value: KNOB_MIN_VALUE,
            max_value: KNOB_MAX_VALUE,
            default_value: 0.5,
        });
    }

    fn get_value(&mut self, param_id: ClapId) -> Option<f64> {
        let index = param_index_from_id(param_id)?;
        Some(self.shared.knobs.value(index))
    }

    fn value_to_text(
        &mut self,
        param_id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        let index = param_index_from_id(param_id).ok_or(std::fmt::Error)?;
        write!(writer, "{}: {:.3}", self.shared.knobs.name(index), value)
    }

    fn text_to_value(&mut self, param_id: ClapId, text: &CStr) -> Option<f64> {
        param_index_from_id(param_id)?;
        text.to_str()
            .ok()
            .and_then(|raw| raw.trim().parse::<f64>().ok())
            .map(|value| value.clamp(KNOB_MIN_VALUE, KNOB_MAX_VALUE))
    }

    fn flush(
        &mut self,
        input_parameter_changes: &clack_plugin::events::io::InputEvents,
        _output_parameter_changes: &mut OutputEvents,
    ) {
        for event in input_parameter_changes {
            if let Some(CoreEventSpace::ParamValue(value)) = event.as_core_event() {
                let _ = self.shared.knobs.set_from_param_event(value);
            }
        }
    }
}
