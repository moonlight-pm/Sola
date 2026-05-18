// crates/sola-shell/web/menu.tsx — placeholder; real UI lands in T8.
import { type Handle } from "@remix-run/ui";

interface Props {
  initial: unknown;
}

export function Main(_handle: Handle<Props>) {
  return () => (
    <div
      style="background: #181818; color: #fff; font-family: sans-serif; padding: 4px 12px;"
    >
      sola-shell · menu (placeholder)
    </div>
  );
}
