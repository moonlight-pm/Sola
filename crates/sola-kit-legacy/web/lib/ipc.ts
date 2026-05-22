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
    if (msg.text) insertTextAtCaret(msg.text);
  }],
]);

// Modern replacement for the deprecated `document.execCommand("insertText",
// …)`. Splits on element type because the spec'd APIs do:
//   - `<input>` / `<textarea>` — `setRangeText` replaces the selected range
//     and parks the caret at the end of the inserted text. It does not fire
//     an input event, so we synthesise one ourselves; controlled-input
//     bindings in every framework we use (signals, Lit, React …) listen for
//     `input`, not `change`.
//   - contenteditable — `Selection` + `Range.insertNode`. Mirrors what the
//     browser does internally for typed text.
// Tradeoff vs. `execCommand`: native undo entries are not produced. That's
// also true of `execCommand` in modern Chrome paths, so it's a wash for us.
function insertTextAtCaret(text: string) {
  const el = document.activeElement;
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const s = el.selectionStart ?? el.value.length;
    const e = el.selectionEnd ?? s;
    el.setRangeText(text, s, e, "end");
    el.dispatchEvent(new InputEvent("input", {
      bubbles: true,
      inputType: "insertFromPaste",
      data: text,
    }));
    return;
  }
  if (el instanceof HTMLElement && el.isContentEditable) {
    const sel = window.getSelection();
    if (!sel?.rangeCount) return;
    const range = sel.getRangeAt(0);
    range.deleteContents();
    const node = document.createTextNode(text);
    range.insertNode(node);
    range.setStartAfter(node);
    sel.removeAllRanges();
    sel.addRange(range);
  }
}

// Called from Rust via CefFrame::execute_java_script to deliver responses
// and events. A synchronous bootstrap script in <head> (injected by
// `inject_solarecv_bootstrap` in the kit) installs a queueing stub so
// messages that arrive before this module loads aren't dropped. We install
// the real handler here and drain anything queued.
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

// CEF MessageRouter installs `window.cefQuery` on every V8 context. The
// returned Promise here resolves through the `__solaRecv` reply channel
// (correlated by `id` via the `pending` map above), not via cefQuery's
// own onSuccess — the Rust handler in `crates/sola-kit/src/cef/router.rs`
// always acks success_str("") immediately, then the app sends the real
// result back via `WindowHandle::send_to_js`. cefQuery's onFailure is the
// only signal we care about (e.g. the renderer crashed mid-query).
declare global {
  interface Window {
    cefQuery?: (args: {
      request: string;
      persistent?: boolean;
      onSuccess: (response: string) => void;
      onFailure: (errorCode: number, errorMessage: string) => void;
    }) => number;
  }
}

/**
 * Send a command to the Rust side and resolve with its reply.
 *
 * The wire flow is asymmetric on purpose — cefQuery itself is fire-and-ack,
 * not request/response, so we ride two channels:
 *
 *   1. **Outbound:** `cefQuery({ request: '{"id":N,"cmd":...,"args":...}' })`
 *      hits the renderer-side MessageRouter, which marshals the request
 *      into an IPC message to the browser process.
 *   2. **Inbound:** Rust handles the command and ships the reply back via
 *      `WindowHandle::send_to_js → execute_java_script("__solaRecv(...)")`.
 *      The reply is matched to the originating call by `id` (see the
 *      `pending` Map at the top of this module).
 *
 * `cefQuery`'s own `onSuccess` is intentionally a no-op — the Rust browser
 * handler always acks `success_str("")` so the query callback completes,
 * but the *real* result rides the `__solaRecv` reply path. `onFailure`
 * fires only on transport errors (renderer crashed mid-query, etc.) and
 * rejects the matching pending Promise.
 *
 * Returns a Promise that resolves with the `result` field of the Rust
 * reply, or rejects with that result's `error` field (or any transport
 * error).
 *
 * @example
 *   const themes = await invoke("list_themes");
 *   await invoke("theme_set", { theme: { ... } });
 */
export function invoke(cmd: string, args: Record<string, any> = {}): Promise<any> {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    if (!window.cefQuery) {
      pending.delete(id);
      reject(new Error("cefQuery not installed (renderer not initialized?)"));
      return;
    }
    window.cefQuery({
      request: JSON.stringify({ id, cmd, args }),
      onSuccess: () => {},
      onFailure: (code, msg) => {
        const p = pending.get(id);
        if (p) {
          pending.delete(id);
          p.reject(new Error(`cefQuery failed (${code}): ${msg}`));
        }
      },
    });
  });
}

/**
 * Subscribe to one-way events pushed from the Rust side.
 *
 * Rust→JS events arrive through the same `__solaRecv` channel as
 * `invoke` replies, distinguished by the absence of an `id` field and
 * the presence of an `event` field. Common sources:
 *
 *   - sticky bus topics replayed on first connect (e.g. `Theme`)
 *   - live bus deliveries the kit's framework converts into events
 *   - per-app pushes from `WindowHandle::send_to_js({ event, ... })`
 *
 * Multiple listeners can subscribe to the same event; each call adds one
 * to the Set. Returns a dispose function that removes just the registered
 * callback — caller is responsible for calling it on unmount.
 *
 * **Event ordering guarantee:** if a message for `event` arrives *before*
 * any listener registers, it is buffered in `orphanBuffer` and replayed
 * on the next `on(event, …)` call for that event. This matters for
 * sticky-topic replay during page-load; without buffering, the sticky
 * Theme delivery would fire before `index.tsx` had a chance to install
 * its listener and the page would render unthemed.
 *
 * **Default fallback:** if no listener and no buffered messages exist,
 * the framework's `defaults` Map is consulted for a sensible no-op
 * (e.g. clipboard copy/paste plumbing). Apps that register their own
 * `copy`/`paste` listeners take precedence over the defaults.
 *
 * @example
 *   const off = on("theme", (msg: { css?: string }) => {
 *     if (msg.css) sheet.replaceSync(msg.css);
 *   });
 *   // later:
 *   off();
 */
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
