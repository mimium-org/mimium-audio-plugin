use crate::examples;
use crate::mimium::{self, CompileFeedback};
use crate::params::{KnobBank, KNOB_COUNT};
use crate::settings::{plugin_about_info, save_global_settings, AboutInfo, GlobalSettings};
use crate::webview_gui::{EmbeddedWebviewConfig, EmbeddedWebviewGui};
use crate::{MimiumPluginMainThread, MimiumPluginShared};
use arboard::Clipboard;
use clack_extensions::gui::*;
use clack_plugin::prelude::*;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const GUI_CONFIG: EmbeddedWebviewConfig = EmbeddedWebviewConfig {
    width: 1180,
    height: 760,
    debug_url: "http://localhost:5173",
    release_html: include_str!("../../webview/dist/index.html"),
};

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PluginMessage {
    EditorState {
        source: String,
        ok: bool,
        message: String,
    },
    ClipboardReadResult {
        request_id: String,
        ok: bool,
        text: Option<String>,
        message: String,
    },
    KnobState {
        knobs: Vec<KnobUiState>,
    },
    GlobalSettings {
        settings: GlobalSettings,
    },
    SaveSettingsResult {
        ok: bool,
        message: String,
    },
    ExampleList {
        examples: Vec<ExampleSummary>,
    },
    AboutInfo {
        about: AboutInfo,
    },
}

#[derive(Serialize)]
struct KnobUiState {
    index: usize,
    name: String,
    value: f64,
}

#[derive(Serialize)]
struct ExampleSummary {
    filename: String,
}

pub struct MimiumPluginGui {
    webview: EmbeddedWebviewGui,
}

