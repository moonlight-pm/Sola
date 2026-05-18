// <Switcher> — centered horizontal strip of app tiles for alt-tab.
//
// IPC contract (Rust → JS via __solaRecv / @sola/ipc on()):
//   { event: "render", apps: AppEntry[], selected: number } — show/refresh
//   { event: "clear" }                                      — hide
//
// IPC contract (JS → Rust via invoke()):
//   invoke("select", { index: number }) — pointer hover changed selection

import { type Handle } from "@remix-run/ui";
import { on as ipcOn, invoke } from "@sola/ipc";
import { SwitcherCard, type AppEntry } from "./switcher-card";

export interface SwitcherInitial {
  visible: boolean;
  apps: AppEntry[];
  selected: number;
}

interface RenderMsg {
  apps: AppEntry[];
  selected: number;
}

export function Switcher(handle: Handle<{ initial: SwitcherInitial }>) {
  // ── Closure-captured state ────────────────────────────────────────────
  let visible = handle.props.initial.visible;
  let apps: AppEntry[] = handle.props.initial.apps;
  let selected = handle.props.initial.selected;

  // ── Bus envelope subscriptions ────────────────────────────────────────
  ipcOn("render", (msg: RenderMsg) => {
    apps = msg.apps ?? [];
    selected = msg.selected ?? 0;
    visible = true;
    handle.update();
  });

  ipcOn("clear", () => {
    visible = false;
    apps = [];
    selected = 0;
    handle.update();
  });

  // ── Render ────────────────────────────────────────────────────────────
  return () => {
    if (!visible) {
      return <div class="sola-switcher" style="display: none;" />;
    }
    return (
      <div class="sola-switcher">
        {apps.map((app, i) => (
          <SwitcherCard
            key={app.app_id}
            app={app}
            selected={i === selected}
            onHover={() => invoke("select", { index: i })}
          />
        ))}
      </div>
    );
  };
}
