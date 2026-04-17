# sola-monitor — Bus Message Inspector

A developer tool for observing all traffic on the sola-bus in real time. Standard sola-app (Rust host + WebView frontend) that connects to the bus as a passive listener and displays every message with decoded payloads.

## Architecture

Same pattern as terminal/browser/shell: a `SolaApp` implementation with a single WebView window.

**Rust host (`apps/monitor/src/main.rs`):**
- Connects to the bus like any app
- Receives all messages via `on_bus_event` — but also needs raw `Message` access for metadata (id, timestamp, sticky, sticky_tag) that `Topic` doesn't expose
- Decodes known topics via `Topic::parse()`, serializes the payload as JSON
- For unknown topics or decode failures, includes hex payload bytes
- Forwards each message to the WebView as a JSON event

**Message forwarding format** (Rust → JS):
```json
{
  "event": "bus_message",
  "id": "uuid-string",
  "timestamp": 1713312000000,
  "topic": "SetWindowPolicy",
  "sticky": true,
  "source": "sola-terminal",
  "payload": { "app_id": "sola-terminal", "windows": [...] },
  "raw_hex": null
}
```

When payload decoding fails, `payload` is `null` and `raw_hex` contains the hex-encoded bytes.

**Key implementation detail:** The standard `on_bus_event` callback only receives parsed `Topic` values. To get raw message metadata (uuid, timestamp, sticky flags), the monitor needs access to the raw `Message`. Two options:

1. **Extend `SolaApp` trait** with an `on_raw_bus_message(&mut self, msg: &Message, topic: Option<&Topic>, ctx: &mut AppCtx)` hook — called before `on_bus_event`, gives access to raw message. Default impl is a no-op.
2. **Poll the bus client directly** — the monitor bypasses the standard event loop and reads from `BusClient` itself.

**Chosen: Option 1.** Adding an optional hook to `SolaApp` is minimal, non-breaking, and keeps the monitor within the standard framework. Other apps ignore it.

## Frontend

Single-page WebView app using Arrow.js (matching existing apps) with a developer-tool aesthetic.

### Layout

```
+------------------------------------------------------------------+
| [filter input___________] [topic ▼] [Pause] [Clear]  842 msgs   |
+------------------------------------------------------------------+
| TIME        | TOPIC             | SOURCE        | S | PREVIEW    |
|-------------|-------------------|---------------|---|------------|
| 14:23:01.442| SetWindowPolicy   | sola-terminal | * | {app_id..} |
| 14:23:01.500| Frame             | sola-shell    |   | {app_id..} |
| 14:23:01.501| Focus             | sola-shell    |   | {app_id..} |
| 14:23:02.100| MouseEntered      | sola-compositor|  | {app_id..} |
| ...         |                   |               |   |            |
+------------------------------------------------------------------+
| DETAIL: SetWindowPolicy                                          |
| Source: sola-terminal | Sticky | ID: 019f3a2b-...               |
| {                                                                |
|   "app_id": "sola-terminal",                                    |
|   "windows": [{ "title": "main", "zoned": true, ... }]          |
| }                                                                |
+------------------------------------------------------------------+
```

- **Top bar:** Text filter (searches topic + payload), topic dropdown filter, pause/resume toggle, clear button, message count
- **Message table:** Compact rows, monospace font. Columns: timestamp (HH:MM:SS.mmm), topic, source (sticky_tag), sticky indicator, payload preview (truncated single line)
- **Detail pane:** Bottom panel showing full message metadata + pretty-printed JSON payload for the selected row

### Behavior

- Auto-scrolls to newest message (pinned to bottom) unless the user has scrolled up
- Pause button stops auto-scroll and buffers incoming messages (resume catches up)
- Message buffer capped at 5000 entries; oldest dropped when exceeded
- Topic color coding: each unique topic gets a consistent color from a palette
- Click a row to select it and show detail; detail pane stays open until explicitly closed or another row is clicked
- Filter is case-insensitive substring match across topic name and JSON payload text

### Styling

- Dark background (`#1a1a2e` / `#16213e` range), light text
- Monospace for all data (timestamps, topics, payloads)
- Sticky messages get a subtle dot/badge in the S column
- Selected row highlighted
- Alternating row shading for readability
- Compact row height (~24px) to maximize visible messages

## Rust-Side Message Decoding

The monitor needs to convert each `Topic` variant's payload into a JSON `serde_json::Value`. Since all payload types already derive `Serialize`, this is straightforward:

```rust
fn topic_to_json(topic: &Topic) -> serde_json::Value {
    match topic {
        Topic::Apps(v) => serde_json::to_value(v).unwrap_or_default(),
        Topic::LaunchApp(v) => serde_json::to_value(v).unwrap_or_default(),
        // ... each variant
        Topic::Shutdown => serde_json::Value::Null,
    }
}
```

This is a manual match, which means new topics require adding a branch. Acceptable for a dev tool — it's explicit and simple.

## Window Configuration

- **Unzoned** (floating, not managed by zone layout)
- **Size:** 900x600 default
- **Decorated:** true (standard window chrome)
- **Keyboard target:** false (doesn't need Meta+key routing)
- **No menu** (no app-specific menu needed for a dev tool)

## Files

```
apps/monitor/
  Cargo.toml
  src/
    main.rs          # SolaApp impl, message decoding, forwarding
  web/
    index.html       # Entry point
    src/
      main.ts        # Bootstrap
      app.ts         # App logic, state, rendering
      theme.css      # Styles
```

## Dependencies

Rust: `sola-app`, `sola-bus`, `sola-core`, `gtk4`, `serde`, `serde_json`, `tracing`, `hex`

Web: `@sola/ipc`, `@sola/store` (both injected by platform), `@arrow-js/core` (injected)

## sola-app Framework Change

Add to `SolaApp` trait in `crates/sola-app/src/lib.rs`:

```rust
/// Called for every raw bus message before topic parsing.
/// Default implementation is a no-op. Override to access message metadata.
fn on_raw_bus_message(&mut self, _msg: &sola_bus::Message, _ctx: &mut AppCtx) {}
```

Call this in the event loop before `on_bus_event`. This is the only framework change needed.