impl MimiumPluginGui {
    pub fn new(parent: Window<'_>, shared: &MimiumPluginShared) -> Result<Self, PluginError> {
        let current_source = Arc::clone(&shared.current_source);
        let compile_feedback = Arc::clone(&shared.compile_feedback);
        let pending_program = Arc::clone(&shared.pending_program);
        let pending_ui_messages = Arc::clone(&shared.pending_ui_messages);
        let state_dirty = Arc::clone(&shared.state_dirty);
        let knobs = Arc::clone(&shared.knobs);
        let global_settings = Arc::clone(&shared.global_settings);
        let host = unsafe { shared.host.with_arbitrary_lifetime() };

        let webview = EmbeddedWebviewGui::new(parent, &GUI_CONFIG, move |msg| {
            match msg.get("type").and_then(|value| value.as_str()) {
                Some("set_source") => {
                    let source = msg
                        .get("source")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string();

                    if let Ok(mut current) = current_source.lock() {
                        *current = source.clone();
                    }

                    state_dirty.store(true, Ordering::SeqCst);

                    let feedback = compile_feedback
                        .lock()
                        .map(|state| state.clone())
                        .unwrap_or_else(|_| {
                            CompileFeedback::error("Failed to read compile status.".to_string())
                        });

                    queue_editor_state(&pending_ui_messages, &source, &feedback);
                    queue_knob_state(&pending_ui_messages, &knobs);
                    queue_global_settings(&pending_ui_messages, &global_settings);
                    queue_example_list(&pending_ui_messages);
                    queue_about_info(&pending_ui_messages);
                    host.request_callback();
                }
                Some("compile_source") => {
                    let source = current_source.lock().map(|value| value.clone()).unwrap_or_default();

                    let library_path = global_settings
                        .lock()
                        .map(|settings| settings.library_path.clone())
                        .unwrap_or_default();

                    let feedback = match mimium::compile_program(
                        &source,
                        Arc::clone(&knobs),
                        Some(library_path.as_str()),
                    ) {
                        Ok(program) => {
                            knobs.set_names(program.resolved_knob_names.clone());
                            if let Ok(mut pending) = pending_program.lock() {
                                *pending = Some(program);
                            }
                            CompileFeedback::success(
                                "Compiled via worker process. The audio thread will swap the program on the next block.",
                            )
                        }
                        Err(error) => CompileFeedback::error(error),
                    };

                    if let Ok(mut state) = compile_feedback.lock() {
                        *state = feedback.clone();
                    }

                    queue_editor_state(&pending_ui_messages, &source, &feedback);
                    queue_knob_state(&pending_ui_messages, &knobs);
                    host.request_callback();
                }
                Some("request_state") => {
                    let source = current_source.lock().map(|value| value.clone()).unwrap_or_default();
                    let feedback = compile_feedback
                        .lock()
                        .map(|value| value.clone())
                        .unwrap_or_else(|_| {
                            CompileFeedback::error("Failed to read plugin state.".to_string())
                        });
                    queue_editor_state(&pending_ui_messages, &source, &feedback);
                    queue_knob_state(&pending_ui_messages, &knobs);
                    queue_global_settings(&pending_ui_messages, &global_settings);
                    queue_example_list(&pending_ui_messages);
                    queue_about_info(&pending_ui_messages);
                    host.request_callback();
                }
                Some("set_knob") => {
                    let index = msg
                        .get("index")
                        .and_then(|value| value.as_u64())
                        .map(|value| value as usize)
                        .unwrap_or(usize::MAX);

                    if index >= KNOB_COUNT {
                        return;
                    }

                    if let Some(name) = msg.get("name").and_then(|value| value.as_str()) {
                        knobs.set_name(index, name.to_string());
                    }

                    if let Some(value) = msg.get("value").and_then(|value| value.as_f64()) {
                        knobs.set_value(index, value);
                    }

                    state_dirty.store(true, Ordering::SeqCst);
                    queue_knob_state(&pending_ui_messages, &knobs);
                    host.request_callback();
                }
                Some("request_global_settings") => {
                    queue_global_settings(&pending_ui_messages, &global_settings);
                    queue_about_info(&pending_ui_messages);
                    host.request_callback();
                }
                Some("save_global_settings") => {
                    let library_path = msg
                        .get("library_path")
                        .and_then(|value| value.as_str())
                        .map(|value| value.trim().to_string())
                        .unwrap_or_default();

                    let result = if library_path.is_empty() {
                        Err("Library path must not be empty.".to_string())
                    } else {
                        let mut next = global_settings
                            .lock()
                            .map(|settings| settings.clone())
                            .unwrap_or_default();
                        next.library_path = library_path;
                        match save_global_settings(&next) {
                            Ok(()) => {
                                if let Ok(mut settings) = global_settings.lock() {
                                    *settings = next;
                                }
                                Ok(())
                            }
                            Err(error) => Err(error),
                        }
                    };

                    queue_save_settings_result(
                        &pending_ui_messages,
                        result.is_ok(),
                        result
                            .err()
                            .unwrap_or_else(|| "Saved global settings.".to_string())
                            .as_str(),
                    );
                    queue_global_settings(&pending_ui_messages, &global_settings);
                    host.request_callback();
                }
                Some("request_examples") => {
                    queue_example_list(&pending_ui_messages);
                    host.request_callback();
                }
                Some("load_example") => {
                    let Some(filename) = msg.get("filename").and_then(|value| value.as_str()) else {
                        return;
                    };

                    let Some(source) =
                        examples::find_example_source(filename).map(|value| value.to_string())
                    else {
                        return;
                    };

                    if let Ok(mut current) = current_source.lock() {
                        *current = source.clone();
                    }

                    state_dirty.store(true, Ordering::SeqCst);

                    let feedback = CompileFeedback::success(
                        "Loaded example source. Press Compile to apply.",
                    );

                    if let Ok(mut state) = compile_feedback.lock() {
                        *state = feedback.clone();
                    }

                    queue_editor_state(&pending_ui_messages, &source, &feedback);
                    queue_knob_state(&pending_ui_messages, &knobs);
                    queue_example_list(&pending_ui_messages);
                    host.request_callback();
                }
                Some("clipboard_write") => {
                    let text = msg
                        .get("text")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string();

                    if let Ok(mut clipboard) = Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                }
                Some("clipboard_read") => {
                    let request_id = msg
                        .get("request_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string();

                    let result = Clipboard::new()
                        .and_then(|mut clipboard| clipboard.get_text())
                        .map(|text| (true, Some(text), "Clipboard read succeeded.".to_string()))
                        .unwrap_or_else(|error| {
                            (false, None, format!("Clipboard read failed: {error}"))
                        });

                    queue_clipboard_read_result(
                        &pending_ui_messages,
                        &request_id,
                        result.0,
                        result.1,
                        &result.2,
                    );
                    host.request_callback();
                }
                _ => {}
            }
        })?;

        Ok(Self { webview })
    }

    pub fn resize(&self, size: GuiSize) {
        self.webview.resize(size);
    }

    pub fn send_raw_message(&self, json: &str) {
        let js = format!("window.__onPluginMessage && window.__onPluginMessage({json})");
        self.webview.evaluate_script(&js);
    }

    pub fn send_editor_state(&self, source: &str, feedback: &CompileFeedback) {
        if let Ok(json) = serde_json::to_string(&PluginMessage::EditorState {
            source: source.to_string(),
            ok: feedback.ok,
            message: feedback.message.clone(),
        }) {
            self.send_raw_message(&json);
        }
    }

    pub fn send_knob_state(&self, shared: &MimiumPluginShared) {
        if let Ok(json) = serde_json::to_string(&PluginMessage::KnobState {
            knobs: snapshot_knobs(&shared.knobs),
        }) {
            self.send_raw_message(&json);
        }
    }

    pub fn send_global_settings(&self, shared: &MimiumPluginShared) {
        if let Ok(settings) = shared.global_settings.lock() {
            if let Ok(json) = serde_json::to_string(&PluginMessage::GlobalSettings {
                settings: settings.clone(),
            }) {
                self.send_raw_message(&json);
            }
        }
    }

    pub fn send_example_list(&self) {
        if let Ok(json) = serde_json::to_string(&PluginMessage::ExampleList {
            examples: snapshot_examples(),
        }) {
            self.send_raw_message(&json);
        }
    }

    pub fn send_about_info(&self) {
        if let Ok(json) = serde_json::to_string(&PluginMessage::AboutInfo {
            about: plugin_about_info(),
        }) {
            self.send_raw_message(&json);
        }
    }
}

pub(crate) fn queue_clipboard_read_result(
    queue: &std::sync::Mutex<Vec<String>>,
    request_id: &str,
    ok: bool,
    text: Option<String>,
    message: &str,
) {
    if let Ok(json) = serde_json::to_string(&PluginMessage::ClipboardReadResult {
        request_id: request_id.to_string(),
        ok,
        text,
        message: message.to_string(),
    }) {
        if let Ok(mut queue) = queue.lock() {
            queue.push(json);
        }
    }
}

pub(crate) fn queue_editor_state(
    queue: &std::sync::Mutex<Vec<String>>,
    source: &str,
    feedback: &CompileFeedback,
) {
    if let Ok(json) = serde_json::to_string(&PluginMessage::EditorState {
        source: source.to_string(),
        ok: feedback.ok,
        message: feedback.message.clone(),
    }) {
        if let Ok(mut queue) = queue.lock() {
            queue.push(json);
        }
    }
}

pub(crate) fn queue_knob_state(queue: &std::sync::Mutex<Vec<String>>, knobs: &KnobBank) {
    if let Ok(json) = serde_json::to_string(&PluginMessage::KnobState {
        knobs: snapshot_knobs(knobs),
    }) {
        if let Ok(mut queue) = queue.lock() {
            queue.push(json);
        }
    }
}

pub(crate) fn queue_global_settings(
    queue: &std::sync::Mutex<Vec<String>>,
    settings: &Arc<std::sync::Mutex<GlobalSettings>>,
) {
    if let Ok(settings) = settings.lock() {
        if let Ok(json) = serde_json::to_string(&PluginMessage::GlobalSettings {
            settings: settings.clone(),
        }) {
            if let Ok(mut queue) = queue.lock() {
                queue.push(json);
            }
        }
    }
}

pub(crate) fn queue_save_settings_result(
    queue: &std::sync::Mutex<Vec<String>>,
    ok: bool,
    message: &str,
) {
    if let Ok(json) = serde_json::to_string(&PluginMessage::SaveSettingsResult {
        ok,
        message: message.to_string(),
    }) {
        if let Ok(mut queue) = queue.lock() {
            queue.push(json);
        }
    }
}

pub(crate) fn queue_example_list(queue: &std::sync::Mutex<Vec<String>>) {
    if let Ok(json) = serde_json::to_string(&PluginMessage::ExampleList {
        examples: snapshot_examples(),
    }) {
        if let Ok(mut queue) = queue.lock() {
            queue.push(json);
        }
    }
}

pub(crate) fn queue_about_info(queue: &std::sync::Mutex<Vec<String>>) {
    if let Ok(json) = serde_json::to_string(&PluginMessage::AboutInfo {
        about: plugin_about_info(),
    }) {
        if let Ok(mut queue) = queue.lock() {
            queue.push(json);
        }
    }
}

fn snapshot_knobs(knobs: &KnobBank) -> Vec<KnobUiState> {
    let names = knobs.snapshot_names();
    (0..KNOB_COUNT)
        .map(|index| KnobUiState {
            index,
            name: names[index].clone(),
            value: knobs.value(index),
        })
        .collect()
}

fn snapshot_examples() -> Vec<ExampleSummary> {
    examples::builtin_examples()
        .into_iter()
        .map(|example| ExampleSummary {
            filename: example.filename,
        })
        .collect()
}

impl<'a> PluginGuiImpl for MimiumPluginMainThread<'a> {
    fn is_api_supported(&mut self, configuration: GuiConfiguration) -> bool {
        configuration.api_type
            == GuiApiType::default_for_current_platform().expect("Unsupported platform")
            && !configuration.is_floating
    }

