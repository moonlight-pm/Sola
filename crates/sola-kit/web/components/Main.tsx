import { on, type Handle } from "@remix-run/ui";
import { invoke } from "@sola/ipc";

// Spike component proving end-to-end:
//   1. JSX → @remix-run/ui/jsx-runtime (auto-imported by swc)
//   2. handle-based component model with closure-captured state
//   3. on() mixin event binding via the `mix` prop
//   4. async IPC roundtrip (invoke → cefQuery → Rust → __solaRecv → resolve)
//      driving a re-render via handle.update()
export function Main(handle: Handle) {
  let count = 0;
  let pong = "(no ping yet)";

  invoke("ping", { from: "Main" }).then((res) => {
    pong = JSON.stringify(res);
    handle.update();
  });

  return () => (
    <main style="font: 14px/1.5 system-ui; padding: 2rem;">
      <h1>sola-kit · remix v3</h1>
      <p>count: {count}</p>
      <button
        mix={[on("click", () => { count++; handle.update(); })]}
      >+1</button>
      <p>ipc: {pong}</p>
    </main>
  );
}
