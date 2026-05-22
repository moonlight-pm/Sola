// crates/sola-shell/web/switcher.tsx — root mount for the switcher overlay.
//
// Thin shell: reads the kit-injected initial state and delegates to <Switcher>.

import { type Handle } from "@remix-run/ui";
import { Switcher, type SwitcherInitial } from "./components/switcher/switcher";

export function Main(handle: Handle<{ initial: SwitcherInitial | null }>) {
  const initial: SwitcherInitial = handle.props.initial ?? {
    visible: false,
    apps: [],
    selected: 0,
  };
  return () => <Switcher initial={initial} />;
}
