# Sola Browser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the Cogsworth browser to Sola as a standalone WebKit6 browser app with vertical tabs, address bar with autocomplete, session persistence, and sola-bus integration.

**Architecture:** Single-process GTK4 app with a Fixed container holding one chrome WebView (Arrow.js UI) and N tab content WebViews. Chrome communicates with Rust via WebKit6 UserContentManager. Bus polling on glib main loop for keyboard shortcuts and external URL requests.

**Tech Stack:** Rust, GTK4 0.9, WebKit6 0.4, sola-bus, Arrow.js (vendored ESM), include_dir

**Design spec:** `docs/specs/2026-04-11-sola-browser-design.md`

---

## Task 1: Project Scaffolding

**Files:**
- Create: `apps/browser/Cargo.toml`
- Create: `apps/browser/src/main.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "sola-browser"
version.workspace = true
edition.workspace = true

[[bin]]
name = "sola-browser"
path = "src/main.rs"

[dependencies]
sola-bus = { path = "../../crates/sola-bus" }
gtk4 = "0.9"
gdk4 = "0.9"
glib = "0.20"
gio = "0.20"
webkit6 = "0.4"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
base64 = "0.22"
include_dir = "0.7"
```

- [ ] **Step 2: Create minimal main.rs**

```rust
fn main() {
    println!("sola-browser stub");
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo make build sola-browser`
Expected: Build succeeds

- [ ] **Step 4: Commit**

```bash
git add apps/browser/
git commit -m "feat(browser): scaffold project with dependencies"
```

---

## Task 2: Add OpenUrl Bus Topic

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`

- [ ] **Step 1: Add OpenUrl struct and topic variant**

In `crates/sola-bus/src/topics.rs`, add the struct alongside existing structs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenUrlRequest {
    pub url: String,
    pub activate: bool,
}
```

Then add the variant inside the `define_topics!` macro invocation, alongside the existing variants:

```rust
OpenUrl(OpenUrlRequest),
```

- [ ] **Step 2: Verify sola-bus compiles**

Run: `cargo make build sola-bus`
Expected: Build succeeds

- [ ] **Step 3: Verify dependent crates compile**

Run: `cargo make build`
Expected: Full workspace builds (existing code using Topic enum still compiles because we only added a variant)

- [ ] **Step 4: Commit**

```bash
git add crates/sola-bus/src/topics.rs
git commit -m "feat(bus): add OpenUrl topic for browser URL requests"
```

---

## Task 3: State Persistence

**Files:**
- Create: `apps/browser/src/state.rs`
- Modify: `apps/browser/src/main.rs` (add module declaration)

- [ ] **Step 1: Write tests for BrowserTabStore**

Create `apps/browser/src/state.rs` with test module first:

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTab {
    pub url: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_state: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TabStore {
    pub tabs: Vec<PersistedTab>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visits: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BrowsingHistory {
    pub entries: Vec<HistoryEntry>,
}

const MAX_HISTORY_ENTRIES: usize = 1000;

impl TabStore {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        let dir = path.parent().expect("tab store path must have parent");
        std::fs::create_dir_all(dir).ok();
        let tmp = path.with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, &json).is_ok() {
                std::fs::rename(&tmp, path).ok();
            }
        }
    }
}

impl BrowsingHistory {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        let dir = path.parent().expect("history path must have parent");
        std::fs::create_dir_all(dir).ok();
        let tmp = path.with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, &json).is_ok() {
                std::fs::rename(&tmp, path).ok();
            }
        }
    }

    pub fn record_visit(&mut self, url: &str, title: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.url == url) {
            entry.title = title.to_string();
            entry.visits += 1;
        } else {
            self.entries.push(HistoryEntry {
                url: url.to_string(),
                title: title.to_string(),
                visits: 1,
            });
        }
        // Move visited entry to front
        if let Some(pos) = self.entries.iter().position(|e| e.url == url) {
            let entry = self.entries.remove(pos);
            self.entries.insert(0, entry);
        }
        self.entries.truncate(MAX_HISTORY_ENTRIES);
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<&HistoryEntry> {
        let query_lower = query.to_lowercase();
        let mut matches: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                e.url.to_lowercase().contains(&query_lower)
                    || e.title.to_lowercase().contains(&query_lower)
            })
            .collect();
        matches.sort_by(|a, b| b.visits.cmp(&a.visits));
        matches.truncate(limit);
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("sola-browser-test");
        fs::create_dir_all(&dir).ok();
        dir.join(name)
    }

    #[test]
    fn tab_store_round_trip() {
        let path = tmp_path("tabs-rt.json");
        let store = TabStore {
            tabs: vec![PersistedTab {
                url: "https://example.com".into(),
                title: "Example".into(),
                session_state: Some("abc123".into()),
            }],
            active_tab_id: Some("tab-1".into()),
        };
        store.save(&path);
        let loaded = TabStore::load(&path);
        assert_eq!(loaded.tabs.len(), 1);
        assert_eq!(loaded.tabs[0].url, "https://example.com");
        assert_eq!(loaded.tabs[0].session_state.as_deref(), Some("abc123"));
        assert_eq!(loaded.active_tab_id.as_deref(), Some("tab-1"));
        fs::remove_file(&path).ok();
    }

    #[test]
    fn tab_store_load_missing_file() {
        let path = tmp_path("nonexistent.json");
        let store = TabStore::load(&path);
        assert!(store.tabs.is_empty());
        assert!(store.active_tab_id.is_none());
    }

    #[test]
    fn history_record_and_search() {
        let mut history = BrowsingHistory::default();
        history.record_visit("https://github.com", "GitHub");
        history.record_visit("https://github.com", "GitHub");
        history.record_visit("https://example.com", "Example");
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.entries[0].url, "https://example.com");
        assert_eq!(history.entries[1].visits, 2);

        let results = history.search("git", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://github.com");
    }

    #[test]
    fn history_caps_at_max() {
        let mut history = BrowsingHistory::default();
        for i in 0..1100 {
            history.record_visit(&format!("https://example.com/{i}"), "Test");
        }
        assert_eq!(history.entries.len(), MAX_HISTORY_ENTRIES);
    }

    #[test]
    fn history_round_trip() {
        let path = tmp_path("history-rt.json");
        let mut history = BrowsingHistory::default();
        history.record_visit("https://github.com", "GitHub");
        history.save(&path);
        let loaded = BrowsingHistory::load(&path);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].url, "https://github.com");
        fs::remove_file(&path).ok();
    }
}
```

- [ ] **Step 2: Add module declaration to main.rs**

Update `apps/browser/src/main.rs`:

```rust
mod state;

