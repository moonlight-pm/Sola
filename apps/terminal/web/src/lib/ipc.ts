type EventCallback = (data: any) => void;

let nextId = 1;
const pending = new Map<number, { resolve: (v: any) => void; reject: (e: any) => void }>();
const listeners = new Map<string, Set<EventCallback>>();

// Called from Rust via evaluate_javascript to deliver responses and events.
(window as any).__solaRecv = (json: string) => {
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
    if (cbs) {
      for (const cb of cbs) cb(msg);
    }
  }
};

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
  return () => { listeners.get(event)?.delete(callback); };
}
