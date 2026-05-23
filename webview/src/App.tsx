import { useEffect, useMemo, useState } from "react";
import * as monaco from "monaco-editor";
import { LANGUAGE_ID, registerMimiumLanguage } from "./editor/language";
import { registerThemes } from "./editor/themes";
import { onPluginMessage, requestState, setSource } from "./ipc";

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
  let left = sin(phasor(220.0) * twopi) * 0.18
  let right = sin(phasor(330.0) * twopi) * 0.18
  (left, right)
}`;

function App() {
  const [status, setStatus] = useState("Waiting for plugin state...");
  const [source, setSourceText] = useState(DEFAULT_SOURCE);
  const [isCompiling, setIsCompiling] = useState(false);

  const editorContainerId = useMemo(() => "editor-container", []);

  useEffect(() => {
    const editor = monaco.editor.create(document.getElementById(editorContainerId)!, {
      value: source,
      language: LANGUAGE_ID,
      theme: "mimium-copper",
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
    });

    const disposable = editor.onDidChangeModelContent(() => {
      setSourceText(editor.getValue());
    });

    requestState();

    const unsubscribe = onPluginMessage((message) => {
      if (message.type !== "editor_state") {
        return;
      }
      setStatus(message.message);
      setIsCompiling(false);

      if (message.source !== editor.getValue()) {
        const selection = editor.getSelection();
        editor.setValue(message.source);
        if (selection) {
          editor.setSelection(selection);
        }
      }
    });

    return () => {
      unsubscribe();
      disposable.dispose();
      editor.dispose();
    };
  }, [editorContainerId]);

  const handleCompile = () => {
    setIsCompiling(true);
    setStatus("Compiling...");
    setSource(source);
  };

  return (
    <div className="page">
      <div className="background-grid" />
      <header className="hero">
        <div>
          <p className="eyebrow">Mimium x CLAP x Clack</p>
          <h1>Live Coded Stereo Synth</h1>
          <p className="subtitle">
            Monaco editor inside a CLAP webview. Source updates are compiled with mimium-rs
            Wasm JIT and swapped in the audio thread.
          </p>
        </div>
        <div className="actions">
          <button className="compile-button" onClick={handleCompile} disabled={isCompiling}>
            {isCompiling ? "Compiling..." : "Compile And Swap"}
          </button>
          <div className="status">{status}</div>
        </div>
      </header>
      <main className="editor-shell">
        <div id={editorContainerId} className="editor-container" />
      </main>
    </div>
  );
}

export default App;
