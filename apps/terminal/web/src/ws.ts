type EventCallback = (data: any) => void;

let socket: WebSocket | null = null;
let nextId = 1;
const pending = new Map<number, { resolve: (v: any) => void; reject: (e: any) => void }>();
const listeners = new Map<string, Set<EventCallback>>();

export function connect(port: number): Promise<void> {
  return new Promise((resolve, reject) => {
    socket = new WebSocket(`ws://127.0.0.1:${port}`);
    socket.onopen = () => resolve();
    socket.onerror = (e) => reject(e);
    socket.onmessage = (e) => {
      const msg = JSON.parse(e.data);
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
    socket.onclose = () => { socket = null; };
  });
}

export function invoke(cmd: string, args: Record<string, any> = {}): Promise<any> {
  return new Promise((resolve, reject) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      reject('WebSocket not connected');
      return;
    }
    const id = nextId++;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, cmd, args }));
  });
}

export function on(event: string, callback: EventCallback): () => void {
  if (!listeners.has(event)) {
    listeners.set(event, new Set());
  }
  listeners.get(event)!.add(callback);
  return () => { listeners.get(event)?.delete(callback); };
}
