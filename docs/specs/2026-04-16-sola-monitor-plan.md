# sola-monitor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a bus message inspector app that displays all sola-bus traffic in real time with filtering, topic color coding, and payload inspection.

**Architecture:** Standard sola-app (Rust host + Arrow.js WebView frontend). The Rust host receives raw bus messages, decodes known topic payloads to JSON, and forwards them to the WebView. The frontend renders a scrolling, filterable message list with a detail pane.

**Tech Stack:** Rust (sola-app, sola-bus, serde_json, hex), TypeScript/Arrow.js frontend, CSS

**Design spec:** `docs/specs/2026-04-16-sola-monitor-design.md`

---

### Task 1: Add `on_raw_bus_message` hook to SolaApp trait

**Files:**
- Modify: `crates/sola-app/src/lib.rs`

This gives the monitor (and any future app) access to raw `Message` metadata (uuid, timestamp, sticky flags) that `Topic` doesn't expose.

- [ ] **Step 1: Add the default method to `SolaApp` trait**

In `crates/sola-app/src/lib.rs`, add this method to the `SolaApp` trait (after `on_bus_event`):

```rust
/// Called for every raw bus message before topic parsing.
/// Override to access message metadata (id, timestamp, sticky flags).
/// Default: no-op.
fn on_raw_bus_message(&mut self, _msg: &sola_bus::Message, _ctx: &mut AppCtx) {}
```

- [ ] **Step 2: Wire the hook into the bus event loop**

In the same file, inside the `glib::unix_fd_add_local` closure, change the message loop from:

```rust
for msg in messages {
    let Some(topic) = Topic::parse(&msg) else {
        continue;
    };
```

to:

```rust
for msg in messages {
    {
        let mut rt = runtime.borrow_mut();
        let AppRuntime { app, ctx } = &mut *rt;
        app.on_raw_bus_message(&msg, ctx);
    }
    let Some(topic) = Topic::parse(&msg) else {
        continue;
    };
```

- [ ] **Step 3: Verify existing apps still compile**