fn main() {
    println!("sola-browser stub");
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p sola-browser`
Expected: All 5 tests pass

- [ ] **Step 4: Commit**

```bash
git add apps/browser/src/state.rs apps/browser/src/main.rs
git commit -m "feat(browser): add tab store and browsing history persistence"
```

---

## Task 4: Vendor Arrow.js and Create Frontend Scaffold

**Files:**
- Create: `apps/browser/web/arrow.js`
- Create: `apps/browser/web/index.html`
- Create: `apps/browser/web/style.css`
- Create: `apps/browser/web/app.js`
- Create: `apps/browser/web/tabs.js`
- Create: `apps/browser/web/address.js`

- [ ] **Step 1: Download and vendor Arrow.js**

Run: `curl -L "https://esm.sh/@aspect-build/arrow-js" -o /tmp/arrow-check.js && echo "Check arrow.js availability"`

If the above doesn't work, download from npm:

Run: `cd /tmp && npm pack @arrow-js/core 2>/dev/null && ls arrow-js-core-*.tgz`

Extract and copy the ESM module to `apps/browser/web/arrow.js`. If npm packages aren't available, download from the CDN URL shown on https://arrow-js.com/ and save to `apps/browser/web/arrow.js`.

The goal is a single `arrow.js` file that exports `reactive`, `html`, and `watch`.

- [ ] **Step 2: Create index.html**

Create `apps/browser/web/index.html`:

```html
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="sola-browser://style.css">
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    html, body { width: 100%; height: 100%; overflow: hidden; background: transparent; }
  </style>
</head>
<body>
  <div id="app"></div>
  <script type="module" src="sola-browser://app.js"></script>
</body>
</html>
```

- [ ] **Step 3: Create style.css**

Create `apps/browser/web/style.css`:

```css
:root {
  --sidebar-width: 200px;
  --topbar-height: 40px;
  --bg: #1e1e2e;
  --bg-surface: #313244;
  --bg-hover: #45475a;
  --bg-active: #585b70;
  --text: #cdd6f4;
  --text-dim: #a6adc8;
  --accent: #89b4fa;
  --border: #45475a;
  font-family: system-ui, -apple-system, sans-serif;
  font-size: 13px;
  color: var(--text);
}

#app {
  width: 100vw;
  height: 100vh;
  display: grid;
  grid-template-columns: var(--sidebar-width) 1fr;
  grid-template-rows: var(--topbar-height) 1fr;
}

/* Sidebar */
.tab-sidebar {
  grid-column: 1;
  grid-row: 1 / -1;
  background: var(--bg);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.tab-sidebar-header {
  padding: 8px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--border);
}

.tab-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px;
}

.tab-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.tab-item:hover {
  background: var(--bg-hover);
}

.tab-item.active {
  background: var(--bg-active);
}

.tab-item-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tab-item-close {
  opacity: 0;
  cursor: pointer;
  color: var(--text-dim);
  font-size: 14px;
  line-height: 1;
  padding: 2px 4px;
  border-radius: 2px;
  border: none;
  background: none;
}

.tab-item:hover .tab-item-close {
  opacity: 1;
}

.tab-item-close:hover {
  background: var(--bg-hover);
  color: var(--text);
}

/* Top bar */
.top-bar {
  grid-column: 2;
  grid-row: 1;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 8px;
}

.nav-btn {
  background: none;
  border: none;
  color: var(--text-dim);
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 4px;
  font-size: 16px;
  line-height: 1;
}

.nav-btn:hover {
  background: var(--bg-hover);
  color: var(--text);
}

.nav-btn:disabled {
  opacity: 0.3;
  cursor: default;
}

.address-bar {
  flex: 1;
  position: relative;
}

.address-input {
  width: 100%;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg-surface);
  color: var(--text);
  font-size: 13px;
  outline: none;
}

.address-input:focus {
  border-color: var(--accent);
}

.autocomplete-list {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 0 0 6px 6px;
  max-height: 300px;
  overflow-y: auto;
  z-index: 100;
}

.autocomplete-item {
  padding: 6px 10px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.autocomplete-item:hover,
.autocomplete-item.selected {
  background: var(--bg-hover);
}

.autocomplete-item-title {
  font-size: 13px;
}

.autocomplete-item-url {
  font-size: 11px;
  color: var(--text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* New tab button */
.new-tab-btn {
  background: none;
  border: none;
  color: var(--text-dim);
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 4px;
  font-size: 16px;
}

.new-tab-btn:hover {
  background: var(--bg-hover);
  color: var(--text);
}

/* Download toast */
.download-toast {
  position: fixed;
  bottom: 12px;
  right: 12px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 14px;
  font-size: 12px;
  color: var(--text);
  z-index: 200;
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
```

- [ ] **Step 4: Create app.js — root component and IPC bridge**

Create `apps/browser/web/app.js`:

```javascript
import { reactive, html } from './arrow.js';
import { renderTabs } from './tabs.js';
import { renderAddressBar } from './address.js';

// window.sola is injected by Rust via UserContentManager init script.
// It provides: sola.invoke(command, args), sola.on(event, cb), sola._emit(event, data)

// --- App State ---
export const state = reactive({
  tabs: [],
  activeTabId: null,
  addressValue: '',
  addressFocused: false,
  suggestions: [],
  downloads: [],
});

// --- Actions ---
export async function createTab(url, activate = true) {
  const result = await sola.invoke('create_tab', { url, activate });
  state.tabs = [...state.tabs, {
    id: result.tabId,
    url: url || '',
    title: 'New Tab',
    loading: false,
  }];
  if (activate) {
    state.activeTabId = result.tabId;
    state.addressValue = url || '';
  }
}

export async function closeTab(tabId) {
  await sola.invoke('close_tab', { tabId });
  state.tabs = state.tabs.filter(t => t.id !== tabId);
  if (state.activeTabId === tabId) {
    const remaining = state.tabs;
    if (remaining.length > 0) {
      await switchTab(remaining[remaining.length - 1].id);
    }
  }
}

export async function switchTab(tabId) {
  await sola.invoke('switch_tab', { tabId });
  state.activeTabId = tabId;
  const tab = state.tabs.find(t => t.id === tabId);
  if (tab) state.addressValue = tab.url;
}

export async function navigate(input) {
  const url = looksLikeUrl(input)
    ? (input.startsWith('http') ? input : `https://${input}`)
    : `https://kagi.com/search?q=${encodeURIComponent(input)}`;
  await sola.invoke('navigate', { url });
}

export async function goBack() { await sola.invoke('go_back'); }
export async function goForward() { await sola.invoke('go_forward'); }
export async function reload() { await sola.invoke('reload'); }

export async function searchHistory(query) {
  if (!query || query.length < 2) {
    state.suggestions = [];
    return;
  }
  const results = await sola.invoke('history_search', { query });
  state.suggestions = results || [];
}

function looksLikeUrl(input) {
  return /^https?:\/\//.test(input)
    || /^localhost(:\d+)?/.test(input)
    || /^[\w-]+\.[\w.-]+/.test(input);
}

// --- Events from Rust ---
sola.on('tab_title_changed', ({ tabId, title }) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, title } : t
  );
});

sola.on('tab_url_changed', ({ tabId, url }) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, url } : t
  );
  if (tabId === state.activeTabId) {
    state.addressValue = url;
  }
});