    fn get_preferred_api(&mut self) -> Option<GuiConfiguration<'_>> {
        Some(GuiConfiguration {
            api_type: GuiApiType::default_for_current_platform()?,
            is_floating: false,
        })
    }

    fn create(&mut self, configuration: GuiConfiguration) -> Result<(), PluginError> {
        if configuration.is_floating {
            return Err(PluginError::Message("Floating windows are not supported"));
        }
        Ok(())
    }

    fn destroy(&mut self) {
        self.gui = None;
    }

    fn set_scale(&mut self, _scale: f64) -> Result<(), PluginError> {
        Ok(())
    }

    fn get_size(&mut self) -> Option<GuiSize> {
        Some(GuiSize {
            width: GUI_CONFIG.width,
            height: GUI_CONFIG.height,
        })
    }

    fn set_size(&mut self, size: GuiSize) -> Result<(), PluginError> {
        if let Some(gui) = &self.gui {
            gui.resize(size);
        }
        Ok(())
    }

    fn set_parent(&mut self, window: Window) -> Result<(), PluginError> {
        self.gui = Some(MimiumPluginGui::new(window, self.shared)?);

        if let Some(gui) = &self.gui {
            let source = self
                .shared
                .current_source
                .lock()
                .map(|value| value.clone())
                .unwrap_or_default();
            let feedback = self
                .shared
                .compile_feedback
                .lock()
                .map(|value| value.clone())
                .unwrap_or_else(|_| CompileFeedback::error("Failed to read plugin state."));
            gui.send_editor_state(&source, &feedback);
            gui.send_knob_state(self.shared);
            gui.send_global_settings(self.shared);
            gui.send_example_list();
            gui.send_about_info();
        }

        Ok(())
    }

    fn set_transient(&mut self, _window: Window) -> Result<(), PluginError> {
        Ok(())
    }

    fn show(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    fn hide(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
}
