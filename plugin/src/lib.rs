use crate::gui::MimiumPluginGui;
use crate::mimium::{compile_program, CompileFeedback};
use crate::processor::MimiumPluginAudioProcessor;
use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::gui::PluginGui;
use clack_extensions::state::{HostState, PluginState, PluginStateImpl};
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

mod gui;
mod mimium;
mod processor;
mod webview_gui;

pub struct MimiumPlugin;

impl Plugin for MimiumPlugin {
    type AudioProcessor<'a> = MimiumPluginAudioProcessor<'a>;
    type Shared<'a> = MimiumPluginShared;
    type MainThread<'a> = MimiumPluginMainThread<'a>;

    fn declare_extensions(
        builder: &mut PluginExtensions<Self>,
        _shared: Option<&Self::Shared<'_>>,
    ) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginGui>()
            .register::<PluginState>();
    }
}

impl DefaultPluginFactory for MimiumPlugin {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;

        PluginDescriptor::new("org.mimium.mimium-clap-plugin", "Mimium CLAP Plugin")
            .with_features([SYNTHESIZER, STEREO, INSTRUMENT])
    }

    fn new_shared(host: HostSharedHandle<'_>) -> Result<Self::Shared<'_>, PluginError> {
        let source = mimium::DEFAULT_SOURCE.to_string();
        let (pending_program, compile_feedback) = match compile_program(&source) {
            Ok(program) => (
                Some(program),
                CompileFeedback::success("Compiled initial mimium program."),
            ),
            Err(error) => {
                eprintln!("failed to compile default mimium source: {error}");
                (
                    None,
                    CompileFeedback::error(format!(
                        "Initial program failed to compile: {error}"
                    )),
                )
            }
        };

        Ok(MimiumPluginShared {
            current_source: Arc::new(Mutex::new(source)),
            compile_feedback: Arc::new(Mutex::new(compile_feedback)),
            pending_program: Arc::new(Mutex::new(pending_program)),
            pending_ui_messages: Arc::new(Mutex::new(Vec::new())),
            state_dirty: Arc::new(AtomicBool::new(false)),
            host: unsafe { host.with_arbitrary_lifetime() },
        })
    }

    fn new_main_thread<'a>(
        host: HostMainThreadHandle<'a>,
        shared: &'a Self::Shared<'a>,
    ) -> Result<Self::MainThread<'a>, PluginError> {
        let host_state = host.get_extension::<HostState>();
        Ok(MimiumPluginMainThread {
            shared,
            gui: None,
            host,
            host_state,
        })
    }
}

pub struct MimiumPluginShared {
    pub(crate) current_source: Arc<Mutex<String>>,
    pub(crate) compile_feedback: Arc<Mutex<CompileFeedback>>,
    pub(crate) pending_program: Arc<Mutex<Option<mimium::PreparedProgram>>>,
    pub(crate) pending_ui_messages: Arc<Mutex<Vec<String>>>,
    pub(crate) state_dirty: Arc<AtomicBool>,
    pub(crate) host: HostSharedHandle<'static>,
}

impl PluginShared<'_> for MimiumPluginShared {}

pub struct MimiumPluginMainThread<'a> {
    pub(crate) shared: &'a MimiumPluginShared,
    pub(crate) gui: Option<MimiumPluginGui>,
    pub(crate) host: HostMainThreadHandle<'a>,
    pub(crate) host_state: Option<HostState>,
}

impl<'a> PluginMainThread<'a, MimiumPluginShared> for MimiumPluginMainThread<'a> {
    fn on_main_thread(&mut self) {
        if self.shared.state_dirty.swap(false, Ordering::SeqCst) {
            if let Some(mut state) = self.host_state {
                state.mark_dirty(&self.host);
            }
        }

        if let Some(gui) = &self.gui {
            if let Ok(mut queue) = self.shared.pending_ui_messages.lock() {
                for message in queue.drain(..) {
                    gui.send_raw_message(&message);
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedPluginState {
    version: u32,
    source: String,
}

impl PluginStateImpl for MimiumPluginMainThread<'_> {
    fn save(&mut self, output: &mut OutputStream) -> Result<(), PluginError> {
        let source = self
            .shared
            .current_source
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default();

        let data = serde_json::to_vec(&PersistedPluginState { version: 1, source })
            .map_err(|_| PluginError::Message("Failed to serialize plugin state."))?;

        output.write_all(&data)?;
        Ok(())
    }

    fn load(&mut self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut data = Vec::new();
        input.read_to_end(&mut data)?;

        if data.is_empty() {
            return Ok(());
        }

        let source = serde_json::from_slice::<PersistedPluginState>(&data)
            .map(|state| state.source)
            .or_else(|_| {
                String::from_utf8(data)
                    .map_err(|_| PluginError::Message("Invalid plugin state format."))
            })?;

        if let Ok(mut current) = self.shared.current_source.lock() {
            *current = source.clone();
        }

        let feedback = match compile_program(&source) {
            Ok(program) => {
                if let Ok(mut pending) = self.shared.pending_program.lock() {
                    *pending = Some(program);
                }
                CompileFeedback::success("Loaded and compiled saved state.")
            }
            Err(error) => CompileFeedback::error(format!(
                "Loaded source from saved state, but compilation failed: {error}"
            )),
        };

        if let Ok(mut state) = self.shared.compile_feedback.lock() {
            *state = feedback.clone();
        }

        if let Some(gui) = &self.gui {
            gui.send_editor_state(&source, &feedback);
        }

        Ok(())
    }
}

impl PluginAudioPortsImpl for MimiumPluginMainThread<'_> {
    fn count(&mut self, is_input: bool) -> u32 {
        if is_input { 0 } else { 1 }
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if !is_input && index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(0),
                name: b"main",
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            });
        }
    }
}

clack_export_entry!(SinglePluginEntry<MimiumPlugin>);