sola.on('tab_load_changed', ({ tabId, loading }) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, loading } : t
  );
});

sola.on('bus_new_tab', ({ tabId, url, activate }) => {
  // Tab WebView already created by Rust — just update frontend state
  state.tabs = [...state.tabs, {
    id: tabId,
    url: url || '',
    title: 'New Tab',
    loading: true,
  }];
  if (activate !== false) {
    state.activeTabId = tabId;
    state.addressValue = url || '';
  }
});

sola.on('bus_focus_address', () => {
  const input = document.querySelector('.address-input');
  if (input) {
    input.focus();
    input.select();
  }
});

sola.on('download_started', ({ id, filename }) => {
  state.downloads = [...state.downloads, { id, filename, progress: 0 }];
});

sola.on('download_progress', ({ id, progress }) => {
  state.downloads = state.downloads.map(d =>
    d.id === id ? { ...d, progress } : d
  );
});

sola.on('download_finished', ({ id }) => {
  setTimeout(() => {
    state.downloads = state.downloads.filter(d => d.id !== id);
  }, 3000);
});

// --- Render ---
function renderDownloads() {
  return html`${() => state.downloads.map(d =>
    html`<div class="download-toast">${() => d.filename} — ${() => Math.round(d.progress * 100)}%</div>`
  )}`;
}

function render() {
  const app = document.getElementById('app');
  app.append(
    renderTabs(),
    html`<div class="top-bar">
      <button class="nav-btn" @click="${goBack}">&#9664;</button>
      <button class="nav-btn" @click="${goForward}">&#9654;</button>
      <button class="nav-btn" @click="${reload}">&#8635;</button>
      ${renderAddressBar()}
    </div>`,
    renderDownloads(),
  );
}

// --- Init ---
async function init() {
  const session = await sola.invoke('ready');
  if (session.tabs && session.tabs.length > 0) {
    state.tabs = session.tabs;
    state.activeTabId = session.activeTabId || session.tabs[0].id;
    state.addressValue = state.tabs.find(t => t.id === state.activeTabId)?.url || '';
  }
  render();
  if (state.tabs.length === 0) {
    await createTab('about:blank');
  }
}

init();
```

- [ ] **Step 5: Create tabs.js — vertical tab sidebar**

Create `apps/browser/web/tabs.js`:

```javascript
import { html } from './arrow.js';
import { state, createTab, closeTab, switchTab } from './app.js';

export function renderTabs() {
  return html`
    <div class="tab-sidebar">
      <div class="tab-sidebar-header">
        <span style="font-weight: 600; font-size: 12px;">Tabs</span>
        <button class="new-tab-btn" @click="${() => createTab('about:blank')}" title="New Tab">+</button>
      </div>
      <div class="tab-list">
        ${() => state.tabs.map(tab =>
          html`<div
            class="${() => `tab-item ${tab.id === state.activeTabId ? 'active' : ''}`}"
            @click="${() => switchTab(tab.id)}"
          >
            <span class="tab-item-title">${() => tab.title || tab.url || 'New Tab'}</span>
            <button class="tab-item-close" @click="${(e) => { e.stopPropagation(); closeTab(tab.id); }}" title="Close tab">&times;</button>
          </div>`
        )}
      </div>
    </div>
  `;
}
```

- [ ] **Step 6: Create address.js — address bar with autocomplete**

Create `apps/browser/web/address.js`:

```javascript
import { html } from './arrow.js';
import { state, navigate, searchHistory } from './app.js';

let debounceTimer = null;

function onInput(e) {
  state.addressValue = e.target.value;
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => searchHistory(state.addressValue), 150);
}

function onKeyDown(e) {
  if (e.key === 'Enter') {
    e.preventDefault();
    state.suggestions = [];
    navigate(state.addressValue);
    e.target.blur();
  } else if (e.key === 'Escape') {
    state.suggestions = [];
    e.target.blur();
  }
}

function selectSuggestion(url) {
  state.addressValue = url;
  state.suggestions = [];
  navigate(url);
}

function onFocus(e) {
  e.target.select();
  state.addressFocused = true;
}

function onBlur() {
  // Delay to allow click on suggestion
  setTimeout(() => {
    state.addressFocused = false;
    state.suggestions = [];
  }, 200);
}

export function renderAddressBar() {
  return html`
    <div class="address-bar">
      <input
        class="address-input"
        type="text"
        placeholder="Search or enter URL"
        value="${() => state.addressValue}"
        @input="${onInput}"
        @keydown="${onKeyDown}"
        @focus="${onFocus}"
        @blur="${onBlur}"
      />
      ${() => state.suggestions.length > 0 ? html`
        <div class="autocomplete-list">
          ${() => state.suggestions.map(s =>
            html`<div class="autocomplete-item" @mousedown="${() => selectSuggestion(s.url)}">
              <span class="autocomplete-item-title">${() => s.title}</span>
              <span class="autocomplete-item-url">${() => s.url}</span>
            </div>`
          )}
        </div>
      ` : html``}
    </div>
  `;
}
```

- [ ] **Step 7: Commit**

```bash
git add apps/browser/web/
git commit -m "feat(browser): add Arrow.js frontend scaffold with tab sidebar, address bar, and IPC bridge"
```

---

## Task 5: GTK Application, WebView, and Custom URI Scheme

**Files:**
- Modify: `apps/browser/src/main.rs`
- Create: `apps/browser/src/chrome.rs`

- [ ] **Step 1: Create chrome.rs — layout constants and calculations**

Create `apps/browser/src/chrome.rs`:

```rust
pub const SIDEBAR_WIDTH: i32 = 200;
pub const TOPBAR_HEIGHT: i32 = 40;

