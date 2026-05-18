// crates/sola-shell/web/menu.tsx — kit entry point for the menu surface.
import { type Handle } from "@remix-run/ui";
import { Menu, type MenuInitial } from "./components/menu/menu";

export function Main(handle: Handle<{ initial: MenuInitial | null }>) {
  const initial: MenuInitial = handle.props.initial ?? { visible: false };
  return () => <Menu initial={initial} />;
}
