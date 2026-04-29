type EventCallback = (data: any) => void;

let nextId = 1;
const pending = new Map<number, { resolve: (v: any) => void; reject: (e: any) => void }>();
const listeners = new Map<string, Set<EventCallback>>();
// Per-event buffer for messages that arrive before any listener is
// registered. Drained on the next `on(event, ...)` call so events
// delivered during page-load or sticky replay aren't silently lost.
const orphanBuffer = new Map<string, any[]>();

// Framework defaults for specific events — used when the app has not
// registered its own handler. Apps that want custom behavior call
// on("copy", ...) / on("paste", ...), which takes precedence.
const defaults = new Map<string, EventCallback>([
  ["copy", () => {
    const sel = window.getSelection()?.toString();
    if (sel) {
      navigator.clipboard.writeText(sel).catch((e) => {
        console.error("default copy failed", e);
      });
    }
  }],
  // The framework reads the clipboard on the Rust side and hands the
  // resolved text to us as `msg.text` — navigator.clipboard.readText() can't
  // be used here because host-injected JS lacks user-activation transient.
  ["paste", (msg: { text?: string }) => {
    if (msg.text) document.execCommand("insertText", false, msg.text);
  }],
]);

// Called from Rust via evaluate_javascript to deliver responses and events.
// A synchronous bootstrap script in <head> (injected by sola-app) installs
// a queueing stub so messages that arrive before this module loads aren't
// dropped. We install the real handler here and drain anything queued.
const recv = (json: string) => {
  const msg = JSON.parse(json);
  if (msg.id !== undefined) {
    const p = pending.get(msg.id);
    if (p) {
      pending.delete(msg.id);
      if (msg.result?.error) {
        p.reject(msg.result.error);
      } else {
        p.resolve(msg.result);
      }
    }
  } else if (msg.event) {
    const cbs = listeners.get(msg.event);
    if (cbs && cbs.size > 0) {
      for (const cb of cbs) cb(msg);
    } else {
      const def = defaults.get(msg.event);
      if (def) {
        def(msg);
      } else {
        let buf = orphanBuffer.get(msg.event);
        if (!buf) {
          buf = [];
          orphanBuffer.set(msg.event, buf);
        }
        buf.push(msg);
      }
    }
  }
};

(window as any).__solaRecv = recv;
const earlyQueue: string[] = (window as any).__solaRecvQueue ?? [];
delete (window as any).__solaRecvQueue;
for (const json of earlyQueue) recv(json);

export function invoke(cmd: string, args: Record<string, any> = {}): Promise<any> {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    (window as any).webkit.messageHandlers.sola.postMessage(
      JSON.stringify({ id, cmd, args })
    );
  });
}

export function on(event: string, callback: EventCallback): () => void {
  if (!listeners.has(event)) {
    listeners.set(event, new Set());
  }
  listeners.get(event)!.add(callback);
  // Drain any messages that arrived before the first listener for this
  // event was registered.
  const buffered = orphanBuffer.get(event);
  if (buffered && buffered.length > 0) {
    orphanBuffer.delete(event);
    for (const msg of buffered) callback(msg);
  }
  return () => { listeners.get(event)?.delete(callback); };
}