pub struct ContentArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn content_area(window_width: i32, window_height: i32) -> ContentArea {
    ContentArea {
        x: SIDEBAR_WIDTH,
        y: TOPBAR_HEIGHT,
        width: (window_width - SIDEBAR_WIDTH).max(0),
        height: (window_height - TOPBAR_HEIGHT).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_area_calculation() {
        let area = content_area(1920, 1080);
        assert_eq!(area.x, 200);
        assert_eq!(area.y, 40);
        assert_eq!(area.width, 1720);
        assert_eq!(area.height, 1040);
    }

    #[test]
    fn content_area_small_window() {
        let area = content_area(100, 30);
        assert_eq!(area.width, 0);
        assert_eq!(area.height, 0);
    }
}
```

- [ ] **Step 2: Write the full main.rs with GTK app, URI scheme, and chrome WebView**

Replace `apps/browser/src/main.rs`:

```rust
mod chrome;
mod state;

use glib::prelude::*;
use gtk4::prelude::*;
use include_dir::{include_dir, Dir};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use sola_bus::client::BusClient;
use sola_bus::topics::Topic;

static WEB_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web");

fn config_dir() -> PathBuf {
    let dir = glib::user_config_dir().join("sola");
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn mime_from_extension(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

fn setup_logging() {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let log_dir = "/opt/sola/log";
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_browser=info".into());

    let stderr_layer = fmt::layer().with_writer(std::io::stderr);

    if let Ok(file_appender) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{log_dir}/sola-browser.log"))
    {
        let file_layer = fmt::layer()
            .with_writer(std::sync::Mutex::new(file_appender))
            .with_ansi(false);
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .with(file_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .init();
    }
}

fn wait_for_wayland_socket() -> bool {
    let display = match std::env::var("WAYLAND_DISPLAY") {
        Ok(d) => d,
        Err(_) => {
            tracing::error!("WAYLAND_DISPLAY not set");
            return false;
        }
    };
    let runtime_dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) => d,
        Err(_) => {
            tracing::error!("XDG_RUNTIME_DIR not set");
            return false;
        }
    };
    let socket_path = PathBuf::from(&runtime_dir).join(&display);
    for attempt in 1..=20 {
        if socket_path.exists() {
            tracing::info!("wayland socket ready (attempt {attempt})");
            return true;
        }
        tracing::debug!("waiting for wayland socket (attempt {attempt}/20)");
        std::thread::sleep(Duration::from_millis(500));
    }
    tracing::error!("wayland socket not found after 10s");
    false
}

fn main() {
    setup_logging();
    tracing::info!("sola-browser starting");

    if !wait_for_wayland_socket() {
        std::process::exit(1);
    }

    glib::set_prgname(Some("sola-browser"));

    let app = gtk4::Application::new(None::<&str>, Default::default());
    app.connect_activate(build_ui);
    app.run_with_args::<String>(&[]);
}

fn build_ui(app: &gtk4::Application) {
    let display = gdk4::Display::default().expect("could not get display");

    // Transparent window CSS
    let css = gtk4::CssProvider::new();
    css.load_from_string("window, window.background { background: transparent; }");
    gtk4::style_context_add_provider_for_display(
        &display,
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Window
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Sola Browser")
        .default_width(1920)
        .default_height(1080)
        .decorated(false)
        .build();

    let container = gtk4::Fixed::new();
    window.set_child(Some(&container));

    // WebKit setup
    let web_context = webkit6::WebContext::new();
    web_context.register_uri_scheme("sola-browser", |request| {
        let path = request.path().unwrap_or_default().to_string();
        let path = path.strip_prefix('/').unwrap_or(&path);
        let path = if path.is_empty() { "index.html" } else { path };
        match WEB_DIST.get_file(path) {
            Some(file) => {
                let data = file.contents();
                let mime = mime_from_extension(path);
                let bytes = glib::Bytes::from(data);
                let stream = gio::MemoryInputStream::from_bytes(&bytes);
                request.finish(&stream, data.len() as i64, Some(mime));
            }
            None => {
                tracing::warn!("embedded file not found: {path}");
                let bytes = glib::Bytes::from(b"Not Found" as &[u8]);
                let stream = gio::MemoryInputStream::from_bytes(&bytes);
                request.finish(&stream, 9, Some("text/plain"));
            }
        }
    });

    // Network session for tab WebViews (cookies, cache)
    let data_dir = glib::user_data_dir().join("sola").join("browser");
    let cache_dir = glib::user_cache_dir().join("sola").join("browser");
    std::fs::create_dir_all(&data_dir).ok();
    std::fs::create_dir_all(&cache_dir).ok();
    let network_session = webkit6::NetworkSession::new(
        Some(data_dir.to_str().unwrap()),
        Some(cache_dir.to_str().unwrap()),
    );
    if let Some(cookie_mgr) = network_session.cookie_manager() {
        let cookie_db = data_dir.join("cookies.db");
        cookie_mgr.set_persistent_storage(
            cookie_db.to_str().unwrap(),
            webkit6::CookiePersistentStorage::Sqlite,
        );
    }

    // Chrome WebView
    let chrome_manager = webkit6::UserContentManager::new();
    let chrome_webview = webkit6::WebView::builder()
        .web_context(&web_context)
        .user_content_manager(&chrome_manager)
        .build();
    chrome_webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = chrome_webview.settings() {
        settings.set_enable_developer_extras(true);
    }

    container.put(&chrome_webview, 0.0, 0.0);
    chrome_webview.set_size_request(1920, 1080);
    chrome_webview.load_uri("sola-browser://index.html");

    // Shared state
    let app_state = Rc::new(AppState {
        container: container.clone(),
        chrome_webview: chrome_webview.clone(),
        web_context,
        network_session,
        tab_store_path: config_dir().join("browser-tabs.json"),
        history_path: config_dir().join("browser-history.json"),
        tab_store: RefCell::new(state::TabStore::load(&config_dir().join("browser-tabs.json"))),
        history: RefCell::new(state::BrowsingHistory::load(
            &config_dir().join("browser-history.json"),
        )),
        tabs: RefCell::new(Vec::new()),
        active_tab_id: RefCell::new(None),
        focused: RefCell::new(false),
    });

    // IPC setup (Task 6 will fill this in)
    // ipc::setup(&chrome_manager, &app_state);

    // Bus connection
    let bus: Rc<RefCell<Option<BusClient>>> = Rc::new(RefCell::new(None));
    match BusClient::connect() {
        Ok(client) => {
            tracing::info!("connected to bus");
            *bus.borrow_mut() = Some(client);
        }
        Err(e) => tracing::warn!("bus not available: {e}"),
    }

    // Bus poll loop (Task 9 will fill this in)
    // glib::timeout_add_local(Duration::from_millis(50), { ... });

    // Handle window resize
    window.connect_default_width_notify({
        let app_state = app_state.clone();
        move |win| resize_views(&app_state, win.width(), win.height())
    });
    window.connect_default_height_notify({
        let app_state = app_state.clone();
        move |win| resize_views(&app_state, win.width(), win.height())
    });

    window.present();
}

fn resize_views(app_state: &AppState, width: i32, height: i32) {
    app_state.chrome_webview.set_size_request(width, height);
    let area = chrome::content_area(width, height);
    for tab in app_state.tabs.borrow().iter() {
        tab.webview.set_size_request(area.width, area.height);
        app_state
            .container
            .move_(&tab.webview, area.x as f64, area.y as f64);
    }
}

struct Tab {
    id: String,
    webview: webkit6::WebView,
}

struct AppState {
    container: gtk4::Fixed,
    chrome_webview: webkit6::WebView,
    web_context: webkit6::WebContext,
    network_session: webkit6::NetworkSession,
    tab_store_path: PathBuf,
    history_path: PathBuf,
    tab_store: RefCell<state::TabStore>,
    history: RefCell<state::BrowsingHistory>,
    tabs: RefCell<Vec<Tab>>,
    active_tab_id: RefCell<Option<String>>,
    focused: RefCell<bool>,
}

impl AppState {
    fn persist_tabs(&self) {
        self.tab_store.borrow().save(&self.tab_store_path);
    }

    fn persist_history(&self) {
        self.history.borrow().save(&self.history_path);
    }
}
```

- [ ] **Step 3: Add chrome module declaration**

Already included in the main.rs above (`mod chrome;`).

- [ ] **Step 4: Verify it compiles**

Run: `cargo make build sola-browser`
Expected: Build succeeds (IPC and bus loop are commented out)

- [ ] **Step 5: Run tests**

Run: `cargo test -p sola-browser`
Expected: All tests from Task 3 + chrome tests pass

- [ ] **Step 6: Commit**

```bash
git add apps/browser/src/
git commit -m "feat(browser): GTK4 app with WebView, URI scheme, and chrome layout"
```

---

## Task 6: IPC Bridge

**Files:**
- Create: `apps/browser/src/ipc.rs`
- Modify: `apps/browser/src/main.rs` (add module, wire up)

- [ ] **Step 1: Create ipc.rs with init script injection and command dispatch**

Create `apps/browser/src/ipc.rs`:

```rust
use crate::AppState;
use std::rc::Rc;
use webkit6::prelude::*;

const INIT_SCRIPT: &str = r#"
(function() {
    window.sola = {
        _handlers: {},
        _nextId: 0,

        invoke(command, args = {}) {
            const callbackId = String(this._nextId++);
            return window.webkit.messageHandlers.sola
                .postMessage(JSON.stringify({ command, args, callbackId }))
                .then(raw => {
                    try { return JSON.parse(raw); } catch { return raw; }
                });
        },

        on(event, callback) {
            if (!this._handlers[event]) this._handlers[event] = new Set();
            this._handlers[event].add(callback);
            return () => this._handlers[event]?.delete(callback);
        },

        _emit(event, data) {
            const handlers = this._handlers[event];
            if (handlers) {
                for (const cb of handlers) {
                    try { cb(typeof data === 'string' ? JSON.parse(data) : data); }
                    catch (e) { console.error('sola event error:', e); }
                }
            }
        }
    };
})();
"#;

pub fn inject_init_script(manager: &webkit6::UserContentManager) {
    let script = webkit6::UserScript::new(
        INIT_SCRIPT,
        webkit6::UserContentInjectedFrames::AllFrames,
        webkit6::UserScriptInjectionTime::AtDocumentStart,
        &[],
        &[],
    );
    manager.add_script(&script);
}

pub fn emit_event(webview: &webkit6::WebView, event: &str, data: &str) {
    let js = format!("window.sola?._emit('{event}', '{data}')");
    webview.evaluate_javascript(&js, None, None, None::<&gio::Cancellable>, |_| {});
}

pub fn emit_event_json(webview: &webkit6::WebView, event: &str, data: &serde_json::Value) {
    let json_str = serde_json::to_string(data).unwrap_or_default();
    // Escape for JS string literal
    let escaped = json_str.replace('\\', "\\\\").replace('\'', "\\'");
    let js = format!("window.sola?._emit('{event}', '{escaped}')");
    webview.evaluate_javascript(&js, None, None, None::<&gio::Cancellable>, |_| {});
}

pub fn setup(manager: &webkit6::UserContentManager, app_state: &Rc<AppState>) {
    inject_init_script(manager);

    manager.register_script_message_handler("sola", None);

    let state = app_state.clone();
    manager.connect_script_message_with_reply_received(move |_mgr, js_value, reply| {
        let msg_str = match js_value.to_str() {
            Some(s) => s.to_string(),
            None => {
                reply.return_error_message("invalid message");
                return true;
            }
        };

        let msg: serde_json::Value = match serde_json::from_str(&msg_str) {
            Ok(v) => v,
            Err(_) => {
                reply.return_error_message("invalid json");
                return true;
            }
        };

        let command = msg["command"].as_str().unwrap_or("");
        let args = &msg["args"];

        let result = handle_command(&state, command, args);

        let response_str = match &result {
            Ok(val) => serde_json::to_string(val).unwrap_or_else(|_| "null".into()),
            Err(e) => {
                tracing::warn!("ipc command '{command}' failed: {e}");
                format!(r#"{{"error":"{e}"}}"#)
            }
        };

        let ctx = js_value.context().unwrap();
        let js_result = webkit6::jsc::Value::new_string(&ctx, Some(&response_str));
        reply.return_value(&js_result);
        true
    });
}

fn handle_command(
    state: &Rc<AppState>,
    command: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match command {
        "ready" => cmd_ready(state),
        "create_tab" => cmd_create_tab(state, args),
        "close_tab" => cmd_close_tab(state, args),
        "switch_tab" => cmd_switch_tab(state, args),
        "navigate" => cmd_navigate(state, args),
        "go_back" => cmd_go_back(state),
        "go_forward" => cmd_go_forward(state),
        "reload" => cmd_reload(state),
        "history_search" => cmd_history_search(state, args),
        _ => Err(format!("unknown command: {command}")),
    }
}

fn cmd_ready(state: &Rc<AppState>) -> Result<serde_json::Value, String> {
    // Return persisted session for the frontend to render
    let store = state.tab_store.borrow();
    let tabs: Vec<serde_json::Value> = store
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let tab_id = format!("restored-{i}");
            serde_json::json!({
                "id": tab_id,
                "url": t.url,
                "title": t.title,
            })
        })
        .collect();
    let active = store.active_tab_id.clone();
    drop(store);

    // Create actual WebViews for restored tabs
    for (i, persisted) in state.tab_store.borrow().tabs.iter().enumerate() {
        let tab_id = format!("restored-{i}");
        crate::tabs::create_tab_webview(state, &tab_id, Some(&persisted.url), persisted.session_state.as_deref());
    }

    // Activate first restored tab
    let active_id = active.unwrap_or_else(|| {
        if !tabs.is_empty() {
            tabs[0]["id"].as_str().unwrap_or("").to_string()
        } else {
            String::new()
        }
    });
    if !active_id.is_empty() {
        crate::tabs::switch_tab(state, &active_id);
    }

    Ok(serde_json::json!({
        "tabs": tabs,
        "activeTabId": active_id,
    }))
}

fn cmd_create_tab(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = args["url"].as_str();
    let activate = args["activate"].as_bool().unwrap_or(true);
    let tab_id = uuid::Uuid::new_v4().to_string();
    crate::tabs::create_tab_webview(state, &tab_id, url, None);
    if activate {
        crate::tabs::switch_tab(state, &tab_id);
    }

    // Persist
    let mut store = state.tab_store.borrow_mut();
    store.tabs.push(crate::state::PersistedTab {
        url: url.unwrap_or("").to_string(),
        title: String::new(),
        session_state: None,
    });
    drop(store);
    state.persist_tabs();

    Ok(serde_json::json!({ "tabId": tab_id }))
}

fn cmd_close_tab(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = args["tabId"].as_str().ok_or("missing tabId")?;
    crate::tabs::close_tab(state, tab_id);
    Ok(serde_json::json!("ok"))
}

fn cmd_switch_tab(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = args["tabId"].as_str().ok_or("missing tabId")?;
    crate::tabs::switch_tab(state, tab_id);
    Ok(serde_json::json!("ok"))
}

fn cmd_navigate(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = args["url"].as_str().ok_or("missing url")?;
    crate::tabs::navigate_active(state, url);
    Ok(serde_json::json!("ok"))
}

fn cmd_go_back(state: &Rc<AppState>) -> Result<serde_json::Value, String> {
    crate::tabs::go_back(state);
    Ok(serde_json::json!("ok"))
}

fn cmd_go_forward(state: &Rc<AppState>) -> Result<serde_json::Value, String> {
    crate::tabs::go_forward(state);
    Ok(serde_json::json!("ok"))
}

fn cmd_reload(state: &Rc<AppState>) -> Result<serde_json::Value, String> {
    crate::tabs::reload(state);
    Ok(serde_json::json!("ok"))
}

fn cmd_history_search(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let query = args["query"].as_str().ok_or("missing query")?;
    let history = state.history.borrow();
    let results: Vec<serde_json::Value> = history
        .search(query, 10)
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "url": e.url,
                "title": e.title,
                "visits": e.visits,
            })
        })
        .collect();
    Ok(serde_json::json!(results))
}
```

- [ ] **Step 2: Add ipc module to main.rs and wire up setup**

In `apps/browser/src/main.rs`, add `mod ipc;` at the top with the other modules, and uncomment the IPC setup line in `build_ui`:

```rust
mod chrome;
mod ipc;
mod state;
```

Replace the commented-out IPC line:
```rust
    // IPC setup (Task 6 will fill this in)
    // ipc::setup(&chrome_manager, &app_state);
