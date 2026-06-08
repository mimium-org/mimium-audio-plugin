import { useEffect, useMemo, useRef, useState } from "react";
import * as monaco from "monaco-editor";
import { DawKnob } from "./components/DawKnob";
import logoSvg from "../assets/mimium_logo_slant.svg?raw";
import { LANGUAGE_ID, registerMimiumLanguage } from "./editor/language";
import { registerThemes } from "./editor/themes";
import {
  loadExample,
  onPluginMessage,
  requestClipboardRead,
  requestExamples,
  requestGlobalSettings,
  requestState,
  saveGlobalSettings,
  setKnob,
  setSource,
  writeClipboardText,
} from "./ipc";

self.MonacoEnvironment = {
  getWorker(_moduleId: string, _label: string) {
    return new Worker(
      new URL("monaco-editor/esm/vs/editor/editor.worker.js", import.meta.url),
      { type: "module" }
    );
  }
};

registerMimiumLanguage();
registerThemes();

const DEFAULT_SOURCE = `let twopi = 6.283185307179586

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
}`;

const DEFAULT_KNOBS = Array.from({ length: 8 }, (_, index) => ({
  index,
  name: `Knob ${index + 1}`,
  value: 0.5,
}));

const LOGO_URL = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(logoSvg)}`;

function App() {
  const [source, setSourceText] = useState(DEFAULT_SOURCE);
  const [isDrawerOpen, setIsDrawerOpen] = useState(true);
  const [isExamplesOpen, setIsExamplesOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [knobs, setKnobs] = useState(DEFAULT_KNOBS);
  const [compileError, setCompileError] = useState<string | null>(null);
  const [libraryPath, setLibraryPath] = useState("");
  const [saveResult, setSaveResult] = useState<string | null>(null);
  const [examples, setExamples] = useState<
    Array<{ filename: string }>
  >([]);
  const [aboutInfo, setAboutInfo] = useState<{
    plugin_version: string;
    mimium_compiler_version: string;
    repository_url: string;
  } | null>(null);

  const editorContainerId = useMemo(() => "editor-container", []);
  const hasInitialStateRef = useRef(false);
  const skipNextSyncCompileRef = useRef(false);
  const draggingKnobIndexRef = useRef<number | null>(null);

  useEffect(() => {
    const editor = monaco.editor.create(document.getElementById(editorContainerId)!, {
      value: source,
      language: LANGUAGE_ID,
      theme: "mimium-copper",
      dragAndDrop: false,
      fontFamily: "'Iosevka Comfy', 'Fira Code', 'JetBrains Mono', monospace",
      fontLigatures: true,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      fontSize: 16,
      lineNumbers: "on",
      tabSize: 2,
      insertSpaces: true,
      automaticLayout: true,
      padding: { top: 12, bottom: 14 },
      overviewRulerLanes: 0,
      hideCursorInOverviewRuler: true,
      overviewRulerBorder: false,
      scrollbar: {
        verticalScrollbarSize: 9,
        horizontalScrollbarSize: 9,
      },
      mouseStyle: "text",
      contextmenu: false,
    });

    let dragAnchor: monaco.Position | null = null;

    const updateDragSelection = (clientX: number, clientY: number) => {
      if (!dragAnchor) {
        return;
      }

      const target = editor.getTargetAtClientPoint(clientX, clientY);
      const position = target?.position;
      if (!position) {
        return;
      }

      editor.setSelection(
        new monaco.Selection(
          dragAnchor.lineNumber,
          dragAnchor.column,
          position.lineNumber,
          position.column
        )
      );
    };

    const mouseDownDisposable = editor.onMouseDown((event) => {
      if (event.event.leftButton && event.target.position) {
        dragAnchor = event.target.position;
      }
    });

    const mouseUpDisposable = editor.onMouseUp(() => {
      dragAnchor = null;
    });

    const handleWindowMouseMove = (event: MouseEvent) => {
      if (!dragAnchor) {
        return;
      }

      if ((event.buttons & 1) === 0) {
        dragAnchor = null;
        return;
      }

      updateDragSelection(event.clientX, event.clientY);
    };

    const handleWindowMouseUp = () => {
      dragAnchor = null;
    };

    window.addEventListener("mousemove", handleWindowMouseMove, true);
    window.addEventListener("mouseup", handleWindowMouseUp, true);

    const disposable = editor.onDidChangeModelContent(() => {
      setSourceText(editor.getValue());
    });

    let pendingPasteRequestId: string | null = null;

    const insertTextAtSelection = (text: string) => {
      const selection = editor.getSelection();
      if (!selection) {
        return;
      }
      editor.executeEdits("clipboard-paste", [{ range: selection, text }]);
    };

    const writeSelectionToClipboard = () => {
      const selection = editor.getSelection();
      if (!selection || selection.isEmpty()) {
        return false;
      }

      const text = editor.getModel()?.getValueInRange(selection) ?? "";
      if (text.length === 0) {
        return false;
      }

      writeClipboardText(text);
      return true;
    };

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyC, () => {
      writeSelectionToClipboard();
    });

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyX, () => {
      const selection = editor.getSelection();
      if (!selection || selection.isEmpty()) {
        return;
      }

      if (writeSelectionToClipboard()) {
        editor.executeEdits("clipboard-cut", [{ range: selection, text: "" }]);
      }
    });

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyV, () => {
      const requestId = `paste-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      pendingPasteRequestId = requestId;
      requestClipboardRead(requestId);
    });

    requestState();
    requestGlobalSettings();
    requestExamples();

    const unsubscribe = onPluginMessage((message) => {
      if (message.type === "clipboard_read_result") {
        if (message.request_id === pendingPasteRequestId) {
          pendingPasteRequestId = null;
          if (message.ok && typeof message.text === "string") {
            insertTextAtSelection(message.text);
          }
        }
        return;
      }

      if (message.type === "knob_state") {
        setKnobs((current) => {
          const draggingIndex = draggingKnobIndexRef.current;
          if (draggingIndex == null) {
            return message.knobs;
          }

          const currentByIndex = new Map(current.map((knob) => [knob.index, knob]));
          return message.knobs.map((knob) => {
            if (knob.index !== draggingIndex) {
              return knob;
            }

            const local = currentByIndex.get(knob.index);
            if (!local) {
              return knob;
            }

            return { ...knob, value: local.value };
          });
        });
        return;
      }

      if (message.type === "global_settings") {
        setLibraryPath(message.settings.library_path);
        return;
      }

      if (message.type === "save_settings_result") {
        setSaveResult(message.message);
        return;
      }

      if (message.type === "example_list") {
        setExamples(message.examples);
        return;
      }

      if (message.type === "about_info") {
        setAboutInfo(message.about);
        return;
      }

      if (message.type !== "editor_state") {
        return;
      }

      hasInitialStateRef.current = true;
      if (message.ok) {
        setCompileError(null);
      } else {
        setCompileError(message.message);
      }

      if (message.source !== editor.getValue()) {
        skipNextSyncCompileRef.current = true;
        const selection = editor.getSelection();
        editor.setValue(message.source);
        if (selection) {
          editor.setSelection(selection);
        }
      }
    });

    return () => {
      window.removeEventListener("mousemove", handleWindowMouseMove, true);
      window.removeEventListener("mouseup", handleWindowMouseUp, true);
      mouseDownDisposable.dispose();
      mouseUpDisposable.dispose();
      unsubscribe();
      disposable.dispose();
      editor.dispose();
    };
  }, [editorContainerId]);

  useEffect(() => {
    if (!hasInitialStateRef.current) {
      return;
    }

    if (skipNextSyncCompileRef.current) {
      skipNextSyncCompileRef.current = false;
      return;
    }

    const timer = window.setTimeout(() => {
      setSource(source);
    }, 350);

    return () => {
      window.clearTimeout(timer);
    };
  }, [source]);

  const handleKnobNameChange = (index: number, name: string) => {
    setKnobs((current) =>
      current.map((knob) => (knob.index === index ? { ...knob, name } : knob))
    );
  };

  const commitKnobName = (index: number, name: string) => {
    setKnob(index, { name });
  };

  const handleKnobValueChange = (index: number, value: number) => {
    setKnobs((current) =>
      current.map((knob) => (knob.index === index ? { ...knob, value } : knob))
    );
    setKnob(index, { value });
  };

  const handleSaveSettings = () => {
    saveGlobalSettings(libraryPath);
  };

  const handleLoadExample = (filename: string) => {
    loadExample(filename);
    setIsExamplesOpen(false);
  };

  return (
    <div className="page">
      <div className="background-grid" />
      <header className="logo-bar" aria-label="mimium logo">
        <button
          className="chrome-icon-button left"
          onClick={() => setIsExamplesOpen((open) => !open)}
          title="Examples"
          aria-label="Examples"
        >
          ☰
        </button>
        <img className="logo-image" src={LOGO_URL} alt="mimium" />
        <button
          className="chrome-icon-button right"
          onClick={() => setIsSettingsOpen(true)}
          title="Settings"
          aria-label="Settings"
        >
          ⚙
        </button>
      </header>
      {isExamplesOpen && (
        <aside className="examples-panel" role="complementary" aria-label="Examples">
          <div className="panel-title">Examples</div>
          <div className="examples-list">
            {examples.map((example) => (
              <button
                key={example.filename}
                className="example-item"
                onClick={() => handleLoadExample(example.filename)}
              >
                <div className="example-name">{example.filename}</div>
              </button>
            ))}
          </div>
        </aside>
      )}
      <main className={`editor-shell ${isDrawerOpen ? "editor-shell-with-drawer" : ""}`}>
        <div id={editorContainerId} className="editor-container" />
      </main>
      {isSettingsOpen && (
        <div
          className="settings-overlay"
          role="dialog"
          aria-modal="true"
          aria-label="Settings"
          onClick={() => setIsSettingsOpen(false)}
        >
          <section className="settings-modal" onClick={(event) => event.stopPropagation()}>
            <div className="settings-header">
              <div className="settings-title">Plugin Settings</div>
              <button
                className="settings-close-button"
                onClick={() => setIsSettingsOpen(false)}
                aria-label="Close settings"
                title="Close"
              >
                ×
              </button>
            </div>
            <label className="settings-label" htmlFor="library-path-input">
              mimium library path
            </label>
            <input
              id="library-path-input"
              className="settings-input"
              value={libraryPath}
              onChange={(event) => setLibraryPath(event.target.value)}
            />
            <div className="settings-buttons">
              <button className="settings-button" onClick={handleSaveSettings}>
                Save
              </button>
            </div>
            {saveResult && <div className="settings-note">{saveResult}</div>}
            <div className="about-block">
              <div className="panel-title">About</div>
              <div className="about-row">Plugin: {aboutInfo?.plugin_version ?? "-"}</div>
              <div className="about-row">
                mimium compiler: {aboutInfo?.mimium_compiler_version ?? "-"}
              </div>
              <a className="about-link" href={aboutInfo?.repository_url} target="_blank" rel="noreferrer">
                {aboutInfo?.repository_url ?? "https://github.com/mimium-org/mimium-audio-plugin"}
              </a>
            </div>
          </section>
        </div>
      )}
      {compileError && (
        <aside className="error-float" role="alert" aria-live="assertive">
          <div className="error-float-title">Compile Error</div>
          <pre className="error-float-message">{compileError}</pre>
        </aside>
      )}
      <section
        className={`knob-drawer ${isDrawerOpen ? "knob-drawer-open" : "knob-drawer-closed"}`}
        aria-label="automation knobs"
      >
        <button
          className="drawer-toggle"
          onClick={() => setIsDrawerOpen((open) => !open)}
          aria-expanded={isDrawerOpen}
          title={isDrawerOpen ? "Hide knobs" : "Show knobs"}
          aria-label={isDrawerOpen ? "Hide knobs" : "Show knobs"}
        >
          <span aria-hidden="true">{isDrawerOpen ? "▼" : "▲"}</span>
        </button>
        <div className="knob-row">
          {knobs.map((knob) => (
            <div key={knob.index} className="knob-mini">
              <div className="knob-mini-meta">
                <span>#{knob.index + 1}</span>
                <span>{knob.value.toFixed(2)}</span>
              </div>
              <div className="knob-canvas-wrap">
                <DawKnob
                  size={56}
                  value={knob.value}
                  onChange={(next) => handleKnobValueChange(knob.index, next)}
                  onDragStart={() => {
                    draggingKnobIndexRef.current = knob.index;
                  }}
                  onDragEnd={() => {
                    if (draggingKnobIndexRef.current === knob.index) {
                      draggingKnobIndexRef.current = null;
                    }
                  }}
                />
              </div>
              <input
                className="knob-name"
                value={knob.name}
                onChange={(event) => handleKnobNameChange(knob.index, event.target.value)}
                onBlur={(event) => commitKnobName(knob.index, event.target.value)}
              />
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

export default App;
