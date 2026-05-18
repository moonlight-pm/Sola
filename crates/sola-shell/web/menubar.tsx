// crates/sola-shell/web/menubar.tsx — entry point for the menubar window.
//
// Mounts <Menubar> with the initial state seeded from Rust via
// WindowConfig::initial_state. The shape is MenubarInitial:
//   { focused: { app_name: string; menu_labels: string[] } | null }
//
// At startup Rust seeds `{ focused: null }`; subsequent focus changes
// flow through the existing send_to_js path as { event: "focus", … }.

import { type Handle } from "@remix-run/ui";
import { Menubar, type MenubarInitial } from "./components/menubar/menubar";

interface MainProps {
  initial: MenubarInitial | null;
}

export function Main(handle: Handle<MainProps>) {
  const initial: MenubarInitial = handle.props.initial ?? { focused: null };
  return () => <Menubar initial={initial} />;
}
