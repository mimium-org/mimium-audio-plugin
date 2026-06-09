declare global {
  interface Window {
    ipc?: {
      postMessage: (message: string) => void;
    };
    __onPluginMessage?: (message: PluginMessage) => void;
  }
}

export type PluginCommand =
  | {
      type: "set_source";
      source: string;
    }
  | {
      type: "compile_source";
    }
  | {
      type: "request_state";
    }
  | {
      type: "clipboard_write";
      text: string;
    }
  | {
      type: "clipboard_read";
      request_id: string;
    }
  | {
      type: "set_knob";
      index: number;
      name?: string;
      value?: number;
    }
  | {
      type: "request_global_settings";
    }
  | {
      type: "save_global_settings";
      library_path: string;
    }
  | {
      type: "request_examples";
    }
  | {
      type: "load_example";
      filename: string;
    };

export type PluginMessage =
  | {
      type: "editor_state";
      source: string;
      ok: boolean;
      message: string;
    }
  | {
      type: "clipboard_read_result";
      request_id: string;
      ok: boolean;
      text: string | null;
      message: string;
    }
  | {
      type: "knob_state";
      knobs: Array<{
        index: number;
        name: string;
        value: number;
      }>;
    }
  | {
      type: "global_settings";
      settings: {
        library_path: string;
      };
    }
  | {
      type: "save_settings_result";
      ok: boolean;
      message: string;
    }
  | {
      type: "example_list";
      examples: Array<{
        filename: string;
      }>;
    }
  | {
      type: "about_info";
      about: {
        plugin_version: string;
        mimium_compiler_version: string;
        repository_url: string;
      };
    };

type PluginMessageCallback = (message: PluginMessage) => void;

const listeners = new Set<PluginMessageCallback>();

function dispatchPluginMessage(message: PluginMessage): void {
  for (const listener of listeners) {
    listener(message);
  }
}

window.__onPluginMessage = dispatchPluginMessage;

function sendToPlugin(message: PluginCommand): void {
  const postMessage = window.ipc?.postMessage;
  if (!postMessage) {
    return;
  }

  postMessage(JSON.stringify(message));
}

export function setSource(source: string): void {
  sendToPlugin({ type: "set_source", source });
}

export function compileSource(): void {
  sendToPlugin({ type: "compile_source" });
}

export function requestState(): void {
  sendToPlugin({ type: "request_state" });
}

export function writeClipboardText(text: string): void {
  sendToPlugin({ type: "clipboard_write", text });
}

export function requestClipboardRead(requestId: string): void {
  sendToPlugin({ type: "clipboard_read", request_id: requestId });
}

export function setKnob(index: number, payload: { name?: string; value?: number }): void {
  sendToPlugin({
    type: "set_knob",
    index,
    ...payload,
  });
}

export function requestGlobalSettings(): void {
  sendToPlugin({ type: "request_global_settings" });
}

export function saveGlobalSettings(libraryPath: string): void {
  sendToPlugin({ type: "save_global_settings", library_path: libraryPath });
}

export function requestExamples(): void {
  sendToPlugin({ type: "request_examples" });
}

export function loadExample(filename: string): void {
  sendToPlugin({ type: "load_example", filename });
}

export function onPluginMessage(callback: PluginMessageCallback): () => void {
  listeners.add(callback);
  return () => {
    listeners.delete(callback);
  };
}
