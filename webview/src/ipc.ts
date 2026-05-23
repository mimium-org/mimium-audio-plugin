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
      type: "request_state";
    };

export type PluginMessage = {
  type: "editor_state";
  source: string;
  ok: boolean;
  message: string;
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

export function requestState(): void {
  sendToPlugin({ type: "request_state" });
}

export function onPluginMessage(callback: PluginMessageCallback): () => void {
  listeners.add(callback);
  return () => {
    listeners.delete(callback);
  };
}
