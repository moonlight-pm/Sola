# Sola Browser Design

Port of the Cogsworth browser to Sola. WebKit6-based browser with vertical tabs, address bar with autocomplete, session persistence, and sola-bus integration. Arrow.js frontend, no build step.

## Architecture

Single OS process (`sola-browser`), single thread (glib main loop). GTK4 Application with a `gtk4::Fixed` container holding one chrome WebView and N tab content WebViews. Communication between chrome and Rust via WebKit6 UserContentManager. Bus polling via `glib::timeout_add_local`.

### Window Layout

```
┌──────────┬────────────────────────────────┐
│          │  address bar + nav buttons      │
│  tab     │────────────────────────────────│
│  sidebar │                                │
│          │  tab content (WebView)          │
│          │                                │
│          │                                │
└──────────┴────────────────────────────────┘
```

- **Chrome WebView:** Full window size, renders vertical tab sidebar (left) and address bar + nav buttons (top). Positioned at (0, 0).
- **Tab WebViews:** One per open tab. Positioned at `(sidebar_width, address_bar_height)`, sized to fill remaining space. Only the active tab is visible.

Rust owns the Fixed container and recalculates positions on window resize. Chrome WebView is added to the Fixed first, tab WebViews after — GTK4 Fixed renders later children on top, so tab content naturally overlays the chrome's content area.

## Features

- Tabbed browsing with per-tab WebView (separate web process per tab)
- Vertical tab sidebar (left)
- Address bar with browsing history autocomplete
- Navigation: back, forward, reload
- Search: non-URL input goes to Kagi (`https://kagi.com/search?q=...`)
- Session persistence: tab URLs, titles, and WebViewSessionState across restarts
- Browsing history: URL/title/visit-count, capped at 1000 entries
- Emacs keybindings injected into tab WebViews
- Download handling with toast notifications
- Bus-driven shortcuts: Super+T (new tab), Super+W (close tab), Super+L (focus address bar)
- OpenUrl bus topic: other apps can request the browser open a URL

## File Structure

```
apps/browser/
├── Cargo.toml
├── src/
│   ├── main.rs        # GTK4 init, WebView setup, bus connection, glib loop
│   ├── ipc.rs         # UserContentManager setup, command dispatch, event pushing
│   ├── tabs.rs        # Tab lifecycle (create/close/switch), WebView management
│   ├── state.rs       # Tab + history persistence, JSON read/write
│   └── chrome.rs      # Chrome layout calculations, WebView positioning
└── web/
    ├── arrow.js       # Vendored Arrow.js ESM
    ├── index.html     # Chrome entry point
    ├── app.js         # Root component, IPC bridge, state management
    ├── tabs.js        # Vertical tab sidebar component
    ├── address.js     # Address bar + autocomplete component
    └── style.css      # Chrome styles
```

## Dependencies

### Rust

- `gtk4`, `gdk4`, `glib`, `gio` — GTK4 windowing
- `webkit6` — WebKit6 WebViews and UserContentManager
- `sola-bus` — IPC bus client + topic definitions
- `serde`, `serde_json` — state serialization
- `uuid` — tab IDs
- `base64` — WebViewSessionState encoding
- `include_dir` — embed web/ dist at compile time
- `tracing`, `tracing-subscriber`, `tracing-appender` — logging

### Frontend (web/)

- `arrow.js` — vendored Arrow.js ESM (~5KB), no build step
- No npm, no Vite, no bundler

## Communication

### Chrome WebView <-> Rust (UserContentManager)

Rust registers a `sola` message handler on the chrome WebView's UserContentManager. An init script is injected at document start that provides the IPC bridge:

```javascript
window.sola = {
  invoke: async (command, args) => {
    // Posts JSON to webkit.messageHandlers.sola
    // Returns parsed JSON response
  },
  _handlers: {},
  on: (event, callback) => { /* register event listener */ },
  _emit: (event, data) => { /* called by Rust via evaluate_javascript */ }
};
```

Rust pushes events to JS via `webview.evaluate_javascript("sola._emit('event', data)")`.

### IPC Commands (JS -> Rust)

| Command | Args | Response | Effect |
|---------|------|----------|--------|
| `ready` | — | `{ tabs, activeTabId }` | Chrome mounted, Rust sends restored session state |
| `create_tab` | `{ url?, activate? }` | `{ tabId }` | Create tab WebView, optionally load URL |
| `close_tab` | `{ tabId }` | `"ok"` | Destroy tab WebView, persist state |
| `switch_tab` | `{ tabId }` | `"ok"` | Show/hide WebViews, update focus |
| `navigate` | `{ url }` | `"ok"` | Load URL or Kagi search in active tab |
| `go_back` | — | `"ok"` | Active tab `go_back()` |
| `go_forward` | — | `"ok"` | Active tab `go_forward()` |
| `reload` | — | `"ok"` | Active tab `reload()` |
| `history_search` | `{ query }` | `[{ url, title, visits }]` | Address bar autocomplete suggestions |