Run: `cargo check -p sola-terminal -p sola-shell -p sola-browser`
Expected: compiles clean (default no-op doesn't break anything)

- [ ] **Step 4: Commit**

```bash
git add crates/sola-app/src/lib.rs
git commit -m "feat(sola-app): add on_raw_bus_message hook to SolaApp trait"
```

---

### Task 2: Create monitor app scaffold (Rust host)

**Files:**
- Create: `apps/monitor/Cargo.toml`
- Create: `apps/monitor/src/main.rs`

- [ ] **Step 1: Create `apps/monitor/Cargo.toml`**

```toml
[package]
name = "sola-monitor"
version.workspace = true
edition.workspace = true

[[bin]]
name = "sola-monitor"
path = "src/main.rs"

[dependencies]
sola-app = { path = "../../crates/sola-app" }
sola-bus = { path = "../../crates/sola-bus" }
sola-core = { path = "../../crates/sola-core" }
gtk4 = "0.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
hex = "0.4"
```

- [ ] **Step 2: Create `apps/monitor/src/main.rs`**

```rust
use serde_json::Value;
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_bus::Message;
use sola_bus::topics::Topic;

mod decode;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
};

struct MonitorApp {
    main_window: WindowHandle,
}

impl SolaApp for MonitorApp {
    const APP_ID: &'static str = "sola-monitor";

    fn new(ctx: &mut AppCtx) -> Self {
        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (900, 600),
            position: None,
            decorated: true,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: None,
            zoned: false,
            keyboard_target: false,
        });

        tracing::info!("sola-monitor ready");

        Self { main_window }
    }

    fn on_raw_bus_message(&mut self, msg: &Message, _ctx: &mut AppCtx) {
        let event = decode::message_to_json(msg);
        self.main_window.send_to_js(&event);
    }

    fn on_js_command(
        &mut self,
        _cmd: &str,
        _args: &Value,
        _id: Option<u64>,
        _source: &WindowHandle,
        _ctx: &mut AppCtx,
    ) {
        // No JS commands needed for the monitor
    }

    fn on_bus_event(&mut self, _topic: &Topic, _ctx: &mut AppCtx) {
        // All handling is in on_raw_bus_message
    }
}

fn main() {
    sola_app::run::<MonitorApp>();
}
```

- [ ] **Step 3: Verify it compiles (will fail — `decode` module and web files missing)**

Run: `cargo check -p sola-monitor`
Expected: errors about missing `decode` module and web files. That's correct — we'll add them next.

---

### Task 3: Message decoding module

**Files:**
- Create: `apps/monitor/src/decode.rs`

This module converts a raw `Message` into a JSON value suitable for the frontend.

- [ ] **Step 1: Create `apps/monitor/src/decode.rs`**

```rust
use serde_json::{Value, json};
use sola_bus::Message;
use sola_bus::topics::Topic;

/// Convert a raw bus message into a JSON event for the frontend.
pub fn message_to_json(msg: &Message) -> Value {
    let topic_name = &msg.topic;
    let timestamp = msg.timestamp_ms();
    let id = msg.id.to_string();

    let (payload, raw_hex) = decode_payload(msg);

    json!({
        "event": "bus_message",
        "id": id,
        "timestamp": timestamp,
        "topic": topic_name,
        "sticky": msg.sticky,
        "source": msg.sticky_tag,
        "payload": payload,
        "rawHex": raw_hex,
    })
}

/// Attempt to decode the payload via Topic::parse, falling back to hex.
fn decode_payload(msg: &Message) -> (Value, Value) {
    if let Some(topic) = Topic::parse(msg) {
        let payload = topic_to_json(&topic);
        return (payload, Value::Null);
    }

    // Unknown topic or decode failure — return raw hex
    match &msg.payload {
        Some(bytes) => (Value::Null, Value::String(hex::encode(bytes))),
        None => (Value::Null, Value::Null),
    }
}

/// Convert a parsed Topic's payload into a JSON value.
fn topic_to_json(topic: &Topic) -> Value {
    match topic {
        Topic::Apps(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::LaunchApp(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::Composition(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::Frame(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::Focus(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::SetWindowPolicy(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::OutputGeometry(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::MouseEntered(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::SetAppMenu(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::MenuAction(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::ShellKeyBindings(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::OpenUrl(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::Shutdown => Value::Null,
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p sola-monitor`
Expected: errors only about missing web files (the `include_str!` macros). Rust code should be clean.

- [ ] **Step 3: Commit**

```bash
git add apps/monitor/Cargo.toml apps/monitor/src/main.rs apps/monitor/src/decode.rs
git commit -m "feat(monitor): add sola-monitor app scaffold with message decoding"
```

---

### Task 4: Frontend — HTML entry point and CSS theme

**Files:**
- Create: `apps/monitor/web/index.html`
- Create: `apps/monitor/web/src/theme.css`

- [ ] **Step 1: Create `apps/monitor/web/index.html`**

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>sola-monitor</title>
  <link rel="stylesheet" href="/src/theme.css">
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

- [ ] **Step 2: Create `apps/monitor/web/src/theme.css`**

Design: dark precision-instrument aesthetic. Topic colors defined as CSS variables. Monospace data, clean sans-serif UI chrome. Color-coded left borders per topic category.

```css
@import url('https://fonts.googleapis.com/css2?family=DM+Sans:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap');

:root {
  /* Surface */
  --bg-primary: #0d1117;
  --bg-secondary: #161b22;
  --bg-tertiary: #1c2129;
  --bg-row-alt: #12161d;
  --bg-selected: #1f2937;
  --bg-hover: #1a2030;
  --border: #2d333b;
  --border-subtle: #21262d;

  /* Text */
  --text-primary: #e6edf3;
  --text-secondary: #8b949e;
  --text-muted: #484f58;
  --text-accent: #58a6ff;

  /* Topic colors */
  --topic-lifecycle: #f0883e;    /* Apps, LaunchApp, Shutdown */
  --topic-composition: #58a6ff;  /* Composition, Frame, Focus */
  --topic-window: #d2a8ff;       /* SetWindowPolicy, OutputGeometry */
  --topic-input: #3fb950;        /* MouseEntered, ShellKeyBindings */
  --topic-menu: #f778ba;         /* SetAppMenu, MenuAction */
  --topic-browser: #79c0ff;      /* OpenUrl */
  --topic-unknown: #484f58;

  /* Sticky indicator */
  --sticky-dot: #f0883e;

  /* Fonts */
  --font-mono: 'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace;
  --font-ui: 'DM Sans', system-ui, sans-serif;

  /* Sizes */
  --row-height: 28px;
  --toolbar-height: 44px;
  --detail-height: 200px;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html, body {
  height: 100%;
  overflow: hidden;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: var(--font-ui);
  font-size: 13px;
  -webkit-font-smoothing: antialiased;
}

#app {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

/* --- Toolbar --- */

.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 12px;
  height: var(--toolbar-height);
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.toolbar input[type="text"] {
  flex: 1;
  max-width: 320px;
  height: 28px;
  padding: 0 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 12px;
  outline: none;
  transition: border-color 0.15s;
}

.toolbar input[type="text"]:focus {
  border-color: var(--text-accent);
}

.toolbar input[type="text"]::placeholder {
  color: var(--text-muted);
}

.toolbar select {
  height: 28px;
  padding: 0 8px;
  background: var(--bg-primary);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-secondary);
  font-family: var(--font-ui);
  font-size: 12px;
  outline: none;
  cursor: pointer;
}

.toolbar button {
  height: 28px;
  padding: 0 12px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-secondary);
  font-family: var(--font-ui);
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
  white-space: nowrap;
}

.toolbar button:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.toolbar button.active {
  background: #1a2744;
  border-color: var(--text-accent);
  color: var(--text-accent);
}

.toolbar .spacer {
  flex: 1;
}

.toolbar .count {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-muted);
  padding-right: 4px;
}

/* --- Message table header --- */

.table-header {
  display: grid;
  grid-template-columns: 100px 160px 140px 28px 1fr;
  align-items: center;
  height: 26px;
  padding: 0 12px 0 16px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border);
  font-size: 11px;
  font-weight: 500;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  flex-shrink: 0;
}

/* --- Message list --- */

.message-list {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  min-height: 0;
}

.message-list::-webkit-scrollbar {
  width: 8px;
}

.message-list::-webkit-scrollbar-track {
  background: var(--bg-primary);
}

.message-list::-webkit-scrollbar-thumb {
  background: var(--border);
  border-radius: 4px;
}

.message-list::-webkit-scrollbar-thumb:hover {
  background: var(--text-muted);
}

/* --- Message row --- */

.message-row {
  display: grid;
  grid-template-columns: 100px 160px 140px 28px 1fr;
  align-items: center;
  height: var(--row-height);
  padding: 0 12px 0 0;
  border-bottom: 1px solid var(--border-subtle);
  cursor: pointer;
  transition: background 0.1s;
  border-left: 3px solid transparent;
  padding-left: 13px;
}

.message-row:nth-child(even) {
  background: var(--bg-row-alt);
}

.message-row:hover {
  background: var(--bg-hover);
}

.message-row.selected {
  background: var(--bg-selected);
}

/* Topic color borders */
.message-row[data-category="lifecycle"]   { border-left-color: var(--topic-lifecycle); }
.message-row[data-category="composition"] { border-left-color: var(--topic-composition); }
.message-row[data-category="window"]      { border-left-color: var(--topic-window); }
.message-row[data-category="input"]       { border-left-color: var(--topic-input); }
.message-row[data-category="menu"]        { border-left-color: var(--topic-menu); }
.message-row[data-category="browser"]     { border-left-color: var(--topic-browser); }
.message-row[data-category="unknown"]     { border-left-color: var(--topic-unknown); }

.message-row .cell {
  font-family: var(--font-mono);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.message-row .cell.time {
  color: var(--text-muted);
}

.message-row .cell.topic {
  color: var(--text-primary);
  font-weight: 500;
}

/* Topic text colors (match border) */
.message-row[data-category="lifecycle"]   .cell.topic { color: var(--topic-lifecycle); }
.message-row[data-category="composition"] .cell.topic { color: var(--topic-composition); }
.message-row[data-category="window"]      .cell.topic { color: var(--topic-window); }
.message-row[data-category="input"]       .cell.topic { color: var(--topic-input); }
.message-row[data-category="menu"]        .cell.topic { color: var(--topic-menu); }
.message-row[data-category="browser"]     .cell.topic { color: var(--topic-browser); }
.message-row[data-category="unknown"]     .cell.topic { color: var(--topic-unknown); }

.message-row .cell.source {
  color: var(--text-secondary);
}

.message-row .cell.sticky {
  text-align: center;
  font-size: 8px;
}

.message-row .cell.sticky .dot {
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sticky-dot);
}

.message-row .cell.preview {
  color: var(--text-muted);
  font-size: 11px;
}

/* --- Detail pane --- */

.detail-pane {
  height: var(--detail-height);
  background: var(--bg-secondary);
  border-top: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  overflow: hidden;
}

.detail-pane.hidden {
  display: none;
}

.detail-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-subtle);
  font-size: 12px;
  flex-shrink: 0;
}

.detail-header .detail-topic {
  font-family: var(--font-mono);
  font-weight: 500;
}

.detail-header .detail-meta {
  color: var(--text-muted);
  font-family: var(--font-mono);
  font-size: 11px;
}

.detail-header .detail-close {
  margin-left: auto;
  background: none;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 16px;
  line-height: 1;
  padding: 2px 6px;
}

.detail-header .detail-close:hover {
  color: var(--text-primary);
}

.detail-body {
  flex: 1;
  overflow: auto;
  padding: 10px 12px;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  color: var(--text-primary);
}

/* --- Auto-scroll indicator --- */

.auto-scroll-indicator {
  position: fixed;
  bottom: 8px;
  left: 50%;
  transform: translateX(-50%);
  padding: 4px 12px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 12px;
  font-size: 11px;
  color: var(--text-muted);
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.2s;
  z-index: 10;
}

.auto-scroll-indicator.visible {
  opacity: 1;
}

.auto-scroll-indicator:hover {
  color: var(--text-primary);
  border-color: var(--text-accent);
}
```

- [ ] **Step 3: Commit**

```bash
git add apps/monitor/web/index.html apps/monitor/web/src/theme.css
git commit -m "feat(monitor): add HTML entry point and CSS theme"
```

---

### Task 5: Frontend — TypeScript application

**Files:**
- Create: `apps/monitor/web/src/main.ts`
- Create: `apps/monitor/web/src/app.ts`

- [ ] **Step 1: Create `apps/monitor/web/src/main.ts`**

```typescript
import { createApp } from './app.js';

createApp(document.getElementById('app')!).catch((e) => {
  document.title = 'app-error:' + String(e);
  console.error('[sola-monitor] createApp failed:', e);
});
```

- [ ] **Step 2: Create `apps/monitor/web/src/app.ts`**

```typescript
import { html, reactive } from '@arrow-js/core';
import { on } from '@sola/ipc';

// --- Types ---

interface BusMessage {
  id: string;
  timestamp: number;
  topic: string;
  sticky: boolean;
  source: string;
  payload: any;
  rawHex: string | null;
}

// --- Topic categories ---

const TOPIC_CATEGORIES: Record<string, string> = {
  Apps: 'lifecycle',
  LaunchApp: 'lifecycle',
  Shutdown: 'lifecycle',
  Composition: 'composition',
  Frame: 'composition',
  Focus: 'composition',
  SetWindowPolicy: 'window',
  OutputGeometry: 'window',
  MouseEntered: 'input',
  ShellKeyBindings: 'input',
  SetAppMenu: 'menu',
  MenuAction: 'menu',
  OpenUrl: 'browser',
};

function categoryOf(topic: string): string {
  return TOPIC_CATEGORIES[topic] || 'unknown';
}

// --- State ---

const MAX_MESSAGES = 5000;

const state = reactive({
  messages: [] as BusMessage[],
  filteredMessages: [] as BusMessage[],
  selectedId: null as string | null,
  selectedMessage: null as BusMessage | null,
  paused: false,
  filter: '',
  topicFilter: '',
  count: 0,
  autoScroll: true,
});

// Buffer for messages received while paused
let pauseBuffer: BusMessage[] = [];

// --- Topic list (for dropdown) ---
const seenTopics = new Set<string>();

// --- Filtering ---

function applyFilter() {
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
}

// --- Time formatting ---

function formatTime(ms: number): string {
  const d = new Date(ms);
  const h = String(d.getHours()).padStart(2, '0');
  const m = String(d.getMinutes()).padStart(2, '0');
  const s = String(d.getSeconds()).padStart(2, '0');
  const ms_ = String(d.getMilliseconds()).padStart(3, '0');
  return `${h}:${m}:${s}.${ms_}`;
}

function previewPayload(msg: BusMessage): string {
  if (msg.payload != null) {
    const str = JSON.stringify(msg.payload);
    return str.length > 80 ? str.slice(0, 80) + '...' : str;
  }
  if (msg.rawHex) return `[hex: ${msg.rawHex.slice(0, 40)}...]`;
  return '';
}

// --- Message handling ---

function addMessage(msg: BusMessage) {
  seenTopics.add(msg.topic);

  if (state.paused) {
    pauseBuffer.push(msg);
    return;
  }

  state.messages = [...state.messages.slice(-(MAX_MESSAGES - 1)), msg];
  state.count = state.messages.length;
  applyFilter();
}

// --- Actions ---

function togglePause() {
  state.paused = !state.paused;
  if (!state.paused && pauseBuffer.length > 0) {
    state.messages = [...state.messages, ...pauseBuffer].slice(-MAX_MESSAGES);
    pauseBuffer = [];
    state.count = state.messages.length;
    applyFilter();
  }
}

function clearMessages() {
  state.messages = [];
  state.filteredMessages = [];
  state.count = 0;
  state.selectedId = null;
  state.selectedMessage = null;
}

function selectMessage(msg: BusMessage | null) {
  if (msg) {
    state.selectedId = msg.id;
    state.selectedMessage = msg;
  } else {
    state.selectedId = null;
    state.selectedMessage = null;
  }
}

// --- Scroll management ---

let listEl: HTMLElement | null = null;

function scrollToBottom() {
  if (listEl && state.autoScroll) {
    listEl.scrollTop = listEl.scrollHeight;
  }
}

function onListScroll() {
  if (!listEl) return;
  const atBottom = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 40;
  state.autoScroll = atBottom;
}

function jumpToBottom() {
  state.autoScroll = true;
  scrollToBottom();
}

// --- Rendering ---

export async function createApp(root: HTMLElement) {
  // Subscribe to bus messages from Rust host
  on('bus_message', (msg: BusMessage) => {
    addMessage(msg);
    requestAnimationFrame(scrollToBottom);
  });

  // Render
  const template = html`
    <div class="toolbar">
      <input
        type="text"
        placeholder="Filter messages..."
        @input="${(e: Event) => {
          state.filter = (e.target as HTMLInputElement).value;
          applyFilter();
        }}"
      />
      <select
        @change="${(e: Event) => {
          state.topicFilter = (e.target as HTMLSelectElement).value;
          applyFilter();
        }}"
      >
        <option value="">All topics</option>
      </select>
      <button
        class="${() => (state.paused ? 'active' : '')}"
        @click="${togglePause}"
      >
        ${() => (state.paused ? `Resume (${pauseBuffer.length})` : 'Pause')}
      </button>
      <button @click="${clearMessages}">Clear</button>
      <div class="spacer"></div>
      <span class="count">${() => state.count} msgs</span>
    </div>

    <div class="table-header">
      <span>Time</span>
      <span>Topic</span>
      <span>Source</span>
      <span>S</span>
      <span>Preview</span>
    </div>

    <div
      class="message-list"
      id="message-list"
      @scroll="${onListScroll}"
    >
      ${() =>
        state.filteredMessages.map(
          (msg) => html`
            <div
              class="${`message-row${state.selectedId === msg.id ? ' selected' : ''}`}"
              data-category="${categoryOf(msg.topic)}"
              @click="${() => selectMessage(msg)}"
            >
              <span class="cell time">${formatTime(msg.timestamp)}</span>
              <span class="cell topic">${msg.topic}</span>
              <span class="cell source">${msg.source || '\u2014'}</span>
              <span class="cell sticky">${msg.sticky ? html`<span class="dot"></span>` : ''}</span>
              <span class="cell preview">${previewPayload(msg)}</span>
            </div>
          `
        )}
    </div>

    <div class="${() => `detail-pane${state.selectedMessage ? '' : ' hidden'}`}">
      <div class="detail-header">
        <span
          class="detail-topic"
          style="${() => `color: var(--topic-${state.selectedMessage ? categoryOf(state.selectedMessage.topic) : 'unknown'})`}"
        >
          ${() => state.selectedMessage?.topic || ''}
        </span>
        <span class="detail-meta">
          ${() => {
            const m = state.selectedMessage;
            if (!m) return '';
            const parts = [];
            if (m.source) parts.push(m.source);
            if (m.sticky) parts.push('sticky');
            parts.push(m.id);
            return parts.join(' \u00b7 ');
          }}
        </span>
        <button class="detail-close" @click="${() => selectMessage(null)}">\u00d7</button>
      </div>
      <div class="detail-body">
        ${() => {
          const m = state.selectedMessage;
          if (!m) return '';
          if (m.payload != null) return JSON.stringify(m.payload, null, 2);
          if (m.rawHex) return `[raw hex]\n${m.rawHex}`;
          return '(no payload)';
        }}
      </div>
    </div>

    <div
      class="${() => `auto-scroll-indicator${!state.autoScroll ? ' visible' : ''}`}"
      @click="${jumpToBottom}"
    >
      \u2193 Auto-scroll paused \u2014 click to resume
    </div>
  `;

  template(root);

  // Grab the list element for scroll management
  listEl = document.getElementById('message-list');

  // Populate topic dropdown dynamically as topics arrive
  const selectEl = root.querySelector('select');
  if (selectEl) {
    const observer = new MutationObserver(() => {
      const current = new Set(
        Array.from(selectEl.options).map((o) => o.value).filter(Boolean)
      );
      for (const topic of seenTopics) {
        if (!current.has(topic)) {
          const option = document.createElement('option');
          option.value = topic;
          option.textContent = topic;
          selectEl.appendChild(option);
        }
      }
    });
    // Trigger on list changes (new messages rendered)
    const listContainer = document.querySelector('.message-list');
    if (listContainer) {
      observer.observe(listContainer, { childList: true });
    }
  }
}
```

- [ ] **Step 3: Verify full project compiles**

Run: `cargo check -p sola-monitor`
Expected: clean compilation

- [ ] **Step 4: Commit**

```bash
git add apps/monitor/web/src/main.ts apps/monitor/web/src/app.ts
git commit -m "feat(monitor): add frontend application with message list and detail pane"
```

---

### Task 6: Final verification

- [ ] **Step 1: Full workspace check**

Run: `cargo check`
Expected: entire workspace compiles cleanly

- [ ] **Step 2: Verify asset bundle completeness**

Check that every file referenced in `APP_ASSETS` in `main.rs` exists on disk:
- `apps/monitor/web/index.html`
- `apps/monitor/web/src/main.ts`
- `apps/monitor/web/src/app.ts`
- `apps/monitor/web/src/theme.css`

- [ ] **Step 3: Commit any remaining changes**

If there are fixups, commit them. Then verify the branch is clean.
