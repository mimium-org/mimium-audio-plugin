use crate::mimium;
use crate::params::param_index_from_id;
use crate::{MimiumPluginMainThread, MimiumPluginShared};
use clack_extensions::params::PluginAudioProcessorParams;
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use mimium_lang::runtime::wasm::engine::WasmDspRuntime;
use mimium_lang::runtime::{DspRuntime, Time};

pub struct MimiumPluginAudioProcessor<'a> {
    shared: &'a MimiumPluginShared,
    host: HostAudioProcessorHandle<'a>,
    runtime: Option<WasmDspRuntime>,
    sample_time: u64,
    sample_rate: f64,
}

impl<'a> PluginAudioProcessor<'a, MimiumPluginShared, MimiumPluginMainThread<'a>>
    for MimiumPluginAudioProcessor<'a>
{
    fn activate(
        host: HostAudioProcessorHandle<'a>,
        _main_thread: &mut MimiumPluginMainThread<'a>,
        shared: &'a MimiumPluginShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let mut processor = Self {
            shared,
            host,
            runtime: None,
            sample_time: 0,
            sample_rate: audio_config.sample_rate,
        };
        processor.try_swap_runtime();
        Ok(processor)
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let mut had_param_updates = false;
        for event_batch in events.input.batch() {
            for event in event_batch.events() {
                if let Some(CoreEventSpace::ParamValue(value)) = event.as_core_event() {
                    if let Some(param_id) = value.param_id() {
                        if let Some(index) = param_index_from_id(param_id) {
                            self.shared.knobs.set_value(index, value.value());
                            had_param_updates = true;
                        }
                    }
                }
            }
        }

        if had_param_updates {
            self.host.request_callback();
        }

        self.try_swap_runtime();

        let mut output_port = audio
            .output_port(0)
            .ok_or(PluginError::Message("No output port found"))?;
        let mut output_channels = output_port
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("Expected f32 output"))?;

        let (mut left_channels, mut right_channels) = output_channels.split_at_mut(1);
        let left = left_channels
            .channel_mut(0)
            .ok_or(PluginError::Message("Expected left channel"))?;
        let right = right_channels
            .channel_mut(0)
            .ok_or(PluginError::Message("Expected right channel"))?;

        for frame in 0..left.len().min(right.len()) {
            let (out_left, out_right) = if let Some(runtime) = self.runtime.as_mut() {
                runtime.run_dsp(Time(self.sample_time));
                self.sample_time = self.sample_time.saturating_add(1);

                let output = runtime.get_output(2);
                (
                    output.get(0).copied().unwrap_or_default() as f32,
                    output.get(1).copied().unwrap_or_default() as f32,
                )
            } else {
                (0.0, 0.0)
            };

            left[frame] = out_left;
            right[frame] = out_right;
        }

        Ok(ProcessStatus::ContinueIfNotQuiet)
    }
}

impl PluginAudioProcessorParams for MimiumPluginAudioProcessor<'_> {
    fn flush(
        &mut self,
        input_parameter_changes: &InputEvents,
        _output_parameter_changes: &mut OutputEvents,
    ) {
        let mut had_param_updates = false;

        for event in input_parameter_changes {
            if let Some(CoreEventSpace::ParamValue(value)) = event.as_core_event() {
                if let Some(param_id) = value.param_id() {
                    if let Some(index) = param_index_from_id(param_id) {
                        self.shared.knobs.set_value(index, value.value());
                        had_param_updates = true;
                    }
                }
            }
        }

        if had_param_updates {
            self.host.request_callback();
        }
    }
}

impl MimiumPluginAudioProcessor<'_> {
    fn try_swap_runtime(&mut self) {
        let Some(program) = self.take_pending_program() else {
            return;
        };

        match mimium::build_runtime(program, self.sample_rate) {
            Ok(runtime) => {
                self.runtime = Some(runtime);
                self.sample_time = 0;
            }
            Err(error) => {
                eprintln!("failed to build mimium runtime: {error}");
            }
        }
    }

    fn take_pending_program(&self) -> Option<mimium::PreparedProgram> {
        self.shared
            .pending_program
            .try_lock()
            .ok()
            .and_then(|mut pending| pending.take())
    }
}
