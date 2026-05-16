# sola-monitor kit port — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `sola-monitor` from `sola-app` (GTK4/WebKit) to `sola-kit` (CEF/Remix v3), preserving behavior and adding sidebar-width persistence via a new sticky bus topic.

**Architecture:** Replace the GTK4/WebKit shell with kit's CEF+Wayland host. Web frontend rewritten on Remix v3 using `@sola/*` primitives. Sidebar width persisted via a new `Topic::MonitorConfig` (mirrors `TerminalConfig`). Resize handle is monitor-local (10 lines of TS) — Split isn't used here because monitor wants the right pane sized and the existing Split sizes its first pane only; adding a Split prop for one consumer isn't worth it.

**Tech Stack:** Rust (`sola-kit`, `sola-bus`, `sola-core`, `include_dir`, `serde_json`), Remix v3 (`@remix-run/ui`), `@sola/*` kit components.

**Spec:** `docs/specs/2026-05-16-sola-monitor-kit-port-design.md`

---

## File Structure

**Created:**
- `crates/sola-monitor/src/app.rs` — `MonitorApp` impl, bus handlers, JS commands, menu.
- `crates/sola-monitor/web/main.tsx` — root component, IPC plumbing, top-level layout.
- `crates/sola-monitor/web/panels/messages.tsx` — toolbar + filtered table + inline expansion.
- `crates/sola-monitor/web/panels/sticky.tsx` — right sidebar list with click-to-expand + resize handle.
- `crates/sola-monitor/web/lib/categories.ts` — `TOPIC_CATEGORIES` map + `categoryOf()`.
- `crates/sola-monitor/web/lib/json-tokens.tsx` — JSON syntax highlighter (port of `tokenizeJson`).
- `crates/sola-monitor/web/lib/style.css` — bespoke table grid + category stripe + JSON token colors.

**Modified:**
- `crates/sola-bus/src/topics.rs` — add `MonitorConfig` struct + `Topic::MonitorConfig` variant.
- `crates/sola-monitor/Cargo.toml` — swap `sola-app` + `gtk4` for `sola-kit` + `include_dir`.
- `crates/sola-monitor/src/main.rs` — subprocess gate + `sola_kit::run::<MonitorApp>()`.

**Deleted:**
- `crates/sola-monitor/web/index.html` — kit auto-provides.
- `crates/sola-monitor/web/src/main.ts` — replaced by `web/main.tsx`.
- `crates/sola-monitor/web/src/app.ts` — replaced by `web/main.tsx` + panels.
- `crates/sola-monitor/web/src/theme.css` — replaced by kit theme + `web/lib/style.css`.

---

## Task 1: Add `Topic::MonitorConfig` to sola-bus (TDD)

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`

- [ ] **Step 1: Write failing roundtrip tests**

Append inside `#[cfg(test)] mod tests { … }` in `crates/sola-bus/src/topics.rs`:

```rust
#[test]
fn monitor_config_roundtrips_via_postcard() {
    let cfg = MonitorConfig { sidebar_width: 312 };
    let topic = Topic::MonitorConfig(cfg.clone());
    let msg = topic.to_message();
    let parsed = Topic::parse(&msg).unwrap();
    match parsed {
        Topic::MonitorConfig(back) => {
            assert_eq!(back.sidebar_width, 312);
        }
        other => panic!("expected MonitorConfig, got {other:?}"),
    }
}

#[test]
fn monitor_config_roundtrips_via_toml() {
    let cfg = MonitorConfig { sidebar_width: 240 };
    let topic = Topic::MonitorConfig(cfg);
    let value = topic
        .to_toml_value()
        .expect("persistent payload should serialize to TOML");
    let restored = Topic::from_toml_section(TopicKind::MonitorConfig, value)
        .expect("section should deserialize");
    match restored {
        Topic::MonitorConfig(back) => {
            assert_eq!(back.sidebar_width, 240);
        }
        other => panic!("expected MonitorConfig, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test -p sola-bus --lib monitor_config 2>&1 | tail -20`

Expected: compile error referencing missing `MonitorConfig` struct and `Topic::MonitorConfig` variant.

- [ ] **Step 3: Add the `MonitorConfig` struct**

Insert above the `define_topics!` block in `crates/sola-bus/src/topics.rs`, near the other `*Config` structs (e.g. immediately after `BrowserConfig`):

```rust
/// Monitor UI preferences. Persistent so the sticky-panel width
/// survives across monitor restarts and bus restarts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MonitorConfig {
    pub sidebar_width: u32,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self { sidebar_width: 240 }
    }
}
```

- [ ] **Step 4: Add the `Topic` enum variant**

Inside `define_topics! { … }`, add (placed alongside the other `#[persistent]` config topics like `TerminalConfig`):