```
with:
```rust
    ipc::setup(&chrome_manager, &app_state);
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo make build sola-browser`
Expected: Build succeeds (tabs module referenced but not yet created — this will fail, which is expected. We'll create it in Task 7.)

Actually, this won't compile yet because `ipc.rs` references `crate::tabs::*` which doesn't exist. For now, comment out the body of `handle_command` to return a stub:

Temporarily replace the `handle_command` match body with:

```rust
fn handle_command(
    _state: &Rc<AppState>,
    command: &str,
    _args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    tracing::debug!("ipc command: {command}");
    Err(format!("not yet implemented: {command}"))
}
```

Comment out the `cmd_*` functions (they'll be restored in Task 8).

Run: `cargo make build sola-browser`
Expected: Build succeeds

- [ ] **Step 4: Commit**

```bash
git add apps/browser/src/ipc.rs apps/browser/src/main.rs
git commit -m "feat(browser): IPC bridge with UserContentManager and init script"
```

---

## Task 7: Tab Management

**Files:**
- Create: `apps/browser/src/tabs.rs`
- Modify: `apps/browser/src/main.rs` (add module)

- [ ] **Step 1: Create tabs.rs with tab WebView lifecycle**

Create `apps/browser/src/tabs.rs`:

```rust
use crate::ipc;
use crate::AppState;
use std::rc::Rc;
use webkit6::prelude::*;

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