### Events (Rust -> JS)

| Event | Data | Trigger |
|-------|------|---------|
| `tab_title_changed` | `{ tabId, title }` | WebKit `notify::title` signal |
| `tab_url_changed` | `{ tabId, url }` | WebKit `notify::uri` signal |
| `tab_load_changed` | `{ tabId, loading }` | WebKit load-changed signal |
| `download_started` | `{ filename, id }` | WebKit download signal |
| `download_progress` | `{ id, progress }` | Download progress callback |
| `download_finished` | `{ id }` | Download complete |
| `bus_new_tab` | `{ url?, activate? }` | Bus `OpenUrl` topic received |
| `bus_focus_address` | — | Bus Super+L received |

### Tab WebViews -> Rust

Tab WebViews render arbitrary web content. Communication is via:

- **WebKit signals:** `notify::title`, `notify::uri`, `notify::estimated-load-progress`, `load-changed`, `decide-policy` (for `target="_blank"` links -> open new tab).
- **Injected scripts:** Emacs keybindings injected via UserScript at document start. Future Super+Click handling will also use injected scripts with a per-tab UserContentManager message handler.

## Bus Integration

### New Topic

Add to `sola-bus/src/topics.rs`:

```rust
OpenUrl { url: String, activate: bool }
```

### Bus Event Handling

Browser's 50ms poll loop handles:

| Topic | Condition | Action |
|-------|-----------|--------|
| `Key` | Super+T, browser focused | Create new tab |
| `Key` | Super+W, browser focused | Close active tab |
| `Key` | Super+L, browser focused | Push `bus_focus_address` event to chrome |
| `OpenUrl` | Any time | Create tab with URL, activate if requested |
| `FocusChanged` | Matches browser app_id | Track focus state |

Browser only acts on Super+key shortcuts when it has focus (tracked via `FocusChanged`).

## Persistence

### Tab State (`~/.config/sola/browser-tabs.json`)

Write-through on every tab mutation (open, close, navigate, switch). Atomic writes via tmp file + rename.

```json
{
  "tabs": [
    {
      "url": "https://github.com",
      "title": "GitHub",
      "sessionState": "base64-encoded-WebViewSessionState"
    }
  ],
  "activeTabId": "uuid-string"
}
```

Session state is captured from `WebViewSessionState::serialize()` and base64-encoded. On restore, `WebViewSessionState::new()` + `restore_session_state()` rebuilds back/forward history.

### Browsing History (`~/.config/sola/browser-history.json`)

Updated on every navigation. Capped at 1000 entries, ordered by most recent visit.

```json
{
  "entries": [
    { "url": "https://github.com", "title": "GitHub", "visits": 15 }
  ]
}
```

Used for address bar autocomplete: substring match on URL and title, sorted by visit count.

## Custom URI Scheme

Register `sola-browser://` on the WebContext. Backed by `include_dir!` embedding the `web/` directory at compile time. Chrome WebView loads `sola-browser://index.html`. Proper MIME types derived from file extension.

## Injected Scripts

### Emacs Keybindings

Injected into each tab WebView via `webkit6::UserScript` at document start, all frames. Same mappings as Cogsworth:

- C-n: next line, C-p: previous line
- C-f: forward char, C-b: backward char
- C-a: beginning of line, C-e: end of line
- C-d: delete forward, C-h: delete backward
- C-k: kill to end of line

### IPC Init Script

Injected into chrome WebView only. Provides `window.sola` object with `invoke()` and `on()` methods.

## WebView Configuration

### Chrome WebView

- Transparent background (`set_background_color` with alpha 0)
- Developer extras enabled
- No network session needed (loads only from custom URI scheme)

### Tab WebViews

- Shared `NetworkSession` with persistent cookie storage and cache
- Shared `WebContext` (for custom URI scheme registration)
- Each tab gets its own `UserContentManager` (for injected scripts)
- Developer extras enabled
- Media playback without user gesture
- Safari-compatible user agent (Cogsworth pattern, avoids Cloudflare issues)
- WebRTC enabled
- `decide-policy` handler: `target="_blank"` links open as new tabs instead of new windows

## Process Manager Integration

Add `"sola-browser"` to the `MANAGED` const in `crates/sola/src/main.rs`. Process manager launches it automatically and restarts on crash.