```rust
    // Monitor UI preferences (sticky-panel width). Persistent
    // so the user's chosen width survives across restarts.
    #[persistent]
    MonitorConfig(MonitorConfig),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p sola-bus --lib monitor_config 2>&1 | tail -20`

Expected: `test result: ok. 2 passed`.

- [ ] **Step 6: Run the full bus test suite to confirm no regressions**

Run: `cargo test -p sola-bus 2>&1 | tail -20`

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-bus/src/topics.rs
git commit -m "$(cat <<'EOF'
feat(sola-bus): add Topic::MonitorConfig

Sticky-persistent monitor UI preferences (sidebar width today;
headroom for future fields). Mirrors TerminalConfig.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Cargo.toml swap + Rust scaffold

**Files:**
- Modify: `crates/sola-monitor/Cargo.toml`
- Rewrite: `crates/sola-monitor/src/main.rs`
- Create: `crates/sola-monitor/src/app.rs`

- [ ] **Step 1: Update Cargo.toml**

Replace `crates/sola-monitor/Cargo.toml` with:

```toml
[package]
name = "sola-monitor"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "sola-monitor"
path = "src/main.rs"

[dependencies]
sola-kit = { path = "../sola-kit" }
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
include_dir = "0.7"
serde_json = "1"
hex = "0.4"
tracing = "0.1"
```

(`hex` is retained because `decode::decode_payload` uses `hex::encode` for unparseable payloads.)

- [ ] **Step 2: Rewrite `src/main.rs`**

Overwrite `crates/sola-monitor/src/main.rs` with:

```rust
mod app;
mod decode;

use std::process::ExitCode;

use sola_kit::SolaApp;

fn main() -> ExitCode {
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess(app::MonitorApp::APP_ID) {
        return code;
    }
    sola_kit::run::<app::MonitorApp>();
    ExitCode::SUCCESS
}
```

- [ ] **Step 3: Create `src/app.rs` skeleton**

Create `crates/sola-monitor/src/app.rs` with:

```rust
//! Monitor app — kit-side implementation.
//!
//! One window. Taps every bus message via `on_raw_bus_message` and
//! forwards a decoded JSON event to the frontend. Owns one sticky
//! topic (`MonitorConfig`) so the sidebar width survives restart.

use serde_json::{Value, json};
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, MonitorConfig,
    Topic, TopicKind,
};
use sola_bus::{Delivery, Message};
use sola_core::KeyCode;
use sola_kit::{
    AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};

use crate::decode;

static APP_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web");

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    @dir "/" => &APP_DIR,
};

pub struct MonitorApp {
    main_window: WindowHandle,
    config: MonitorConfig,
}

impl SolaApp for MonitorApp {
    const APP_ID: &'static str = "sola-monitor";

    fn new(ctx: &mut AppCtx) -> Self {
        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (900, 600),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            zoned: false,
            keyboard_target: true,
        });

        ctx.emit(Topic::SetAppMenu(AppMenuPayload {
            app_id: Self::APP_ID.into(),
            menus: vec![MenuDefinition {
                label: "Monitor".into(),
                items: vec![MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Monitor".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                }],
            }],
        }));

        tracing::info!("sola-monitor ready");

        Self {
            main_window,
            config: MonitorConfig::default(),
        }
    }

    fn on_raw_bus_message(&mut self, msg: &Message, _ctx: &mut AppCtx) {
        let event = decode::message_to_json(msg);
        self.main_window.send_to_js(&event);
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.subscribe_all();
        bus.on(TopicKind::MonitorConfig, Self::on_config);
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        _id: Option<u64>,
        _source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        if cmd == "monitor_set_sidebar_width" {
            if let Some(w) = args.get("width").and_then(|v| v.as_u64()) {
                self.config.sidebar_width = w as u32;
                ctx.emit(Topic::MonitorConfig(self.config.clone()));
            }
        }
    }
}

impl MonitorApp {
    fn on_config(&mut self, d: &Delivery, _ctx: &mut AppCtx) {
        if let Topic::MonitorConfig(cfg) = d.topic {
            self.config = cfg.clone();
            self.main_window.send_to_js(&json!({
                "event": "state",
                "sidebar_width": self.config.sidebar_width,
            }));
        }
    }

    fn on_menu_action(&mut self, d: &Delivery, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = d.topic
            && app_id == Self::APP_ID
            && action_id == "quit"
        {
            std::process::exit(0);
        }
    }
}
```

- [ ] **Step 4: Stub the web tree so `include_dir!` doesn't choke**

Create `crates/sola-monitor/web/main.tsx` with a placeholder so the build resolves the `include_dir!` invocation even though task 5+ fills in real content:

```tsx
export function Main() {
  return () => null;
}
```

- [ ] **Step 5: Delete legacy web files**

