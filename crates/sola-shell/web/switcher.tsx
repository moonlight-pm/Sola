// crates/sola-shell/web/switcher.tsx — placeholder; real UI lands in T9.
import { type Handle } from "@remix-run/ui";

interface Props {
  initial: unknown;
}

export function Main(_handle: Handle<Props>) {
  return () => (
    <div
      style="background: #181818; color: #fff; font-family: sans-serif; padding: 4px 12px;"
    >
      sola-shell · switcher (placeholder)
    </div>
  );
}
