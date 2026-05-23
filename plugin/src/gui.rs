use crate::mimium::{self, CompileFeedback};
use crate::webview_gui::{EmbeddedWebviewConfig, EmbeddedWebviewGui};
use crate::{MimiumPluginMainThread, MimiumPluginShared};
use arboard::Clipboard;
use clack_extensions::gui::*;
use clack_plugin::prelude::*;
use serde::Serialize;
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

                    let feedback = match mimium::compile_program(&source) {
                        Ok(program) => {
                            if let Ok(mut pending) = pending_program.lock() {
                                *pending = Some(program);
                            }
                            CompileFeedback::success(
                                "Compiled. The audio thread will swap the program on the next block.",
                            )
                        }
                        Err(error) => CompileFeedback::error(error),
                    };

                    if let Ok(mut state) = compile_feedback.lock() {
                        *state = feedback.clone();
                    }

                    queue_editor_state(&pending_ui_messages, &source, &feedback);
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