```bash
rm -f crates/sola-monitor/web/index.html
rm -rf crates/sola-monitor/web/src
```

- [ ] **Step 6: Build**

Run: `cargo make build 2>&1 | tail -15`

Expected: `Finished … target(s)` with no errors.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-monitor/
git commit -m "$(cat <<'EOF'
refactor(sola-monitor): scaffold the sola-kit port

Cargo.toml swap (sola-app+gtk4 → sola-kit+include_dir), subprocess
gate + run::<MonitorApp>() in main.rs, MonitorApp skeleton with bus
handlers + menu wiring + JS command handler. Legacy web files
deleted, placeholder Main component added so include_dir! resolves.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Categories + JSON token helpers

**Files:**
- Create: `crates/sola-monitor/web/lib/categories.ts`
- Create: `crates/sola-monitor/web/lib/json-tokens.tsx`

- [ ] **Step 1: Write `web/lib/categories.ts`**

```ts
// Topic → category map used to color-stripe rows in the messages
// table. Categories are visual-only; unknown topics get the
// "unknown" stripe.

export const TOPIC_CATEGORIES: Record<string, string> = {
  Apps: "lifecycle",
  LaunchApp: "lifecycle",
  LaunchResult: "lifecycle",
  UserAppExited: "lifecycle",
  Shutdown: "lifecycle",
  Composition: "composition",
  Frame: "composition",
  Focus: "composition",
  SetWindowPolicy: "window",
  OutputGeometry: "window",
  MouseEntered: "input",
  ShellKeyBindings: "input",
  SetAppMenu: "menu",
  MenuAction: "menu",
  OpenUrl: "browser",
};

export function categoryOf(topic: string): string {
  return TOPIC_CATEGORIES[topic] || "unknown";
}
```

- [ ] **Step 2: Write `web/lib/json-tokens.tsx`**

```tsx
// JSON syntax highlighter. Produces an array of <span> nodes with
// `.token-<kind>` classes that lib/style.css colors. Port of the
// legacy `tokenizeJson` helper, with the array return shape adapted
// to Remix v3 (returns an array of RemixNode, not arrow-js html`…`).

import { type RemixNode } from "@remix-run/ui";

export function highlightedPreview(payload: unknown): RemixNode {
  if (payload == null) return "";
  return tokenizeJson(JSON.stringify(payload), 200);
}

export function highlightedJson(payload: unknown): RemixNode {
  if (payload == null) return "";
  return tokenizeJson(JSON.stringify(payload, null, 2));
}

export function tokenizeJson(json: string, maxChars?: number): RemixNode {
  const tokens: RemixNode[] = [];
  let i = 0;
  let emittedChars = 0;
  const out = (text: string, kind: string | null) => {
    if (maxChars !== undefined) {
      const remaining = maxChars - emittedChars;
      if (remaining <= 0) return false;
      if (text.length > remaining) text = text.slice(0, remaining) + "…";
    }
    emittedChars += text.length;
    if (kind) tokens.push(<span class={`token-${kind}`}>{text}</span>);
    else tokens.push(text);
    return maxChars === undefined || emittedChars < maxChars;
  };

  while (i < json.length) {
    const ch = json[i];
    if (ch === '"') {
      // String literal — peek ahead to decide if it's a key or a value.
      let j = i + 1;
      while (j < json.length) {
        if (json[j] === "\\") { j += 2; continue; }
        if (json[j] === '"') break;
        j++;
      }
      const lit = json.slice(i, j + 1);
      // A key is a string followed (after optional whitespace) by ":".
      let k = j + 1;
      while (k < json.length && /\s/.test(json[k])) k++;
      const isKey = json[k] === ":";
      if (!out(lit, isKey ? "key" : "string")) return tokens;
      i = j + 1;
    } else if (/[\d\-]/.test(ch)) {
      let j = i;
      while (j < json.length && /[\d.eE+\-]/.test(json[j])) j++;
      if (!out(json.slice(i, j), "number")) return tokens;
      i = j;
    } else if (json.startsWith("true", i) || json.startsWith("false", i)) {
      const word = json.startsWith("true", i) ? "true" : "false";
      if (!out(word, "boolean")) return tokens;
      i += word.length;
    } else if (json.startsWith("null", i)) {
      if (!out("null", "null")) return tokens;
      i += 4;
    } else {
      if (!out(ch, null)) return tokens;
      i++;
    }
  }
  return tokens;
}
```

- [ ] **Step 3: Build to confirm assets register**

Run: `cargo make build 2>&1 | tail -10`

Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-monitor/web/lib/
git commit -m "$(cat <<'EOF'
feat(sola-monitor): categories + JSON token helpers

Pure-function helpers used by the table and the sticky panel.
JSON highlighter is a Remix-v3 port of the legacy tokenizeJson —
same one-pass scanner, same token kinds, RemixNode arrays in
place of arrow-js html templates.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Bespoke styles