const EMACS_SCRIPT: &str = r#"
(function() {
    if (window.__sola_emacs) return;
    window.__sola_emacs = true;

    document.addEventListener('keydown', function(e) {
        if (!e.ctrlKey) return;

        var el = document.activeElement;
        if (!el) return;
        if (el.closest && el.closest('.cm-editor')) return;

        var isText = el.tagName === 'TEXTAREA'
            || (el.tagName === 'INPUT' && /^(text|search|url|email|password|tel|number)$/i.test(el.type || 'text'))
            || el.isContentEditable;
        if (!isText) return;

        var handled = true;
        switch (e.key) {
            case 'f': move('forward', 'character'); break;
            case 'b': move('backward', 'character'); break;
            case 'n': move('forward', 'line'); break;
            case 'p': move('backward', 'line'); break;
            case 'a': move('backward', 'lineboundary'); break;
            case 'e': move('forward', 'lineboundary'); break;
            case 'd': document.execCommand('forwardDelete'); break;
            case 'h': document.execCommand('delete'); break;
            case 'k': {
                var sel = window.getSelection();
                sel.modify('extend', 'forward', 'lineboundary');
                if (!sel.isCollapsed) document.execCommand('delete');
                break;
            }
            default: handled = false;
        }
        if (handled) { e.preventDefault(); e.stopPropagation(); }

        function move(dir, gran) {
            var sel = window.getSelection();
            if (sel) sel.modify('move', dir, gran);
        }
    }, true);
})();
"#;

