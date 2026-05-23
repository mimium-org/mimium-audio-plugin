import { useEffect, useMemo, useState } from "react";
import * as monaco from "monaco-editor";
import { LANGUAGE_ID, registerMimiumLanguage } from "./editor/language";
import { registerThemes } from "./editor/themes";
import {
  onPluginMessage,
  requestClipboardRead,
  requestState,
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
      window.removeEventListener("mousemove", handleWindowMouseMove, true);
      window.removeEventListener("mouseup", handleWindowMouseUp, true);
      mouseDownDisposable.dispose();
      mouseUpDisposable.dispose();
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