**Files:**
- Create: `crates/sola-monitor/web/lib/style.css`

- [ ] **Step 1: Write `web/lib/style.css`**

```css
/* Monitor-local bespoke styles. References theme atoms directly
   because this file ships per-app, not as a kit component. */

/* --- Toolbar --- */

.monitor-toolbar {
  border-bottom: 1px solid var(--border-subtle);
  padding: var(--space-sm) var(--space-md);
  background: var(--bg-secondary);
}

/* --- Main grid: messages | divider | sticky --- */

.monitor-main {
  display: grid;
  grid-template-columns: 1fr 4px var(--monitor-sidebar-width, 240px);
  flex: 1 1 0;
  min-height: 0;
}

.monitor-divider {
  background: var(--border-subtle);
  cursor: col-resize;
  transition: background 80ms ease;
}

.monitor-divider:hover,
.monitor-divider.is-dragging {
  background: var(--border);
}

/* --- Messages panel --- */

.monitor-messages {
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: var(--bg-primary);
}

.monitor-table-header,
.monitor-message-row {
  display: grid;
  grid-template-columns: 88px 200px 120px 24px 1fr;
  gap: var(--space-sm);
  padding: 4px var(--space-md);
  font-size: var(--text-caption);
  align-items: baseline;
}

.monitor-table-header {
  position: sticky;
  top: 0;
  background: var(--bg-secondary);
  color: var(--text-tertiary);
  border-bottom: 1px solid var(--border-subtle);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  z-index: 1;
}

.monitor-message-list {
  overflow-y: auto;
  flex: 1 1 0;
  font-family: var(--font-mono);
}

.monitor-message-row {
  border-left: 2px solid transparent;
  cursor: pointer;
}

.monitor-message-row:hover {
  background: var(--bg-hover);
}

.monitor-message-row.selected {
  background: var(--bg-tertiary);
}

/* Category stripes */
.monitor-message-row[data-category="lifecycle"]    { border-left-color: var(--success); }
.monitor-message-row[data-category="composition"]  { border-left-color: var(--text-accent); }
.monitor-message-row[data-category="window"]       { border-left-color: var(--accent); }
.monitor-message-row[data-category="input"]        { border-left-color: var(--text-secondary); }
.monitor-message-row[data-category="menu"]         { border-left-color: var(--danger); }
.monitor-message-row[data-category="browser"]      { border-left-color: var(--accent-dim); }
.monitor-message-row[data-category="unknown"]      { border-left-color: var(--border); }

.monitor-cell {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.monitor-cell.preview {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--text-secondary);
}

.monitor-cell.preview.expanded {
  white-space: pre-wrap;
  overflow: visible;
}

.monitor-cell.sticky-dot .dot {
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
}

/* --- Sticky panel --- */

.monitor-sticky {
  display: flex;
  flex-direction: column;
  background: var(--bg-secondary);
  border-left: 1px solid var(--border-subtle);
  min-height: 0;
}

.monitor-sticky-header {
  padding: var(--space-sm) var(--space-md);
  color: var(--text-tertiary);
  font-size: var(--text-caption);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  border-bottom: 1px solid var(--border-subtle);
}

.monitor-sticky-list {
  overflow-y: auto;
  flex: 1 1 0;
}

.monitor-sticky-entry {
  border-left: 2px solid transparent;
}

.monitor-sticky-entry[data-category="lifecycle"]   { border-left-color: var(--success); }
.monitor-sticky-entry[data-category="composition"] { border-left-color: var(--text-accent); }
.monitor-sticky-entry[data-category="window"]      { border-left-color: var(--accent); }
.monitor-sticky-entry[data-category="input"]       { border-left-color: var(--text-secondary); }
.monitor-sticky-entry[data-category="menu"]        { border-left-color: var(--danger); }
.monitor-sticky-entry[data-category="browser"]     { border-left-color: var(--accent-dim); }
.monitor-sticky-entry[data-category="unknown"]     { border-left-color: var(--border); }

.monitor-sticky-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--space-xs) var(--space-md);
  cursor: pointer;
  font-size: var(--text-caption);
}

.monitor-sticky-item:hover {
  background: var(--bg-hover);
}

.monitor-sticky-item-topic {
  color: var(--text-primary);
  font-family: var(--font-mono);
}

.monitor-sticky-item-source {
  color: var(--text-tertiary);
}

.monitor-sticky-detail {
  padding: var(--space-xs) var(--space-md) var(--space-sm);
  font-family: var(--font-mono);
  font-size: var(--text-caption);
  white-space: pre-wrap;
  word-break: break-all;
  border-top: 1px dashed var(--border-subtle);
}

/* --- JSON token colors --- */

.token-string  { color: var(--success); }
.token-number  { color: var(--text-accent); }
.token-key     { color: var(--accent); }
.token-boolean { color: var(--danger); }
.token-null    { color: var(--text-muted); }

/* --- Auto-scroll pill --- */

.monitor-autoscroll-pill {
  position: absolute;
  bottom: var(--space-md);
  right: var(--space-md);
  z-index: 10;
}
```

