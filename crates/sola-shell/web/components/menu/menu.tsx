// <Menu> — dropdown menu surface.
//
// IPC contract (Rust → JS via __solaRecv / @sola/ipc on()):
//   { event: "show", items: MenuItemData[], anchor_x: number } — show dropdown
//   { event: "clear" }                                         — hide dropdown
//
// IPC contract (JS → Rust via invoke()):
//   invoke("dismiss", {})                      — click outside dropdown
//   invoke("action", { app_id, action_id })    — user clicked an enabled item

import { type Handle } from "@remix-run/ui";
import { on as ipcOn, invoke } from "@sola/ipc";
import { MenuItem, type MenuItemData } from "./menu-item";

export interface MenuInitial {
  visible: boolean;
}

interface ShowMsg {
  items: MenuItemData[];
  anchor_x: number;
}

// Stable class name used as DOM anchor for outside-click detection.
const MENU_CLS = "sola-menu";

export function Menu(handle: Handle<{ initial: MenuInitial }>) {
  // ── Closure-captured state ────────────────────────────────────────────
  let visible = handle.props.initial.visible;
  let items: MenuItemData[] = [];
  let anchorX = 0;

  // ── Bus envelope subscriptions ────────────────────────────────────────
  ipcOn("show", (msg: ShowMsg) => {
    items = msg.items ?? [];
    anchorX = msg.anchor_x ?? 0;
    visible = true;
    handle.update();
  });

  ipcOn("clear", () => {
    visible = false;
    items = [];
    handle.update();
  });

  // ── Outside-click dismiss ─────────────────────────────────────────────
  // The menu window persists for the full session so no cleanup is needed.
  // We use "click" (not "mousedown") to match legacy menu.ts behaviour.
  document.addEventListener("click", (e: MouseEvent) => {
    const menu = document.querySelector<HTMLElement>("." + MENU_CLS);
    if (menu && !menu.contains(e.target as Node)) {
      invoke("dismiss", {});
    }
  });

  // ── Render ────────────────────────────────────────────────────────────
  return () => {
    if (!visible) {
      return <div class={MENU_CLS} style="display: none;" />;
    }
    return (
      <div class={MENU_CLS} style={`left: ${anchorX}px; top: 0px;`}>
        {items.map((item, i) => (
          <MenuItem key={i} item={item} />
        ))}
      </div>
    );
  };
}