pub fn create_tab_webview(
    state: &Rc<AppState>,
    tab_id: &str,
    url: Option<&str>,
    session_state_b64: Option<&str>,
) {
    let manager = webkit6::UserContentManager::new();

    // Inject emacs keybindings
    let emacs = webkit6::UserScript::new(
        EMACS_SCRIPT,
        webkit6::UserContentInjectedFrames::AllFrames,
        webkit6::UserScriptInjectionTime::AtDocumentEnd,
        &[],
        &[],
    );
    manager.add_script(&emacs);

    let webview = webkit6::WebView::builder()
        .web_context(&state.web_context)
        .network_session(&state.network_session)
        .user_content_manager(&manager)
        .build();

    if let Some(settings) = webview.settings() {
        settings.set_enable_developer_extras(true);
        settings.set_media_playback_requires_user_gesture(false);
        settings.set_user_agent(Some(USER_AGENT));
    }

    // Restore session state (back/forward history)
    if let Some(b64) = session_state_b64 {
        if let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64) {
            let gbytes = glib::Bytes::from(&bytes);
            let session = webkit6::WebViewSessionState::new(&gbytes);
            webview.restore_session_state(&session);
        }
    }

    // Load URL
    let load_url = url.unwrap_or("about:blank");
    webview.load_uri(load_url);

    // Position in content area
    let area = crate::chrome::content_area(
        state.chrome_webview.width(),
        state.chrome_webview.height(),
    );
    state
        .container
        .put(&webview, area.x as f64, area.y as f64);
    webview.set_size_request(area.width, area.height);
    webview.set_visible(false); // Hidden until switched to

    // Track title changes
    let chrome_wv = state.chrome_webview.clone();
    let tid = tab_id.to_string();
    webview.connect_notify_local(Some("title"), move |wv, _| {
        if let Some(title) = wv.title() {
            let data = serde_json::json!({ "tabId": tid, "title": title.to_string() });
            ipc::emit_event_json(&chrome_wv, "tab_title_changed", &data);
        }
    });

    // Track URL changes
    let chrome_wv = state.chrome_webview.clone();
    let tid = tab_id.to_string();
    let state_ref = state.clone();
    webview.connect_notify_local(Some("uri"), move |wv, _| {
        if let Some(uri) = wv.uri() {
            let url_str = uri.to_string();
            let data = serde_json::json!({ "tabId": tid, "url": url_str });
            ipc::emit_event_json(&chrome_wv, "tab_url_changed", &data);

            // Record in history
            let title = wv.title().map(|t| t.to_string()).unwrap_or_default();
            state_ref
                .history
                .borrow_mut()
                .record_visit(&url_str, &title);
            state_ref.persist_history();
        }
    });

    // Track load state
    let chrome_wv = state.chrome_webview.clone();
    let tid = tab_id.to_string();
    webview.connect_notify_local(Some("is-loading"), move |wv, _| {
        let loading = wv.is_loading();
        let data = serde_json::json!({ "tabId": tid, "loading": loading });
        ipc::emit_event_json(&chrome_wv, "tab_load_changed", &data);
    });

    // Handle target="_blank" — open as new tab
    let state_ref = state.clone();
    webview.connect_decide_policy(move |_wv, decision, decision_type| {
        if decision_type == webkit6::PolicyDecisionType::NewWindowAction {
            if let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() {
                if let Some(action) = nav.navigation_action() {
                    if let Some(request) = action.request() {
                        if let Some(uri) = request.uri() {
                            let url = uri.to_string();
                            let tab_id = uuid::Uuid::new_v4().to_string();
                            create_tab_webview(&state_ref, &tab_id, Some(&url), None);
                            switch_tab(&state_ref, &tab_id);
                            let data = serde_json::json!({
                                "tabId": tab_id,
                                "url": url,
                                "activate": true,
                            });
                            ipc::emit_event_json(
                                &state_ref.chrome_webview,
                                "bus_new_tab",
                                &data,
                            );
                        }
                    }
                }
            }
            decision.ignore();
            return true;
        }
        false
    });

    // Handle downloads
    let chrome_wv = state.chrome_webview.clone();
    webview.connect_download_started(move |_wv, download| {
        let id = uuid::Uuid::new_v4().to_string();
        let filename = download
            .response()
            .and_then(|r| r.suggested_filename())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "download".to_string());

        let data = serde_json::json!({ "id": id, "filename": filename });
        ipc::emit_event_json(&chrome_wv, "download_started", &data);

        let chrome_wv2 = chrome_wv.clone();
        let id2 = id.clone();
        download.connect_received_data(move |dl, _len| {
            let progress = dl.estimated_progress();
            let data = serde_json::json!({ "id": id2, "progress": progress });
            ipc::emit_event_json(&chrome_wv2, "download_progress", &data);
        });

        let chrome_wv3 = chrome_wv.clone();
        let id3 = id.clone();
        download.connect_finished(move |_dl| {
            let data = serde_json::json!({ "id": id3 });
            ipc::emit_event_json(&chrome_wv3, "download_finished", &data);
        });
    });

    state.tabs.borrow_mut().push(crate::Tab {
        id: tab_id.to_string(),
        webview,
    });
}

pub fn switch_tab(state: &Rc<AppState>, tab_id: &str) {
    let tabs = state.tabs.borrow();

    // Hide current
    if let Some(current_id) = state.active_tab_id.borrow().as_ref() {
        if let Some(tab) = tabs.iter().find(|t| t.id == *current_id) {
            tab.webview.set_visible(false);
        }
    }

    // Show new
    if let Some(tab) = tabs.iter().find(|t| t.id == tab_id) {
        tab.webview.set_visible(true);
        tab.webview.grab_focus();
    }

    drop(tabs);
    *state.active_tab_id.borrow_mut() = Some(tab_id.to_string());

    // Persist
    state.tab_store.borrow_mut().active_tab_id = Some(tab_id.to_string());
    state.persist_tabs();
}

pub fn close_tab(state: &Rc<AppState>, tab_id: &str) {
    let mut tabs = state.tabs.borrow_mut();
    if let Some(pos) = tabs.iter().position(|t| t.id == tab_id) {
        let tab = tabs.remove(pos);
        drop(tabs);
        tab.webview.unparent();

        // Update persisted store
        let mut store = state.tab_store.borrow_mut();
        if pos < store.tabs.len() {
            store.tabs.remove(pos);
        }
        drop(store);
        state.persist_tabs();
    }
}

pub fn navigate_active(state: &Rc<AppState>, url: &str) {
    let active_id = state.active_tab_id.borrow().clone();
    if let Some(id) = active_id {
        let tabs = state.tabs.borrow();
        if let Some(tab) = tabs.iter().find(|t| t.id == id) {
            tab.webview.load_uri(url);
        }
    }
}

pub fn go_back(state: &Rc<AppState>) {
    let active_id = state.active_tab_id.borrow().clone();
    if let Some(id) = active_id {
        let tabs = state.tabs.borrow();
        if let Some(tab) = tabs.iter().find(|t| t.id == id) {
            tab.webview.go_back();
        }
    }
}

pub fn go_forward(state: &Rc<AppState>) {
    let active_id = state.active_tab_id.borrow().clone();
    if let Some(id) = active_id {
        let tabs = state.tabs.borrow();
        if let Some(tab) = tabs.iter().find(|t| t.id == id) {
            tab.webview.go_forward();
        }
    }
}

pub fn reload(state: &Rc<AppState>) {
    let active_id = state.active_tab_id.borrow().clone();
    if let Some(id) = active_id {
        let tabs = state.tabs.borrow();
        if let Some(tab) = tabs.iter().find(|t| t.id == id) {
            tab.webview.reload();
        }
    }
}