- [ ] **Step 2: Build**

Run: `cargo make build 2>&1 | tail -10`

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-monitor/web/lib/style.css
git commit -m "$(cat <<'EOF'
feat(sola-monitor): bespoke styles

Table grid, sticky-panel chrome, category stripes, JSON token
colors, autoscroll pill. References theme atoms directly because
these styles ship per-app, not as kit components.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: MessagesPanel

**Files:**
- Create: `crates/sola-monitor/web/panels/messages.tsx`

- [ ] **Step 1: Write `web/panels/messages.tsx`**

```tsx
// Messages panel — toolbar (filter, topic dropdown, pause, clear,
// counter) + scrollable table of bus messages. Selection toggles
// the selected row's preview cell into a one-line highlighted JSON
// dump (per-design inline expansion).

import { type Handle } from "@remix-run/ui";
import { Button } from "@sola/button";
import { PopoverSelect } from "@sola/popover-select";
import { Text } from "@sola/text";
import { TextInput } from "@sola/text-input";
import { on } from "@sola/kit";

import { categoryOf } from "../lib/categories";
import { highlightedJson, highlightedPreview } from "../lib/json-tokens";

export interface BusMessage {
  msgId: string;
  timestamp: number;
  topic: string;
  sticky: boolean;
  source: string;
  payload: unknown;
  rawHex: string | null;
}

export interface MessagesProps {
  // Caller-owned shared state. The panel mutates the visible-filter +
  // selection slices on this object and calls handle.update().
  state: MessagesState;
}

export interface MessagesState {
  messages: BusMessage[];
  filteredMessages: BusMessage[];
  selectedId: string | null;
  paused: boolean;
  pauseBufferLen: number;
  filter: string;
  topicFilter: string;
  count: number;
  autoScroll: boolean;
  knownTopics: string[];
}

function formatTime(ms: number): string {
  const d = new Date(ms);
  const h = String(d.getHours()).padStart(2, "0");
  const m = String(d.getMinutes()).padStart(2, "0");
  const s = String(d.getSeconds()).padStart(2, "0");
  const ms_ = String(d.getMilliseconds()).padStart(3, "0");
  return `${h}:${m}:${s}.${ms_}`;
}

export function MessagesPanel(handle: Handle<MessagesProps>) {
  let listEl: HTMLElement | null = null;

  const onScroll = () => {
    if (!listEl) return;
    const atBottom =
      listEl.scrollTop + listEl.clientHeight >= listEl.scrollHeight - 4;
    if (handle.props.state.autoScroll !== atBottom) {
      handle.props.state.autoScroll = atBottom;
      handle.update();
    }
  };

  // Mount the list element after first render so we can wire scroll.
  const captureList = (el: HTMLElement | null) => {
    listEl = el;
  };

  return () => {
    const s = handle.props.state;

    return (
      <div class="monitor-messages">
        <div class="monitor-table-header">
          <span>Time</span>
          <span>Topic</span>
          <span>Source</span>
          <span>S</span>
          <span>Preview</span>
        </div>

        <div class="monitor-message-list" mix={[on("scroll", onScroll)]} ref={captureList}>
          {s.filteredMessages.map((msg) => {
            const selected = s.selectedId === msg.msgId;
            const previewCls = selected
              ? "monitor-cell preview expanded"
              : "monitor-cell preview";
            return (
              <div
                key={msg.msgId}
                class={`monitor-message-row${selected ? " selected" : ""}`}
                data-category={categoryOf(msg.topic)}
                mix={[on("click", () => {
                  s.selectedId = selected ? null : msg.msgId;
                  handle.update();
                })]}
              >
                <span class="monitor-cell time">{formatTime(msg.timestamp)}</span>
                <span class="monitor-cell topic">{msg.topic}</span>
                <span class="monitor-cell source">{msg.source || "—"}</span>
                <span class="monitor-cell sticky-dot">
                  {msg.sticky ? <span class="dot"/> : ""}
                </span>
                <span class={previewCls}>
                  {selected && msg.payload != null
                    ? highlightedJson(msg.payload)
                    : highlightedPreview(msg)}
                </span>
              </div>
            );
          })}
        </div>
      </div>
    );
  };
}

// Helper exposed for main.tsx — renders the toolbar above the panel
// so main.tsx can place it in its top-level Stack.
export interface ToolbarProps {
  state: MessagesState;
  onFilter: (v: string) => void;
  onTopic: (v: string) => void;
  onTogglePause: () => void;
  onClear: () => void;
}

export function MessagesToolbar(handle: Handle<ToolbarProps>) {
  return () => {
    const { state, onFilter, onTopic, onTogglePause, onClear } = handle.props;
    const topicOptions = [{ label: "All topics", value: "" }].concat(
      state.knownTopics.map((t) => ({ label: t, value: t })),
    );
    return (
      <div class="monitor-toolbar">
        <div style="display: flex; gap: var(--space-md); align-items: center">
          <div style="flex: 1; max-width: 320px">
            <TextInput
              value={state.filter}
              placeholder="Filter messages…"
              onInput={onFilter}
            />
          </div>
          <PopoverSelect
            value={state.topicFilter}
            options={topicOptions}
            onChange={onTopic}
          />
          <Button
            variant={state.paused ? "primary" : "ghost"}
            onPress={onTogglePause}
          >
            {state.paused ? `Resume (${state.pauseBufferLen})` : "Pause"}
          </Button>
          <Button variant="ghost" onPress={onClear}>Clear</Button>
          <div style="flex: 1"/>
          <Text tone="muted" kind="caption">{state.count} msgs</Text>
        </div>
      </div>
    );
  };
}
```

