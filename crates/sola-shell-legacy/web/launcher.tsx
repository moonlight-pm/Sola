// crates/sola-shell/web/launcher.tsx — root entry for the launcher window.
//
// Mounts <Launcher> with the initial state seeded from Rust via
// WindowConfig::initial_state (delivered as handle.props.initial).

import { type Handle } from "@remix-run/ui";
import { Launcher, type LauncherInitial } from "./components/launcher/launcher";

export function Main(handle: Handle<{ initial: LauncherInitial | null }>) {
  const initial: LauncherInitial =
    handle.props.initial ?? { apps: [], selected: 0, query: "" };
  return () => <Launcher initial={initial} />;
}
