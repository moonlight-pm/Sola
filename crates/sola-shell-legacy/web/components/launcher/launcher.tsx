// <Launcher> — floating application launcher panel.
//
// IPC contract (Rust → JS via __solaRecv / @sola/ipc on()):
//   { event: "reset" }                          — clear query, refocus input
//   { event: "render", apps: AppEntry[], selected: number }
//
// IPC contract (JS → Rust via invoke()):
//   invoke("query",  { text: string })           — user typed in the search input
//   invoke("nav",    { dir: "up" | "down" })     — keyboard arrow navigation
//   invoke("nav",    { index: number })           — pointer hover navigation
//   invoke("launch", { app_id?: string })         — launch (explicit id or current selection)
//   invoke("close",  {})                          — dismiss launcher

import { type Handle } from "@remix-run/ui";
import { on as ipcOn, invoke } from "@sola/ipc";
import { on } from "@sola/kit";
import { AppRow, type AppEntry } from "./app-row";

export interface LauncherInitial {
  apps: AppEntry[];
  selected: number;
  query: string;
}

// Stable class names used as DOM anchors (no ref API in Remix v3).
const QUERY_CLS = "sola-launcher-query";
const RESULTS_CLS = "sola-launcher-results";
const PANEL_CLS = "sola-launcher-panel";

function focusQuery(): void {
  const el = document.querySelector<HTMLInputElement>(`.${QUERY_CLS}`);
  el?.focus();
}

function scrollSelectedIntoView(index: number): void {
  const list = document.querySelector<HTMLElement>(`.${RESULTS_CLS}`);
  if (!list) return;
  const row = list.children[index] as HTMLElement | undefined;
  row?.scrollIntoView({ block: "nearest" });
}

export function Launcher(handle: Handle<{ initial: LauncherInitial }>) {
  // ── Closure-captured state ─────────────────────────────────────────
  let apps: AppEntry[] = handle.props.initial.apps;
  let selected: number = handle.props.initial.selected;

  // ── Bus envelope subscriptions ──────────────────────────────────────
  ipcOn("reset", () => {
    // Clear the native input value and refocus.
    const el = document.querySelector<HTMLInputElement>(`.${QUERY_CLS}`);
    if (el) {
      el.value = "";
    }
    // No state change that requires handle.update() — the input is
    // uncontrolled from Remix's perspective; Rust owns what's in it.
    // After reset the list is cleared by the subsequent render event.
    focusQuery();
  });

  ipcOn("render", (msg: any) => {
    apps = msg.apps ?? [];
    selected = msg.selected ?? 0;
    handle.update();
    // Scroll after Remix commits the new children.
    queueMicrotask(() => scrollSelectedIntoView(selected));
  });

  // ── Backdrop dismiss ────────────────────────────────────────────────
  // A mousedown on the transparent area outside the panel → close.
  // The listener is installed once for the component's lifetime; the
  // launcher window exists for the full session so no cleanup needed.
  document.addEventListener("mousedown", (e: MouseEvent) => {
    const panel = document.querySelector<HTMLElement>(`.${PANEL_CLS}`);
    if (panel && !panel.contains(e.target as Node)) {
      invoke("close", {});
    }
  });

  // ── Input handlers ───────────────────────────────────────────────────
  const onInput = (e: Event) => {
    const el = e.target as HTMLInputElement;
    invoke("query", { text: el.value });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        invoke("nav", { dir: "down" });
        break;
      case "ArrowUp":
        e.preventDefault();
        invoke("nav", { dir: "up" });
        break;
      case "Enter":
        e.preventDefault();
        invoke("launch", {});
        break;
      case "Escape":
        e.preventDefault();
        invoke("close", {});
        break;
    }
  };

  return () => (
    <div class="sola-launcher-backdrop">
      <div class={PANEL_CLS}>
        <input
          class={QUERY_CLS}
          type="text"
          autocomplete="off"
          spellcheck="false"
          mix={[on("input", onInput), on("keydown", onKeyDown)]}
        />
        <div class={RESULTS_CLS}>
          {apps.length === 0
            ? (
              <div class="sola-launcher-empty">No matching applications.</div>
            )
            : apps.map((app, i) => (
              <AppRow
                key={app.app_id}
                app={app}
                selected={i === selected}
                onHover={() => invoke("nav", { index: i })}
                onClick={() => invoke("launch", { app_id: app.app_id })}
              />
            ))}
        </div>
      </div>
    </div>
  );
}
