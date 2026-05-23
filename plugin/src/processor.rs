use crate::mimium;
use crate::{MimiumPluginMainThread, MimiumPluginShared};
use clack_plugin::prelude::*;
use mimium_lang::runtime::wasm::engine::WasmDspRuntime;
use mimium_lang::runtime::{DspRuntime, Time};

pub struct MimiumPluginAudioProcessor<'a> {
    shared: &'a MimiumPluginShared,
    runtime: Option<WasmDspRuntime>,
    sample_time: u64,
    sample_rate: f64,
}

impl<'a> PluginAudioProcessor<'a, MimiumPluginShared, MimiumPluginMainThread<'a>>
    for MimiumPluginAudioProcessor<'a>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &mut MimiumPluginMainThread<'a>,
        shared: &'a MimiumPluginShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let mut processor = Self {
            shared,
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
        _events: Events,
    ) -> Result<ProcessStatus, PluginError> {
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