pub fn capture_session_state(state: &Rc<AppState>) {
    let tabs = state.tabs.borrow();
    let mut store = state.tab_store.borrow_mut();

    for (i, tab) in tabs.iter().enumerate() {
        if i < store.tabs.len() {
            // Capture URL
            if let Some(uri) = tab.webview.uri() {
                store.tabs[i].url = uri.to_string();
            }
            if let Some(title) = tab.webview.title() {
                store.tabs[i].title = title.to_string();
            }
            // Capture session state for back/forward history
            if let Some(session) = tab.webview.session_state() {
                let bytes = session.serialize();
                if let Some(bytes) = bytes {
                    let b64 = base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        bytes.as_ref(),
                    );
                    store.tabs[i].session_state = Some(b64);
                }
            }
        }
    }

    drop(store);
    drop(tabs);
    state.persist_tabs();
}
```

- [ ] **Step 2: Add module declaration and use base64 imports**

In `apps/browser/src/main.rs`, add `mod tabs;` with the other modules:

```rust
mod chrome;
mod ipc;
mod state;
mod tabs;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo make build sola-browser`
Expected: Build may have API issues with WebKit6 signal names or base64 crate usage. Fix any compile errors — the exact WebKit6 API may differ slightly from Cogsworth's version. Common fixes:
- `base64::Engine::decode` might need `use base64::Engine;` import
- `connect_download_started` signal name may differ
- `connect_received_data` may not exist on Download — check `connect_notify_local(Some("estimated-progress"))` instead

- [ ] **Step 4: Commit**

```bash
git add apps/browser/src/tabs.rs apps/browser/src/main.rs
git commit -m "feat(browser): tab WebView lifecycle with navigation, emacs keys, and downloads"
```

---

## Task 8: Restore Full IPC Command Handlers

**Files:**
- Modify: `apps/browser/src/ipc.rs`

- [ ] **Step 1: Replace the stub handle_command with the full implementation**

In `apps/browser/src/ipc.rs`, replace the stub `handle_command` with the full version from Task 6 Step 1 (the one with all the `cmd_*` functions). Restore all the `cmd_*` functions that were commented out.

- [ ] **Step 2: Verify it compiles**

Run: `cargo make build sola-browser`
Expected: Build succeeds — all IPC commands now route to tabs module

- [ ] **Step 3: Commit**

```bash
git add apps/browser/src/ipc.rs
git commit -m "feat(browser): wire IPC commands to tab manager"
```

---

## Task 9: Bus Integration

**Files:**
- Modify: `apps/browser/src/main.rs`

- [ ] **Step 1: Add keycode constants and bus poll loop**

In `apps/browser/src/main.rs`, add keycode module at the top:

```rust
mod keycode {
    pub const T: u32 = 28;
    pub const W: u32 = 25;
    pub const L: u32 = 46;
}
```

Then replace the commented-out bus poll loop in `build_ui` with:

```rust
    // Bus poll loop
    glib::timeout_add_local(Duration::from_millis(50), {
        let bus = bus.clone();
        let app_state = app_state.clone();
        move || {
            let mut bus_ref = bus.borrow_mut();
            if let Some(ref mut client) = *bus_ref {
                while let Some(msg) = client.try_recv() {
                    let Some(topic) = Topic::parse(&msg) else {
                        continue;
                    };
                    match topic {
                        Topic::Key(key) => {
                            if !*app_state.focused.borrow() || !key.pressed || !key.super_held {
                                continue;
                            }
                            match key.code {
                                keycode::T => {
                                    tracing::debug!("super+t: new tab");
                                    let tab_id = uuid::Uuid::new_v4().to_string();
                                    tabs::create_tab_webview(
                                        &app_state,
                                        &tab_id,
                                        Some("about:blank"),
                                        None,
                                    );
                                    tabs::switch_tab(&app_state, &tab_id);
                                    let data = serde_json::json!({
                                        "tabId": tab_id,
                                        "url": "about:blank",
                                        "activate": true,
                                    });
                                    ipc::emit_event_json(
                                        &app_state.chrome_webview,
                                        "bus_new_tab",
                                        &data,
                                    );
                                }
                                keycode::W => {
                                    tracing::debug!("super+w: close tab");
                                    if let Some(id) = app_state.active_tab_id.borrow().clone() {
                                        tabs::close_tab(&app_state, &id);
                                    }
                                }
                                keycode::L => {
                                    tracing::debug!("super+l: focus address bar");
                                    ipc::emit_event_json(
                                        &app_state.chrome_webview,
                                        "bus_focus_address",
                                        &serde_json::json!({}),
                                    );
                                }
                                _ => {}
                            }
                        }
                        Topic::FocusChanged(app_id) => {
                            let focused = app_id == "sola-browser";
                            *app_state.focused.borrow_mut() = focused;
                            tracing::debug!("focus changed: {focused}");
                        }
                        Topic::OpenUrl(req) => {
                            tracing::info!("bus open url: {}", req.url);
                            let tab_id = uuid::Uuid::new_v4().to_string();
                            tabs::create_tab_webview(
                                &app_state,
                                &tab_id,
                                Some(&req.url),
                                None,
                            );
                            if req.activate {
                                tabs::switch_tab(&app_state, &tab_id);
                            }
                            let data = serde_json::json!({
                                "tabId": tab_id,
                                "url": req.url,
                                "activate": req.activate,
                            });
                            ipc::emit_event_json(
                                &app_state.chrome_webview,
                                "bus_new_tab",
                                &data,
                            );
                        }
                        _ => {}
                    }
                }
            }
            glib::ControlFlow::Continue
        }
    });
```

- [ ] **Step 2: Add session state capture on window close**

Add a `connect_close_request` handler to the window in `build_ui`, before `window.present()`:

```rust
    window.connect_close_request({
        let app_state = app_state.clone();
        move |_| {
            tracing::info!("browser window closing, capturing session state");
            tabs::capture_session_state(&app_state);
            glib::Propagation::Proceed
        }
    });
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo make build sola-browser`
Expected: Build succeeds

- [ ] **Step 4: Commit**

```bash
git add apps/browser/src/main.rs
git commit -m "feat(browser): bus integration with Super+key shortcuts and OpenUrl topic"
```

---

## Task 10: Process Manager Integration

**Files:**
- Modify: `crates/sola/src/main.rs`

- [ ] **Step 1: Add sola-browser to MANAGED const**

In `crates/sola/src/main.rs`, add `"sola-browser"` to the MANAGED array:

```rust
const MANAGED: &[&str] = &["sola-bus", "sola-compositor", "sola-x", "sola-switcher", "sola-browser"];
```

- [ ] **Step 2: Verify full workspace compiles**

Run: `cargo make build`
Expected: Full workspace builds successfully

- [ ] **Step 3: Commit**

```bash
git add crates/sola/src/main.rs
git commit -m "feat(sola): add sola-browser to managed processes"
```

---

## Task 11: Build, Deploy, and Smoke Test

**Files:** None — verification only

- [ ] **Step 1: Run all tests**

Run: `cargo test -p sola-browser`
Expected: All unit tests pass (state persistence, chrome layout)

- [ ] **Step 2: Build release**

Run: `cargo make build`
Expected: Clean build with no warnings relevant to sola-browser

- [ ] **Step 3: Install locally**

Run: `cargo make install browser`
Expected: Successful install. Binary appears at `/opt/sola/bin/sola-browser`.

- [ ] **Step 4: Smoke test on a TTY**

Verify the binary exists and runs:

```bash
ls -la /opt/sola/bin/sola-browser
```

Full testing requires running sola from a physical TTY and interacting with the browser.

- [ ] **Step 5: Final commit if any fixes were needed**

```bash
git add -A
git commit -m "fix(browser): address issues found during smoke test"
```