- [ ] **Step 2: Build**

Run: `cargo make build 2>&1 | tail -10`

Expected: clean build (the placeholder `main.tsx` doesn't import this yet — task 7 wires it).

- [ ] **Step 3: Commit**

```bash
git add crates/sola-monitor/web/panels/messages.tsx
git commit -m "$(cat <<'EOF'
feat(sola-monitor): MessagesPanel + MessagesToolbar

Toolbar (filter, topic dropdown, pause, clear, counter) and
scrollable table with click-to-expand inline highlighting.
State is caller-owned; the panel mutates slices and calls
handle.update().

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: StickyPanel + resize handle

**Files:**
- Create: `crates/sola-monitor/web/panels/sticky.tsx`

- [ ] **Step 1: Write `web/panels/sticky.tsx`**

```tsx
// Sticky-state panel. Shows the latest message per (topic, source)
// pair, click-to-expand inline JSON. Owns the drag divider too —
// drag commits to `monitor_set_sidebar_width` on pointer-up so the
// width persists via Topic::MonitorConfig.

import { type Handle } from "@remix-run/ui";
import { invoke } from "@sola/ipc";
import { on } from "@sola/kit";

import { categoryOf } from "../lib/categories";
import { highlightedJson } from "../lib/json-tokens";
import type { BusMessage } from "./messages";

export interface StickyProps {
  state: StickyState;
}

export interface StickyState {
  stickyMessages: BusMessage[];
  expandedStickyKey: string | null;
  sidebarWidth: number;
}

const MIN_WIDTH = 120;
const MAX_WIDTH = 600;

export function StickyDivider(handle: Handle<StickyProps>) {
  let dragging = false;

  const onDown = (e: PointerEvent) => {
    dragging = true;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    (e.target as HTMLElement).classList.add("is-dragging");
    e.preventDefault();
  };

  const onMove = (e: PointerEvent) => {
    if (!dragging) return;
    const next = Math.max(
      MIN_WIDTH,
      Math.min(window.innerWidth - e.clientX, MAX_WIDTH),
    );
    if (next !== handle.props.state.sidebarWidth) {
      handle.props.state.sidebarWidth = next;
      handle.update();
    }
  };

  const onUp = (e: PointerEvent) => {
    if (!dragging) return;
    dragging = false;
    (e.target as HTMLElement).classList.remove("is-dragging");
    invoke("monitor_set_sidebar_width", { width: handle.props.state.sidebarWidth });
  };

  return () => (
    <div
      class="monitor-divider"
      mix={[
        on("pointerdown", onDown),
        on("pointermove", onMove),
        on("pointerup", onUp),
        on("pointercancel", onUp),
      ]}
    />
  );
}

export function StickyPanel(handle: Handle<StickyProps>) {
  return () => {
    const s = handle.props.state;
    return (
      <div class="monitor-sticky">
        <div class="monitor-sticky-header">Sticky State</div>
        <div class="monitor-sticky-list">
          {s.stickyMessages.map((msg) => {
            const key = `${msg.topic}:${msg.source}`;
            const expanded = s.expandedStickyKey === key;
            return (
              <div
                key={key}
                class="monitor-sticky-entry"
                data-category={categoryOf(msg.topic)}
              >
                <div
                  class={`monitor-sticky-item${expanded ? " expanded" : ""}`}
                  mix={[on("click", () => {
                    s.expandedStickyKey = expanded ? null : key;
                    handle.update();
                  })]}
                >
                  <span class="monitor-sticky-item-topic">{msg.topic}</span>
                  <span class="monitor-sticky-item-source">{msg.source || ""}</span>
                </div>
                {expanded && msg.payload != null
                  ? <div class="monitor-sticky-detail">{highlightedJson(msg.payload)}</div>
                  : ""}
              </div>
            );
          })}
        </div>
      </div>
    );
  };
}
```

- [ ] **Step 2: Build**

Run: `cargo make build 2>&1 | tail -10`

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-monitor/web/panels/sticky.tsx
git commit -m "$(cat <<'EOF'
feat(sola-monitor): StickyPanel + StickyDivider

Right-sidebar sticky-state list with click-to-expand inline JSON.
Divider commits width on pointer-up via monitor_set_sidebar_width.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Main.tsx — wire it all together

**Files:**
- Overwrite: `crates/sola-monitor/web/main.tsx`

- [ ] **Step 1: Write `web/main.tsx`**

```tsx
// Monitor root. Owns the shared state, the bus_message ingest,
// the filtered/sticky derivations, and the toolbar action wiring.
// Composes the toolbar + messages panel + divider + sticky panel.

import { type Handle } from "@remix-run/ui";
import { Root } from "@sola/root";
import { on as ipcOn } from "@sola/ipc";

import {
  MessagesPanel,
  MessagesToolbar,
  type BusMessage,
  type MessagesState,
} from "./panels/messages";
import { StickyDivider, StickyPanel, type StickyState } from "./panels/sticky";

const MAX_MESSAGES = 5000;

type MonitorState = MessagesState & StickyState;

interface MainProps {}

export function Main(handle: Handle<MainProps>) {
  const state: MonitorState = {
    // MessagesState
    messages: [],
    filteredMessages: [],
    selectedId: null,
    paused: false,
    pauseBufferLen: 0,
    filter: "",
    topicFilter: "",
    count: 0,
    autoScroll: true,
    knownTopics: [],
    // StickyState
    stickyMessages: [],
    expandedStickyKey: null,
    sidebarWidth: 240,
  };

  let pauseBuffer: BusMessage[] = [];
  const seenTopics = new Set<string>();
  const stickyMap = new Map<string, BusMessage>();

  const applyFilter = () => {
    const filterLower = state.filter.toLowerCase();
    const topicFilter = state.topicFilter;
    state.filteredMessages = state.messages.filter((msg) => {
      if (topicFilter && msg.topic !== topicFilter) return false;
      if (filterLower) {
        const topicMatch = msg.topic.toLowerCase().includes(filterLower);
        const sourceMatch = msg.source.toLowerCase().includes(filterLower);
        const payloadMatch = msg.payload
          ? JSON.stringify(msg.payload).toLowerCase().includes(filterLower)
          : false;
        if (!topicMatch && !sourceMatch && !payloadMatch) return false;
      }
      return true;
    });
  };

  const refreshKnownTopics = () => {
    state.knownTopics = Array.from(seenTopics).sort();
  };

  const addMessage = (msg: BusMessage) => {
    if (state.paused) {
      pauseBuffer.push(msg);
      state.pauseBufferLen = pauseBuffer.length;
      handle.update();
      return;
    }
    state.messages.push(msg);
    if (state.messages.length > MAX_MESSAGES) {
      state.messages.splice(0, state.messages.length - MAX_MESSAGES);
    }
    state.count = state.messages.length;

    if (msg.sticky) {
      const key = `${msg.topic}:${msg.source}`;
      stickyMap.set(key, msg);
      state.stickyMessages = Array.from(stickyMap.values());
    }

    if (!seenTopics.has(msg.topic)) {
      seenTopics.add(msg.topic);
      refreshKnownTopics();
    }

    applyFilter();
    handle.update();
    if (state.autoScroll) {
      requestAnimationFrame(() => {
        const list = document.querySelector(".monitor-message-list");
        if (list) list.scrollTop = list.scrollHeight;
      });
    }
  };

  // --- IPC wiring ---

  ipcOn("bus_message", (msg: BusMessage) => addMessage(msg));

  ipcOn("state", (msg: { sidebar_width: number }) => {
    state.sidebarWidth = msg.sidebar_width;
    handle.update();
  });

  // --- Toolbar handlers ---

  const onFilter = (v: string) => {
    state.filter = v;
    applyFilter();
    handle.update();
  };

  const onTopic = (v: string) => {
    state.topicFilter = v;
    applyFilter();
    handle.update();
  };

  const togglePause = () => {
    state.paused = !state.paused;
    if (!state.paused) {
      for (const msg of pauseBuffer) addMessage(msg);
      pauseBuffer = [];
      state.pauseBufferLen = 0;
    }
    handle.update();
  };

  const clearMessages = () => {
    state.messages = [];
    state.filteredMessages = [];
    state.selectedId = null;
    state.count = 0;
    pauseBuffer = [];
    state.pauseBufferLen = 0;
    handle.update();
  };

  const jumpToBottom = () => {
    state.autoScroll = true;
    handle.update();
    requestAnimationFrame(() => {
      const list = document.querySelector(".monitor-message-list");
      if (list) list.scrollTop = list.scrollHeight;
    });
  };

  return () => (
    <Root>
      <div style="display: flex; flex-direction: column; height: 100vh; position: relative">
        <MessagesToolbar
          state={state}
          onFilter={onFilter}
          onTopic={onTopic}
          onTogglePause={togglePause}
          onClear={clearMessages}
        />
        <div
          class="monitor-main"
          style={`--monitor-sidebar-width: ${state.sidebarWidth}px`}
        >
          <MessagesPanel state={state}/>
          <StickyDivider state={state}/>
          <StickyPanel state={state}/>
        </div>
        {!state.autoScroll
          ? (
            <div class="monitor-autoscroll-pill">
              <button
                type="button"
                class="sola-button sola-button-ghost"
                onClick={jumpToBottom}
              >
                ↓ Auto-scroll paused — click to resume
              </button>
            </div>
          )
          : ""}
      </div>
    </Root>
  );
}
```

- [ ] **Step 2: Build**

Run: `cargo make build 2>&1 | tail -10`

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-monitor/web/main.tsx
git commit -m "$(cat <<'EOF'
feat(sola-monitor): wire panels into main.tsx + state subscription

Owns shared MonitorState (messages + sticky + filter + width).
Subscribes to bus_message + state events from Rust, composes
MessagesToolbar → MessagesPanel + StickyDivider + StickyPanel.
Auto-scroll pill appears at bottom when user scrolls away.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Final verification

**Files:** none modified.

- [ ] **Step 1: Run full workspace build**

Run: `cargo make build 2>&1 | tail -10`

Expected: `Finished … target(s)` with zero warnings.

- [ ] **Step 2: Run full bus + kit tests**

Run: `cargo test -p sola-bus -p sola-kit --lib 2>&1 | tail -20`

Expected: all tests pass, including the two new `monitor_config_roundtrips_*` tests added in Task 1.

- [ ] **Step 3: Confirm binary exists**

Run: `ls -lh target/debug/sola-monitor`

Expected: file present.

- [ ] **Step 4: Surface follow-up to user**

Notify the user that the port is complete and **the 5000-row render performance is a tracked follow-up** — the kit's Remix v3 diff is element-tree-based rather than fine-grained reactive, so emit-heavy topics (e.g. `Frame`) may cause stutter that the legacy `@arrow-js/core` implementation didn't have. Recommend smoke-testing by toggling a high-volume topic and watching for dropped frames. If perf is bad, the fast-follow fix is cap visible rows (e.g. last 500) with a "show more" pagination — not designed in.

To smoke-test, the user runs `cargo make install sola-monitor` and launches `/opt/sola/bin/sola-monitor` from a TTY in the running sola environment. (`install` is left to the user — the assistant does not run it.)

---

## Self-review notes

- **Spec coverage:**
  - `Topic::MonitorConfig` → Task 1.
  - Cargo.toml + subprocess gate + scaffold → Task 2.
  - `decode.rs` unchanged — Task 2 retains it; explicitly no task to modify it.
  - Categories + JSON token helpers → Task 3.
  - Bespoke CSS (table grid, category stripe, JSON tokens, autoscroll pill, divider) → Task 4.
  - MessagesPanel + MessagesToolbar → Task 5.
  - StickyPanel + StickyDivider (which also owns the resize-commit IPC) → Task 6.
  - Main.tsx with state model + IPC subscription + composition → Task 7.
  - `monitor_set_sidebar_width` JS command → Task 2 (Rust side) + Task 6 (JS side).
  - 5000-row perf follow-up → Task 8 Step 4.

- **Implementation deviations from spec:**
  - Spec mentioned `Split` for the divider; plan uses a monitor-local 30-line `StickyDivider` instead because the existing kit `Split` sizes its first pane only, and adding a `startsAt: "second"` prop + an `onResizeCommit` for one consumer isn't worth it. Behavior is identical from the user's perspective.

- **Type consistency:**
  - JS command name `monitor_set_sidebar_width` matches in Rust (Task 2 Step 3) and JS (Task 6 Step 1).
  - `MonitorConfig.sidebar_width: u32` matches the JSON shape `{ width: u32 }` in the JS command args.
  - `MonitorState` extends `MessagesState & StickyState` — both are exported from their panel modules; intersection compiles.
  - `BusMessage` interface declared in `panels/messages.tsx`, imported by `panels/sticky.tsx` and `main.tsx`.
