// <Menubar> — root component for the sola-shell menubar window.
//
// Composes <SystemMenu>, <AppTitle>, <MenuLabels>, and <Tray>.
// Holds all open/close state and dispatches to children via props.
//
// IPC contract (Rust → JS):
//   { event: "focus",      app_name: string, menu_labels: string[] }
//   { event: "close_menu" }
//   { event: "toast",      message: string }   — handled by <Tray>
//
// IPC contract (JS → Rust via invoke):
//   invoke("open_menu",  { source, index, anchor_x })
//   invoke("close_menu", {})

import { type Handle } from "@remix-run/ui";
import { on as ipcOn, invoke } from "@sola/ipc";
import { on } from "@sola/kit";
import { SystemMenu } from "./system-menu";
import { AppTitle } from "./app-title";
import { MenuLabels } from "./menu-labels";
import { Tray } from "./tray";

export interface MenubarFocused {
  app_name: string;
  menu_labels: string[];
}

export interface MenubarInitial {
  focused: MenubarFocused | null;
}

export function Menubar(handle: Handle<{ initial: MenubarInitial }>) {
  // ── Closure-captured state ──────────────────────────────────────────
  const initial = handle.props.initial;
  let appName: string = initial.focused?.app_name ?? "";
  let menuLabels: string[] = initial.focused?.menu_labels ?? [];
  // openKey: null | "system" | "app:0" | "app:1" | ...
  let openKey: string | null = null;

  // ── Bus envelope subscriptions ──────────────────────────────────────
  ipcOn("focus", (msg: any) => {
    appName = msg.app_name ?? "";
    menuLabels = msg.menu_labels ?? [];
    openKey = null;
    handle.update();
  });

  ipcOn("close_menu", () => {
    openKey = null;
    handle.update();
  });

  // ── Helpers ─────────────────────────────────────────────────────────
  const isOpen = (key: string) => openKey === key;

  const showMenu = (key: string, source: string, index: number, anchorX: number) => {
    openKey = key;
    handle.update();
    invoke("open_menu", { source, index, anchor_x: anchorX });
  };

  const dismissMenu = () => {
    if (openKey === null) return;
    openKey = null;
    handle.update();
    invoke("close_menu", {});
  };

  const click = (key: string, source: string, index: number, anchorX: number) => {
    if (openKey === key) {
      dismissMenu();
      return;
    }
    showMenu(key, source, index, anchorX);
  };

  const hover = (key: string, source: string, index: number, anchorX: number) => {
    // Only re-open if a different menu is already open — same guard as legacy.
    if (openKey === null || openKey === key) return;
    showMenu(key, source, index, anchorX);
  };

  // Document-level click dismisses the open menu — matches legacy
  // `document.addEventListener('click', dismissMenu)`.
  document.addEventListener("click", () => dismissMenu());

  return () => (
    <div class="sola-menubar" mix={[]}>
      <div class="sola-menubar-left">
        <SystemMenu
          open={isOpen("system")}
          onClick={(x) => click("system", "system", 0, x)}
          onHover={(x) => hover("system", "system", 0, x)}
        />
        <AppTitle
          name={appName}
          open={isOpen("app:0")}
          clickable={menuLabels.length > 0}
          onClick={(x) => click("app:0", "app", 0, x)}
          onHover={(x) => hover("app:0", "app", 0, x)}
        />
        <MenuLabels
          labels={menuLabels}
          isOpen={isOpen}
          onClick={(i, x) => click(`app:${i}`, "app", i, x)}
          onHover={(i, x) => hover(`app:${i}`, "app", i, x)}
        />
      </div>
      <Tray />
    </div>
  );
}
