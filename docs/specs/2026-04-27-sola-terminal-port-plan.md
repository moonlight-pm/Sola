# sola-terminal Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `apps/terminal/` to `crates/sola-terminal/`, register it as a builtin app, replace its `terminal-state.json` with two persistent bus topics, drop the custom-tab-name feature, and make the app menu's `Tab N` entries reflect the actual tab count.

**Architecture:** The crate moves verbatim into the workspace with a few targeted changes: `cmd_rename_tab` and the `custom_titles` map go away; `JsonConfig`/`localStorage` persistence is replaced by two `#[persistent]` topics (`TerminalConfig`, `TerminalSessions`) on the bus; a new `menu.rs` builds `terminal_menu(tab_count)` and is re-emitted on every spawn/close/reorder. The async command handler emits topics by pushing them through an `mpsc::Sender<Topic>` drained on the GTK main thread (mirrors the existing `event_tx` pattern).

**Tech Stack:** Rust + Smithay/GTK4/WebKit6 (sola-app framework), tmux as PTY backend, `@arrow-js/core` + xterm.js on the web side.

**Spec:** `docs/specs/2026-04-27-sola-terminal-port-design.md`.

**Worktree:** `.worktrees/terminal-port` on branch `feature/terminal-port`.

---

## Task 1: Add `TerminalConfig` topic to sola-bus

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` (postcard-roundtrip) module in `crates/sola-bus/src/topics.rs` (alongside `mail_config_roundtrips_via_postcard_in_clear`):

```rust
#[test]
fn terminal_config_roundtrips_via_postcard() {
    let cfg = TerminalConfig {
        sidebar_width: 312,
        sidebar_collapsed: true,
    };
    let topic = Topic::TerminalConfig(cfg.clone());
    let msg = topic.to_message();
    let parsed = Topic::parse(&msg).unwrap();
    match parsed {
        Topic::TerminalConfig(back) => {
            assert_eq!(back.sidebar_width, 312);
            assert!(back.sidebar_collapsed);
        }
        other => panic!("expected TerminalConfig, got {other:?}"),
    }
}

#[test]
fn terminal_config_roundtrips_via_toml() {
    let cfg = TerminalConfig {
        sidebar_width: 240,
        sidebar_collapsed: false,
    };
    let topic = Topic::TerminalConfig(cfg);
    let value = topic
        .to_toml_value()
        .expect("persistent payload should serialize to TOML");
    let restored = Topic::from_toml_section(TopicKind::TerminalConfig, value)
        .expect("section should deserialize");
    match restored {
        Topic::TerminalConfig(back) => {
            assert_eq!(back.sidebar_width, 240);
            assert!(!back.sidebar_collapsed);
        }
        other => panic!("expected TerminalConfig, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `cargo test -p sola-bus terminal_config`
Expected: FAIL with `cannot find type TerminalConfig` and similar (the variant doesn't exist yet).

- [ ] **Step 3: Add the struct + Default impl**

Insert near the top of `crates/sola-bus/src/topics.rs`, right after the `MailRuleCondition` block (around line 228, before the `EvaluatePayload` doc comment):

```rust
/// Per-window UI preferences for sola-terminal. Persistent so they
/// survive across terminal restarts and bus restarts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TerminalConfig {
    pub sidebar_width: u32,
    pub sidebar_collapsed: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 220,
            sidebar_collapsed: false,
        }
    }
}
```

- [ ] **Step 4: Add the variant to `define_topics!`**

In the `define_topics! { ... }` block (near `MailConfig`), add a new `#[persistent]` variant. Place it directly below `MailConfig(MailConfig),`:

```rust
    // Terminal UI preferences (sidebar width / collapsed). Persistent
    // so terminal restarts restore the user's layout.
    #[persistent]
    TerminalConfig(TerminalConfig),
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test -p sola-bus terminal_config`
Expected: PASS (both tests).

- [ ] **Step 6: Run all sola-bus tests to confirm no regression**

Run: `cargo test -p sola-bus`
Expected: PASS (every test).

- [ ] **Step 7: Commit**

```bash
git add crates/sola-bus/src/topics.rs
git commit -m "$(cat <<'EOF'
feat(sola-bus): add TerminalConfig persistent topic

Sidebar width + collapsed state for sola-terminal. Persistent (lives
in ~/.config/sola/state.toml) so the UI layout survives both terminal
restarts and bus restarts. Modelled on MailConfig.
EOF
)"
```

---

## Task 2: Add `TerminalSessions` topic to sola-bus

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`

- [ ] **Step 1: Write the failing test**

Append two more tests to the postcard-roundtrip `mod tests` module:

```rust
#[test]
fn terminal_sessions_roundtrip_via_postcard() {
    let sessions = TerminalSessions {
        tabs: vec![
            TerminalTab {
                id: "tab-1".into(),
                tmux_session: "sola-tab-1".into(),
                cwd: Some("/home/joshua".into()),
            },
            TerminalTab {
                id: "tab-2".into(),
                tmux_session: "sola-tab-2".into(),
                cwd: None,
            },
        ],
    };
    let topic = Topic::TerminalSessions(sessions.clone());
    let msg = topic.to_message();
    let parsed = Topic::parse(&msg).unwrap();
    match parsed {
        Topic::TerminalSessions(back) => assert_eq!(back, sessions),
        other => panic!("expected TerminalSessions, got {other:?}"),
    }
}

#[test]
fn terminal_sessions_roundtrip_via_toml() {
    let sessions = TerminalSessions {
        tabs: vec![TerminalTab {
            id: "x".into(),
            tmux_session: "sola-x".into(),
            cwd: Some("/tmp".into()),
        }],
    };
    let topic = Topic::TerminalSessions(sessions.clone());
    let value = topic
        .to_toml_value()
        .expect("persistent payload should serialize to TOML");
    let restored = Topic::from_toml_section(TopicKind::TerminalSessions, value)
        .expect("section should deserialize");
    match restored {
        Topic::TerminalSessions(back) => assert_eq!(back, sessions),
        other => panic!("expected TerminalSessions, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run tests, verify fail**

Run: `cargo test -p sola-bus terminal_sessions`
Expected: FAIL (`TerminalSessions`, `TerminalTab` not found).

- [ ] **Step 3: Add structs**

Insert in `crates/sola-bus/src/topics.rs` directly below the `TerminalConfig` block added in Task 1:

```rust
/// One terminal tab as persisted on the bus. The `tmux_session` is the
/// authoritative identifier for the live PTY; `id` is a stable handle
/// used by the JS side. `cwd` is a hint, refreshed via OSC 7.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalTab {
    pub id: String,
    pub tmux_session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// Live tab list for sola-terminal. Persistent so tabs survive across
/// terminal/bus restarts; reconciled against live tmux on startup.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TerminalSessions {
    pub tabs: Vec<TerminalTab>,
}
```

- [ ] **Step 4: Add the variant to `define_topics!`**

Directly below the `TerminalConfig(TerminalConfig),` variant added in Task 1:

```rust
    // Live tab list for sola-terminal. Persistent so tabs survive
    // terminal-app and bus restarts.
    #[persistent]
    TerminalSessions(TerminalSessions),
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test -p sola-bus terminal_sessions`
Expected: PASS (both tests).

- [ ] **Step 6: Run all sola-bus tests**

Run: `cargo test -p sola-bus`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-bus/src/topics.rs
git commit -m "$(cat <<'EOF'
feat(sola-bus): add TerminalSessions persistent topic

Live tab list (id, tmux_session, optional cwd) for sola-terminal.
Persistent so tabs survive terminal-app and bus restarts.
EOF
)"
```

---

## Task 3: Move `apps/terminal` to `crates/sola-terminal`

**Files:**
- Move: `apps/terminal/` → `crates/sola-terminal/` (entire tree)

- [ ] **Step 1: Move the directory with git history preserved**

Run: `git mv apps/terminal crates/sola-terminal`
Then: `rmdir apps 2>/dev/null || true` (only succeeds if `apps/` is now empty — it should be, since terminal was the only entry).

Expected: `git status` shows the rename of every file under `apps/terminal/` to `crates/sola-terminal/`.

- [ ] **Step 2: Verify Cargo.toml's relative dep paths still resolve**

The Cargo.toml has:
```toml
sola-app = { path = "../../crates/sola-app" }
sola-bus = { path = "../../crates/sola-bus" }
sola-core = { path = "../../crates/sola-core" }
```

After moving from `apps/terminal/` (depth 2) to `crates/sola-terminal/` (depth 2), `../../crates/X` still resolves correctly — verify by reading `crates/sola-terminal/Cargo.toml` and confirming the paths.

But cleaner: rewrite to single-`..` to match sister crates. Edit `crates/sola-terminal/Cargo.toml`:

```toml
[dependencies]
sola-app = { path = "../sola-app" }
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
gtk4 = "0.9"
tokio = { version = "1", features = ["rt-multi-thread", "sync", "io-util", "macros"] }
nix = { version = "0.30", features = ["process", "term", "signal"] }
libc = "0.2"
base64 = "0.22"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
async-trait = "0.1"
```

- [ ] **Step 3: Build (expect failures)**

Run: `cargo make build`
Expected: BUILD FAILS. The two known issues we'll fix in Task 4:
1. `apps/terminal/src/main.rs:86` calls `ctx.emit_sticky(...)`, but current `AppCtx` only exposes `emit(...)`.
2. `apps/terminal/src/state.rs:4` imports `sola_app::config::JsonConfig`; verify whether that path still exists in the current API (it should — `sola-settings` still uses it via `config::JsonConfigIn`).

Capture exactly which compile errors appear; we'll fix them in Task 4.

- [ ] **Step 4: Commit the move**

The build is broken at this point, but committing the rename separately preserves history readability. We'll fix compile in the next task.

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(sola-terminal): move apps/terminal to crates/sola-terminal

Pure file move + Cargo.toml dep path fixup. Build is intentionally
broken at this commit (sola-app API drift between apps/ and crates/);
restored in the next commit.
EOF
)"
```

---

## Task 4: Make `sola-terminal` compile against current `sola-app` API

**Files:**
- Modify: `crates/sola-terminal/src/main.rs`

- [ ] **Step 1: Replace `emit_sticky` with `emit`**

Edit `crates/sola-terminal/src/main.rs` line ~86. Current code:

```rust
ctx.emit_sticky(Topic::SetAppMenu(terminal_menu()));
```

Replace with:

```rust
ctx.emit(Topic::SetAppMenu(terminal_menu()));
```

(Sticky/persistent semantics now live on the topic kind, not the emit method — see `crates/sola-bus/src/client.rs::emit`.)

- [ ] **Step 2: Build**

Run: `cargo make build`
Expected: PASS. If other API drifts surface (e.g. `WindowConfig` field names), fix them by reading the current sister-crate usage (`crates/sola-settings/src/main.rs`) and matching its construction shape exactly.

- [ ] **Step 3: Install and smoke**

Run: `cargo make install sola-terminal`
Expected: builds and installs `/opt/sola/bin/sola-terminal`. Don't run the binary yet — `builtin_apps()` doesn't list it, so the launcher won't show it; we register in Task 10.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-terminal/src/main.rs
git commit -m "$(cat <<'EOF'
fix(sola-terminal): emit Topic::SetAppMenu via ctx.emit

The crate was previously living under apps/ where it wasn't part of
the workspace, so it had silently rotted against the current sola-app
API. ctx.emit_sticky was renamed to ctx.emit; sticky/persistent
semantics now live on the topic kind itself.
EOF
)"
```

---

## Task 5: Drop the rename feature on the Rust side

**Files:**
- Modify: `crates/sola-terminal/src/state.rs`
- Modify: `crates/sola-terminal/src/commands.rs`
- Modify: `crates/sola-terminal/src/main.rs`

- [ ] **Step 1: Remove `custom_title` from `TabEntry` and `RestoredTab` and the `custom_titles` map from `TerminalState`**

Edit `crates/sola-terminal/src/state.rs`. Replace the file with:

```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sola_app::config::JsonConfig;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::pty::PtyManager;

#[derive(Serialize, Deserialize, Clone)]
pub struct TabEntry {
    pub pty_id: String,
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RestoredTab {
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedTerminalState {
    #[serde(default)]
    tabs: Vec<RestoredTab>,
}

impl JsonConfig for PersistedTerminalState {
    const FILE_NAME: &'static str = "terminal-state.json";
}

pub struct TerminalState {
    pub tabs: RwLock<Vec<TabEntry>>,
    pub pty_manager: Mutex<PtyManager>,
}

impl TerminalState {
    pub fn new() -> Self {
        Self {
            tabs: RwLock::new(Vec::new()),
            pty_manager: Mutex::new(PtyManager::new()),
        }
    }

    pub async fn persist_to_disk(&self) {
        let tabs = self.tabs.read().await;

        let live_paths: HashMap<String, String> =
            crate::tmux::list_session_paths().into_iter().collect();

        let serialized: Vec<RestoredTab> = tabs
            .iter()
            .map(|tab| RestoredTab {
                tmux_session: tab.tmux_session.clone(),
                cwd: live_paths
                    .get(&tab.tmux_session)
                    .cloned()
                    .or_else(|| tab.cwd.clone()),
            })
            .collect();

        let state = PersistedTerminalState { tabs: serialized };
        state.save();
        info!("Persisted terminal state ({} tabs)", tabs.len());
    }

    pub fn load_from_disk() -> Vec<RestoredTab> {
        let mut saved = PersistedTerminalState::load().tabs;

        let Some(live) = crate::tmux::list_sessions() else {
            info!(
                "Loaded {} tabs from state (tmux query failed, keeping all)",
                saved.len()
            );
            return saved;
        };

        let live_sessions: std::collections::HashSet<String> = live.into_iter().collect();
        saved.retain(|tab| live_sessions.contains(&tab.tmux_session));

        let known: std::collections::HashSet<&str> =
            saved.iter().map(|t| t.tmux_session.as_str()).collect();
        let orphaned_live: Vec<String> = live_sessions
            .iter()
            .filter(|s| !known.contains(s.as_str()))
            .cloned()
            .collect();
        for session in orphaned_live {
            saved.push(RestoredTab {
                tmux_session: session,
                cwd: None,
            });
        }

        info!("Loaded {} tabs from state", saved.len());
        saved
    }
}
```

(Note: this is intentionally an interim state — `JsonConfig` persistence is still here. We replace it entirely in Task 7.)

- [ ] **Step 2: Remove `cmd_rename_tab` and rename-related code from `commands.rs`**

Edit `crates/sola-terminal/src/commands.rs`:

a) Remove the `"rename_tab"` arm from the `dispatch` match (line 25):

```rust
async fn dispatch(&self, cmd: &str, args: &Value) -> Value {
    match cmd {
        "spawn_pty" => self.cmd_spawn_pty(args).await,
        "write_pty" => self.cmd_write_pty(args).await,
        "resize_pty" => self.cmd_resize_pty(args).await,
        "close_pty" => self.cmd_close_pty(args).await,
        "reconnect_pty" => self.cmd_reconnect_pty(args).await,
        "update_cwd" => self.cmd_update_cwd(args).await,
        "reorder_tabs" => self.cmd_reorder_tabs(args).await,
        _ => json!({ "error": format!("unknown command: {cmd}") }),
    }
}
```

b) Delete the entire `async fn cmd_rename_tab` body (lines 173-199).

c) In `cmd_spawn_pty`, remove the `title` lookup and field. The new body of `cmd_spawn_pty` should be:

```rust
async fn cmd_spawn_pty(&self, args: &Value) -> Value {
    let cols = args.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
    let rows = args.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
    let tmux_session = args
        .get("tmuxSession")
        .and_then(|v| v.as_str())
        .map(String::from);
    let cwd = args.get("cwd").and_then(|v| v.as_str()).map(String::from);

    let pty_id = uuid::Uuid::new_v4().to_string();
    let (pty_event_tx, pty_event_rx) = mpsc::unbounded_channel::<PtyEvent>();

    let tmux_session_name = {
        let mut mgr = self.state.pty_manager.lock().await;
        match mgr.spawn_pty(
            pty_id.clone(),
            cols,
            rows,
            tmux_session,
            cwd.clone(),
            pty_event_tx,
        ) {
            Ok(name) => name,
            Err(e) => return json!({ "error": e }),
        }
    };

    {
        let mut tabs = self.state.tabs.write().await;
        tabs.push(TabEntry {
            pty_id: pty_id.clone(),
            tmux_session: tmux_session_name.clone(),
            cwd,
        });
    }

    let tx = self.event_tx.clone();
    tokio::spawn(forward_pty_events(pty_id.clone(), pty_event_rx, tx));

    self.state.persist_to_disk().await;

    json!({
        "pty_id": pty_id,
        "tmux_session": tmux_session_name,
    })
}
```

(Note: still calls `persist_to_disk` and still uses the JSON file — we replace persistence in Task 7.)

- [ ] **Step 3: Remove the `custom_titles` priming block from `main.rs`**

Edit `crates/sola-terminal/src/main.rs::SolaApp::new`. Delete this block (lines ~46-56):

```rust
        let restored_tabs = state::TerminalState::load_from_disk();
        let restored_json = serde_json::to_string(&restored_tabs).unwrap_or_default();

        let terminal_state = Arc::new(state::TerminalState::new());
        {
            let mut titles = terminal_state.custom_titles.try_write().unwrap();
            for tab in &restored_tabs {
                if let Some(ref title) = tab.custom_title {
                    titles.insert(tab.tmux_session.clone(), title.clone());
                }
            }
        }
```

Replace with:

```rust
        let restored_tabs = state::TerminalState::load_from_disk();
        let restored_json = serde_json::to_string(&restored_tabs).unwrap_or_default();

        let terminal_state = Arc::new(state::TerminalState::new());
```

- [ ] **Step 4: Build**

Run: `cargo make build`
Expected: PASS. If anything still references `custom_title` or `custom_titles`, the compiler points to it. Delete those references.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-terminal/
git commit -m "$(cat <<'EOF'
refactor(sola-terminal): drop custom tab name feature

Remove the rename_tab command, the custom_titles map on TerminalState,
the custom_title field on TabEntry/RestoredTab, and the priming code
in SolaApp::new. Tab labels now derive purely from cwd basename and
tmux session id. Web side rename UX is removed in the next commit.
EOF
)"
```

---

## Task 6: Drop the rename feature on the web side

**Files:**
- Modify: `crates/sola-terminal/web/src/components/sidebar.ts`
- Modify: `crates/sola-terminal/web/src/app.ts`

- [ ] **Step 1: Strip rename code from `sidebar.ts`**

Make the following edits to `crates/sola-terminal/web/src/components/sidebar.ts`:

(a) In `interface TabItem`, remove the `customTitle?: string` line.

```ts
export interface TabItem {
  id: string;
  title: string;
  cwd: string;
}
```

(b) In `interface SidebarConfig`, remove the `onRename: ...` line.

(c) In `function displayTitle`, remove the `tab.customTitle ||` prefix:

```ts
function displayTitle(tab: TabItem): string {
  return cwdBasename(tab.cwd) || 'shell';
}
```

(d) In `createSidebar`'s `ui` reactive block, remove `renamingTabId` and `renameValue`. Update the leading comment:

```ts
// Local UI state for drag interactions
const ui = reactive({
  dragTabIndex: null as number | null,
  dropTargetIndex: null as number | null,
  isDragging: false,
  resizing: false,
});
```

(e) Delete the entire `// --- Tab rename ---` block (functions `startRename`, `commitRename`, `cancelRename`).

(f) Delete the rename-input focus `watch(...)` block (the one that focuses `.tab-rename-input`).

(g) In `tabContent`, delete the `if (ui.renamingTabId === tab.id) { ... }` branch entirely. After edit, `tabContent` is:

```ts
function tabContent(tab: TabItem) {
  if (config.collapsed()) return html``;

  return html`
    <div class="tab-info">
      <span class="tab-title">${() => displayTitle(tab)}</span>
    </div>
    <button class="tab-close" aria-label="Close tab"
      @click="${(e: MouseEvent) => { e.stopPropagation(); config.onClose(tab.id); }}"
      @mousedown="${(e: MouseEvent) => { e.preventDefault(); e.stopPropagation(); }}"
    >x</button>
  `;
}
```

(h) In the template, remove the `@dblclick="${() => { if (!config.collapsed()) startRename(tab); }}"` attribute on the per-tab `<div>`.

Verify:

```bash
grep -ni "rename\|customTitle" crates/sola-terminal/web/src/components/sidebar.ts
```

Expected: no matches.

- [ ] **Step 2: Strip rename code from `app.ts`**

In `crates/sola-terminal/web/src/app.ts`:

(a) In `interface RestoredTab`, remove the `customTitle?: string` line.

(b) In `interface Tab extends TabItem`, do not need any change (TabItem no longer has customTitle).

(c) In `createTab`, remove the `customTitle?: string` parameter and the `customTitle` field on the constructed tab object:

```ts
function createTab(tmuxSession?: string, cwd?: string): string {
  const tabId = `tab-${nextTabNum++}`;
  const tab: Tab = { id: tabId, title: '', cwd: cwd || '', tmuxSession };
  // ...rest unchanged...
}
```

(d) Update the two `createTab(undefined, undefined, activeCwd ...)` call sites in the `on('new_tab', ...)` listener and the `onCreate` sidebar option, to drop the second argument:

```ts
createTab(undefined, activeCwd || undefined);
```

(e) Update the restored-tabs loop:

```ts
for (const rt of restoredTabs) {
  createTab(rt.tmuxSession, rt.cwd);
}
```

(f) Delete the `function handleRename(...)` (lines 134-141) entirely.

(g) Remove the `onRename: handleRename,` line from the `createSidebar({ ... })` config object.

Verify:

```bash
grep -ni "rename\|customTitle" crates/sola-terminal/web/src/
```

Expected: no matches.

- [ ] **Step 3: Build**

Run: `cargo make build`
Expected: PASS. If TypeScript stripping or include_str! complains about a missing file, restore it; we should not be deleting files in this task.

- [ ] **Step 4: Install and smoke**

Run: `cargo make install sola-terminal`
Expected: install succeeds.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-terminal/web/
git commit -m "$(cat <<'EOF'
refactor(sola-terminal): drop rename UI on the web side

Remove the rename input, its keyboard/blur handlers, the right-click
rename entry, and the rename invoke from app.ts. Tab labels are now
always derived: cwd basename if known, otherwise the tmux session id.
EOF
)"
```

---

## Task 7: Replace `JsonConfig` persistence with bus state on the Rust side

**Files:**
- Modify: `crates/sola-terminal/src/state.rs`
- Modify: `crates/sola-terminal/src/commands.rs`
- Modify: `crates/sola-terminal/src/main.rs`

- [ ] **Step 1: Strip `JsonConfig` and persistence from `state.rs`**

Replace `crates/sola-terminal/src/state.rs` entirely with:

```rust
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::pty::PtyManager;

#[derive(Serialize, Deserialize, Clone)]
pub struct TabEntry {
    pub pty_id: String,
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

pub struct TerminalState {
    pub tabs: RwLock<Vec<TabEntry>>,
    pub pty_manager: Mutex<PtyManager>,
}

impl TerminalState {
    pub fn new() -> Self {
        Self {
            tabs: RwLock::new(Vec::new()),
            pty_manager: Mutex::new(PtyManager::new()),
        }
    }
}
```

This deletes `RestoredTab`, `PersistedTerminalState`, `JsonConfig`, `persist_to_disk`, `load_from_disk`. The bus replays the persisted state into `main.rs`'s handler instead, and tmux reconciliation moves into `main.rs` (Step 4 below).

- [ ] **Step 2: Add an `emit_tx` channel to `TerminalHandler`, replace `persist_to_disk` calls with topic emits, add `set_sidebar`**

Replace `crates/sola-terminal/src/commands.rs` with:

```rust
use std::sync::Arc;

use base64::Engine;
use serde_json::{Value, json};
use sola_bus::topics::{TerminalSessions, TerminalTab, Topic};
use tokio::sync::mpsc;

use crate::pty::PtyEvent;
use crate::state::{TabEntry, TerminalState};

/// Terminal command handler implementing the sola-app AppHandler trait.
/// Emits bus topics by sending them through `emit_tx`; the main thread
/// drains and calls `ctx.emit`.
pub struct TerminalHandler {
    pub state: Arc<TerminalState>,
    pub event_tx: std::sync::mpsc::Sender<String>,
    pub emit_tx: std::sync::mpsc::Sender<Topic>,
}

#[async_trait::async_trait]
impl sola_app::AppHandler for TerminalHandler {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value {
        match cmd {
            "spawn_pty" => self.cmd_spawn_pty(args).await,
            "write_pty" => self.cmd_write_pty(args).await,
            "resize_pty" => self.cmd_resize_pty(args).await,
            "close_pty" => self.cmd_close_pty(args).await,
            "reconnect_pty" => self.cmd_reconnect_pty(args).await,
            "update_cwd" => self.cmd_update_cwd(args).await,
            "reorder_tabs" => self.cmd_reorder_tabs(args).await,
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        }
    }
}

impl TerminalHandler {
    async fn cmd_spawn_pty(&self, args: &Value) -> Value {
        let cols = args.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
        let rows = args.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
        let tmux_session = args
            .get("tmuxSession")
            .and_then(|v| v.as_str())
            .map(String::from);
        let cwd = args.get("cwd").and_then(|v| v.as_str()).map(String::from);

        let pty_id = uuid::Uuid::new_v4().to_string();
        let (pty_event_tx, pty_event_rx) = mpsc::unbounded_channel::<PtyEvent>();

        let tmux_session_name = {
            let mut mgr = self.state.pty_manager.lock().await;
            match mgr.spawn_pty(
                pty_id.clone(),
                cols,
                rows,
                tmux_session,
                cwd.clone(),
                pty_event_tx,
            ) {
                Ok(name) => name,
                Err(e) => return json!({ "error": e }),
            }
        };

        {
            let mut tabs = self.state.tabs.write().await;
            tabs.push(TabEntry {
                pty_id: pty_id.clone(),
                tmux_session: tmux_session_name.clone(),
                cwd,
            });
        }

        let tx = self.event_tx.clone();
        tokio::spawn(forward_pty_events(pty_id.clone(), pty_event_rx, tx));

        self.emit_sessions().await;

        json!({
            "pty_id": pty_id,
            "tmux_session": tmux_session_name,
        })
    }

    async fn cmd_write_pty(&self, args: &Value) -> Value {
        let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({ "error": "missing pty_id" }),
        };
        let data_b64 = match args.get("data").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => return json!({ "error": "missing data" }),
        };
        let data = match base64::engine::general_purpose::STANDARD.decode(data_b64) {
            Ok(d) => d,
            Err(e) => return json!({ "error": format!("base64 decode failed: {e}") }),
        };

        let mgr = self.state.pty_manager.lock().await;
        match mgr.write_pty(pty_id, &data) {
            Ok(()) => json!("ok"),
            Err(e) => json!({ "error": e }),
        }
    }

    async fn cmd_resize_pty(&self, args: &Value) -> Value {
        let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({ "error": "missing pty_id" }),
        };
        let cols = match args.get("cols").and_then(|v| v.as_u64()) {
            Some(c) => c as u16,
            None => return json!({ "error": "missing cols" }),
        };
        let rows = match args.get("rows").and_then(|v| v.as_u64()) {
            Some(r) => r as u16,
            None => return json!({ "error": "missing rows" }),
        };

        let mgr = self.state.pty_manager.lock().await;
        match mgr.resize_pty(pty_id, cols, rows) {
            Ok(()) => {}
            Err(e) => return json!({ "error": e }),
        }
        match mgr.sigwinch_pty(pty_id) {
            Ok(()) => json!("ok"),
            Err(e) => json!({ "error": e }),
        }
    }

    async fn cmd_close_pty(&self, args: &Value) -> Value {
        let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({ "error": "missing pty_id" }),
        };

        {
            let mut mgr = self.state.pty_manager.lock().await;
            if let Err(e) = mgr.close_pty(pty_id) {
                return json!({ "error": e });
            }
        }

        {
            let mut tabs = self.state.tabs.write().await;
            tabs.retain(|t| t.pty_id != pty_id);
        }

        self.emit_sessions().await;

        json!("ok")
    }

    async fn cmd_reconnect_pty(&self, args: &Value) -> Value {
        let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({ "error": "missing pty_id" }),
        };

        let mgr = self.state.pty_manager.lock().await;
        match mgr.reconnect_pty(pty_id) {
            Ok(scrollback) => json!({ "scrollback": scrollback }),
            Err(e) => json!({ "error": e }),
        }
    }

    async fn cmd_update_cwd(&self, args: &Value) -> Value {
        let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({ "error": "missing pty_id" }),
        };
        let cwd = match args.get("cwd").and_then(|v| v.as_str()) {
            Some(c) => c.to_string(),
            None => return json!({ "error": "missing cwd" }),
        };

        let changed = {
            let mut tabs = self.state.tabs.write().await;
            if let Some(tab) = tabs.iter_mut().find(|t| t.pty_id == pty_id) {
                if tab.cwd.as_deref() == Some(cwd.as_str()) {
                    false
                } else {
                    tab.cwd = Some(cwd);
                    true
                }
            } else {
                false
            }
        };

        if changed {
            self.emit_sessions().await;
        }

        json!("ok")
    }

    async fn cmd_reorder_tabs(&self, args: &Value) -> Value {
        let pty_ids: Vec<String> = match args.get("pty_ids").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            None => return json!({ "error": "missing pty_ids" }),
        };

        {
            let mut tabs = self.state.tabs.write().await;
            let mut reordered = Vec::with_capacity(pty_ids.len());
            for id in &pty_ids {
                if let Some(tab) = tabs.iter().find(|t| &t.pty_id == id).cloned() {
                    reordered.push(tab);
                }
            }
            *tabs = reordered;
        }

        self.emit_sessions().await;

        json!("ok")
    }

    async fn emit_sessions(&self) {
        let tabs = self.state.tabs.read().await;
        let payload = TerminalSessions {
            tabs: tabs
                .iter()
                .map(|t| TerminalTab {
                    id: t.pty_id.clone(),
                    tmux_session: t.tmux_session.clone(),
                    cwd: t.cwd.clone(),
                })
                .collect(),
        };
        if self.emit_tx.send(Topic::TerminalSessions(payload)).is_err() {
            tracing::warn!("emit channel closed; topic dropped");
        }
    }
}

async fn forward_pty_events(
    _pty_id: String,
    mut event_rx: mpsc::UnboundedReceiver<PtyEvent>,
    tx: std::sync::mpsc::Sender<String>,
) {
    let b64 = base64::engine::general_purpose::STANDARD;

    while let Some(event) = event_rx.recv().await {
        let msg = match event {
            PtyEvent::Data { pty_id, data } => json!({
                "event": "pty:data",
                "pty_id": pty_id,
                "data": b64.encode(&data),
            }),
            PtyEvent::Scrollback { pty_id, data } => json!({
                "event": "pty:scrollback",
                "pty_id": pty_id,
                "data": b64.encode(&data),
            }),
            PtyEvent::Exit { pty_id } => {
                let msg = json!({
                    "event": "pty:exit",
                    "pty_id": pty_id,
                });
                let _ = tx.send(msg.to_string());
                break;
            }
        };
        if tx.send(msg.to_string()).is_err() {
            break;
        }
    }
}
```

Note: `cmd_update_cwd` now no-ops the emit if the cwd didn't actually change (avoids feedback loops where the bus echoes our emit and we'd re-emit again — see Step 3 about the handler).

- [ ] **Step 3: Wire up the emit channel and bus handlers in `main.rs`**

Replace `crates/sola-terminal/src/main.rs` with:

```rust
use std::sync::Arc;

use serde_json::{Value, json};
use sola_app::{
    AppCtx, AsyncDispatcher, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, OpenUrlRequest, TerminalConfig,
    TerminalSessions, Topic, TopicKind,
};
use sola_core::KeyCode;

mod commands;
mod pty;
mod state;
mod tmux;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/terminal-pane.ts" => (include_str!("../web/src/terminal-pane.ts"), TypeScript),
    "/src/components/sidebar.ts" => (include_str!("../web/src/components/sidebar.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
    "/vendor/xterm.mjs" => (include_str!("../web/vendor/xterm.mjs"), JavaScript),
    "/vendor/xterm.css" => (include_str!("../web/vendor/xterm.css"), Css),
    "/vendor/addon-fit.mjs" => (include_str!("../web/vendor/addon-fit.mjs"), JavaScript),
    "/vendor/addon-web-links.mjs" => (include_str!("../web/vendor/addon-web-links.mjs"), JavaScript),
};

struct TerminalApp {
    main_window: WindowHandle,
    dispatcher: AsyncDispatcher,
    state: Arc<state::TerminalState>,
    config: TerminalConfig,
    sessions_synced: bool,
}

impl SolaApp for TerminalApp {
    const APP_ID: &'static str = "sola-terminal";

    fn new(ctx: &mut AppCtx) -> Self {
        tmux::cleanup_stale_socket();
        tmux::kill_orphaned_clients();
        tmux::reload_config();

        let terminal_state = Arc::new(state::TerminalState::new());

        // Initial JS state: empty tabs + default config. The bus replays
        // the persisted TerminalConfig and TerminalSessions into our
        // handlers a few ms after subscription, and we push the real
        // state to JS at that point.
        let initial_state =
            serde_json::to_string(&state_payload(&[], &TerminalConfig::default())).unwrap_or_default();

        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (1920, 1080),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: Some(initial_state),
            zoned: true,
            keyboard_target: true,
        });

        // Bridge dispatcher → JS for PTY events.
        let (event_tx, event_rx) = std::sync::mpsc::channel::<String>();
        let mw_for_events = main_window.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            while let Ok(msg) = event_rx.try_recv() {
                mw_for_events.send_raw_json_to_js(&msg);
            }
            gtk4::glib::ControlFlow::Continue
        });

        // Bridge dispatcher → bus for topic emits. AppCtx is GTK-thread-bound
        // (Rc<RefCell<BusClient>>), so we can't share it with the tokio
        // runtime. Instead, the handler sends Topics through this channel
        // and the GTK main loop drains them via ctx.emit.
        let (emit_tx, emit_rx) = std::sync::mpsc::channel::<Topic>();
        let ctx_proxy = ctx.bus_proxy();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            while let Ok(topic) = emit_rx.try_recv() {
                ctx_proxy.emit(topic);
            }
            gtk4::glib::ControlFlow::Continue
        });

        let dispatcher = AsyncDispatcher::spawn(commands::TerminalHandler {
            state: terminal_state.clone(),
            event_tx,
            emit_tx,
        });

        ctx.emit(Topic::SetAppMenu(menu::terminal_menu(0)));
        tracing::info!("registered terminal menu");

        Self {
            main_window,
            dispatcher,
            state: terminal_state,
            config: TerminalConfig::default(),
            sessions_synced: false,
        }
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        _source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        match cmd {
            "open_url" => {
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
                if url.is_empty() {
                    tracing::warn!("open_url command with empty url");
                    return;
                }
                let activate = args
                    .get("activate")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                ctx.emit(Topic::OpenUrl(OpenUrlRequest {
                    url: url.to_string(),
                    activate,
                }));
                if let Some(id) = id {
                    self.main_window
                        .send_to_js(&json!({ "id": id, "result": "ok" }));
                }
            }
            "set_sidebar" => {
                let width = args
                    .get("width")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(self.config.sidebar_width);
                let collapsed = args
                    .get("collapsed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(self.config.sidebar_collapsed);
                if width != self.config.sidebar_width
                    || collapsed != self.config.sidebar_collapsed
                {
                    self.config.sidebar_width = width;
                    self.config.sidebar_collapsed = collapsed;
                    ctx.emit(Topic::TerminalConfig(self.config.clone()));
                }
                if let Some(id) = id {
                    self.main_window
                        .send_to_js(&json!({ "id": id, "result": "ok" }));
                }
            }
            _ => {
                let source = self.main_window.clone();
                let args = args.clone();
                self.dispatcher
                    .dispatch(cmd.to_string(), args, move |result| {
                        if let Some(id) = id {
                            source.send_to_js(&json!({ "id": id, "result": result }));
                        }
                    });
            }
        }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
        bus.on(TopicKind::TerminalConfig, Self::on_terminal_config);
        bus.on(TopicKind::TerminalSessions, Self::on_terminal_sessions);
    }
}

impl TerminalApp {
    fn on_menu_action(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic else {
            return;
        };
        if app_id != Self::APP_ID {
            return;
        }
        match action_id.as_str() {
            "new_tab" => {
                self.main_window
                    .send_to_js(&json!({"event": "new_tab"}));
            }
            id if id.starts_with("select_tab_") => {
                if let Ok(index) = id.strip_prefix("select_tab_").unwrap().parse::<usize>() {
                    self.main_window
                        .send_to_js(&json!({"event": "select_tab", "index": index}));
                }
            }
            "quit" => std::process::exit(0),
            _ => {
                tracing::debug!(action_id, "unknown menu action");
            }
        }
    }

    fn on_terminal_config(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::TerminalConfig(cfg) = topic else { return };
        self.config = cfg.clone();
        self.push_state_to_js();
    }

    fn on_terminal_sessions(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::TerminalSessions(sessions) = topic else { return };

        // First replay: reconcile against live tmux. Drop tabs whose tmux
        // session is gone; preserve ordering and cwds for survivors. Re-emit
        // the cleaned set (only on this first replay) so the disk record
        // converges with reality.
        let reconciled: Vec<TerminalTab> = if !self.sessions_synced {
            self.sessions_synced = true;
            let live: std::collections::HashSet<String> = tmux::list_sessions()
                .map(|v| v.into_iter().collect())
                .unwrap_or_default();
            let kept: Vec<TerminalTab> = sessions
                .tabs
                .iter()
                .filter(|t| live.is_empty() || live.contains(&t.tmux_session))
                .cloned()
                .collect();
            if kept.len() != sessions.tabs.len() {
                ctx.emit(Topic::TerminalSessions(TerminalSessions {
                    tabs: kept.clone(),
                }));
            }
            kept
        } else {
            sessions.tabs.clone()
        };

        // Sync to in-memory TerminalState mirror.
        let entries: Vec<state::TabEntry> = reconciled
            .iter()
            .map(|t| state::TabEntry {
                pty_id: t.id.clone(),
                tmux_session: t.tmux_session.clone(),
                cwd: t.cwd.clone(),
            })
            .collect();
        let state = self.state.clone();
        gtk4::glib::MainContext::default().spawn_local(async move {
            *state.tabs.write().await = entries;
        });

        // Re-emit menu reflecting the reconciled count.
        ctx.emit(Topic::SetAppMenu(menu::terminal_menu(reconciled.len())));

        // Push fresh state to JS.
        let payload = state_payload(&reconciled, &self.config);
        self.main_window
            .send_to_js(&json!({ "event": "state", "state": payload }));
    }

    fn push_state_to_js(&self) {
        let tabs: Vec<TerminalTab> = futures_lite::future::block_on(async {
            self.state
                .tabs
                .read()
                .await
                .iter()
                .map(|t| TerminalTab {
                    id: t.pty_id.clone(),
                    tmux_session: t.tmux_session.clone(),
                    cwd: t.cwd.clone(),
                })
                .collect()
        });
        let payload = state_payload(&tabs, &self.config);
        self.main_window
            .send_to_js(&json!({ "event": "state", "state": payload }));
    }
}

fn state_payload(tabs: &[TerminalTab], config: &TerminalConfig) -> Value {
    json!({
        "tabs": tabs,
        "config": {
            "sidebar_width": config.sidebar_width,
            "sidebar_collapsed": config.sidebar_collapsed,
        },
    })
}

mod menu {
    use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
    use sola_core::KeyCode;

    use crate::TerminalApp;

    /// Build the terminal app menu reflecting the actual tab count.
    /// Tabs 1-9 get Cmd+N shortcuts; tabs 10+ have no shortcut.
    pub fn terminal_menu(tab_count: usize) -> AppMenuPayload {
        AppMenuPayload {
            app_id: TerminalApp::APP_ID.into(),
            menus: vec![
                MenuDefinition {
                    label: "Terminal".into(),
                    items: vec![
                        MenuItem::Action {
                            id: "about".into(),
                            label: "About Terminal".into(),
                            shortcut: None,
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Divider,
                        MenuItem::Action {
                            id: "quit".into(),
                            label: "Quit Terminal".into(),
                            shortcut: Some(KeyCode::Q.meta()),
                            disabled: false,
                            checked: false,
                        },
                    ],
                },
                MenuDefinition {
                    label: "Shell".into(),
                    items: vec![MenuItem::Action {
                        id: "new_tab".into(),
                        label: "New Tab".into(),
                        shortcut: Some(KeyCode::T.meta()),
                        disabled: false,
                        checked: false,
                    }],
                },
                MenuDefinition {
                    label: "Tabs".into(),
                    items: (0..tab_count).map(|i| tab_item(i)).collect(),
                },
            ],
        }
    }

    fn tab_item(index: usize) -> MenuItem {
        MenuItem::Action {
            id: format!("select_tab_{index}"),
            label: format!("Tab {}", index + 1),
            shortcut: tab_shortcut(index),
            disabled: false,
            checked: false,
        }
    }

    fn tab_shortcut(index: usize) -> Option<sola_core::KeyChord> {
        let key = match index {
            0 => KeyCode::KEY_1,
            1 => KeyCode::KEY_2,
            2 => KeyCode::KEY_3,
            3 => KeyCode::KEY_4,
            4 => KeyCode::KEY_5,
            5 => KeyCode::KEY_6,
            6 => KeyCode::KEY_7,
            7 => KeyCode::KEY_8,
            8 => KeyCode::KEY_9,
            _ => return None,
        };
        Some(key.meta())
    }
}

fn main() {
    sola_app::run::<TerminalApp>();
}
```

Note: this collapses Task 9's `menu.rs` extraction into `main.rs` as an inline `mod menu`. Task 9 will move it to its own file.

- [ ] **Step 4: Verify `AppCtx::bus_proxy` exists**

Run: `grep -n "bus_proxy\b" crates/sola-app/src/ctx.rs`

If `bus_proxy` does not exist, we need a different way to emit topics from the GTK timeout closure. Two options:

**Option A** (preferred): add a `bus_proxy()` accessor to `AppCtx` that returns a `Clone`-able handle wrapping the `Rc<RefCell<BusClient>>`. The handle's `emit()` is callable from a `'static` glib closure as long as it's not Send. Add to `crates/sola-app/src/ctx.rs`:

```rust
/// Cheap clone of the bus client, usable from any GTK-thread closure
/// (not Send — runs on the GTK main loop).
#[derive(Clone)]
pub struct BusProxy {
    bus: Rc<RefCell<BusClient>>,
}

impl BusProxy {
    pub fn emit(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit(topic);
    }
}

impl AppCtx {
    pub fn bus_proxy(&self) -> BusProxy {
        BusProxy { bus: self.bus.clone() }
    }
}
```

Add `BusProxy` to the public re-exports at the top of `crates/sola-app/src/lib.rs` if needed.

**Option B**: emit from inside `AsyncDispatcher::spawn`'s reply path. The reply callback runs on the main thread and has captured the dispatcher's pending state — but it doesn't have `ctx`. Plumbing `ctx` into the reply is a bigger change. **Use Option A.**

- [ ] **Step 5: Build**

Run: `cargo make build`
Expected: PASS. Common compile errors to fix:
- `futures_lite` not in `Cargo.toml` — replace `futures_lite::future::block_on` with `tokio::runtime::Handle::current().block_on(...)` if a tokio runtime is reachable, OR keep the state-mirror sync via the gtk4 main loop (use `MainContext::default().spawn_local`) — see how sola-settings synchronously snapshots state for `current_state`. The simplest safe path: do the state read *inside* the bus handler before we mutate `self.config`, by reading `self.state.tabs.try_read()` (returns `Option`) and falling back to "skip the JS push, the next event will refresh".

Replace the `push_state_to_js` body with:

```rust
fn push_state_to_js(&self) {
    let Ok(tabs) = self.state.tabs.try_read() else { return };
    let mapped: Vec<TerminalTab> = tabs
        .iter()
        .map(|t| TerminalTab {
            id: t.pty_id.clone(),
            tmux_session: t.tmux_session.clone(),
            cwd: t.cwd.clone(),
        })
        .collect();
    drop(tabs);
    let payload = state_payload(&mapped, &self.config);
    self.main_window
        .send_to_js(&json!({ "event": "state", "state": payload }));
}
```

This avoids the futures_lite dep entirely.

- [ ] **Step 6: Install and quick smoke**

Run:

```bash
cargo make install sola-terminal
ls -la /opt/sola/bin/sola-terminal
```

Expected: install succeeds, binary present.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-terminal/ crates/sola-app/src/
git commit -m "$(cat <<'EOF'
refactor(sola-terminal): persist via bus topics instead of state.json

Replace the JsonConfig-backed terminal-state.json with two persistent
bus topics: TerminalConfig (sidebar prefs) and TerminalSessions
(tab list). The async command handler emits topics through an
mpsc::Sender<Topic> drained on the GTK main thread via a new
AppCtx::bus_proxy() accessor.

The first TerminalSessions replay reconciles against live tmux and
re-emits the cleaned list (and a fresh SetAppMenu) so the persisted
state converges with reality on restart.
EOF
)"
```

---

## Task 8: Replace `localStorage` persistence with bus state on the web side

**Files:**
- Modify: `crates/sola-terminal/web/index.html`
- Modify: `crates/sola-terminal/web/src/app.ts`

- [ ] **Step 1: Update `index.html` to expose the new state shape**

Edit `crates/sola-terminal/web/index.html`. Find the existing line:

```html
<script>window.RESTORED_TABS = __RESTORED_STATE__;</script>
```

Replace with:

```html
<script>window.RESTORED_STATE = __RESTORED_STATE__;</script>
```

(`__RESTORED_STATE__` is a string-replacement placeholder filled by `sola-app` from `WindowConfig::initial_state` — see `crates/sola-app/src/ctx.rs:52-53`. The new shape is `{ tabs: [...], config: {...} }` instead of a bare tab array.)

- [ ] **Step 2: Drop the `persist()` import + call in `app.ts`**

In `crates/sola-terminal/web/src/app.ts`, change the import line:

```ts
import { createStore } from '@sola/store';
```

(Drop both `persist` and `save`.)

Delete line 27:

```ts
persist(state, 'terminal-sidebar', ['sidebarCollapsed', 'sidebarWidth']);
```

In `handleToggleCollapse` (lines 143-149), replace `save(...)` with `invoke('set_sidebar', ...)`:

```ts
function handleToggleCollapse() {
  state.sidebarCollapsed = !state.sidebarCollapsed;
  invoke('set_sidebar', {
    width: state.sidebarWidth,
    collapsed: state.sidebarCollapsed,
  });
  requestAnimationFrame(() => {
    if (state.activeTabId) panes.get(state.activeTabId)?.refit();
  });
}
```

In `handleSidebarResize` (lines 151-157), do the same — but only emit on drag-end, not every mouse move. The sidebar emits `onResize` continuously during drag; persisting on every mousemove would spam the bus. Refactor to:

```ts
function handleSidebarResize(width: number) {
  state.sidebarWidth = width;
  requestAnimationFrame(() => {
    if (state.activeTabId) panes.get(state.activeTabId)?.refit();
  });
}

function handleSidebarResizeEnd() {
  invoke('set_sidebar', {
    width: state.sidebarWidth,
    collapsed: state.sidebarCollapsed,
  });
}
```

Then add `onResizeEnd: () => void` to `SidebarConfig` in `sidebar.ts` and call it in `onMouseUp` (after `ui.resizing = false`):

```ts
function onMouseUp() {
  const wasResizing = ui.resizing;
  if (ui.resizing) ui.resizing = false;
  if (wasResizing) config.onResizeEnd();
  // ...rest unchanged
}
```

Wire it up in `createSidebar({ ..., onResize, onResizeEnd: handleSidebarResizeEnd, ... })`.

- [ ] **Step 3: Initialize store from new `RESTORED_STATE` shape and subscribe to bus state replays**

In `app.ts`, replace the `RestoredTab` interface and the restored-tabs init block. After the `Tab extends TabItem` interface, add:

```ts
interface TabSnapshot {
  id: string;
  tmux_session: string;
  cwd?: string;
}

interface RestoredState {
  tabs: TabSnapshot[];
  config: {
    sidebar_width: number;
    sidebar_collapsed: boolean;
  };
}

const initial: RestoredState = (window as any).RESTORED_STATE ?? {
  tabs: [],
  config: { sidebar_width: 220, sidebar_collapsed: false },
};
```

Update the store initializer to seed from `initial.config`:

```ts
const state = createStore({
  tabs: [] as Tab[],
  activeTabId: null as string | null,
  sidebarCollapsed: initial.config.sidebar_collapsed,
  sidebarWidth: initial.config.sidebar_width,
});
```

Replace the `// Restore tabs` block (lines 192-201) with:

```ts
  // Restore tabs from the initial snapshot.
  if (initial.tabs.length > 0) {
    for (const t of initial.tabs) {
      createTab(t.tmux_session, t.cwd);
    }
  } else {
    createTab();
  }
```

(`createTab(tmuxSession, cwd)` matches the simplified signature from Task 6 step 2c.)

Add a bus subscription for state replays. Insert near the other `on(...)` subscriptions in `createApp`:

```ts
  on('state', (payload: { tabs: TabSnapshot[]; config: { sidebar_width: number; sidebar_collapsed: boolean } }) => {
    // Sidebar prefs always re-sync.
    state.sidebarWidth = payload.config.sidebar_width;
    state.sidebarCollapsed = payload.config.sidebar_collapsed;
    requestAnimationFrame(() => {
      if (state.activeTabId) panes.get(state.activeTabId)?.refit();
    });

    // Tabs: only act if the bus's set differs from ours by id.
    // This handles the post-reconcile re-emit (Rust dropped a dead tab)
    // and any future case where another agent edits the topic. We do
    // NOT auto-spawn TerminalPanes for unknown tabs — those came from
    // a prior session and need user-initiated reconnect anyway.
    const knownIds = new Set(state.tabs.map(t => t.id));
    const incomingIds = new Set(payload.tabs.map(t => t.id));
    if (state.tabs.some(t => !incomingIds.has(t.id))) {
      // Rust dropped a tab; remove from UI.
      for (const t of [...state.tabs]) {
        if (!incomingIds.has(t.id)) removeTab(t.id);
      }
    }
  });
```

- [ ] **Step 4: Build**

Run: `cargo make build`
Expected: PASS. The Rust side should not be touched here; only the embedded TS / index.html assets change.

- [ ] **Step 5: Install and quick visual smoke**

Run: `cargo make install sola-terminal`
Expected: install succeeds.

(Full smoke is in Task 11. We don't run the binary yet because launcher registration is in Task 10.)

- [ ] **Step 6: Commit**

```bash
git add crates/sola-terminal/web/
git commit -m "$(cat <<'EOF'
refactor(sola-terminal): persist sidebar prefs via the bus, not localStorage

Drop the @sola/store persist() call and read sidebar width/collapsed
from window.RESTORED_STATE (renamed from RESTORED_TABS to match the
new {tabs, config} shape). Mutations route through
invoke('set_sidebar', ...) which the Rust side mirrors into
Topic::TerminalConfig. State replays from Rust ('state' events)
re-sync the store. Sidebar resize is debounced to drag-end via a new
onResizeEnd callback so we don't spam the bus during drag.
EOF
)"
```

---

## Task 9: Extract `menu.rs` from the inline module

**Files:**
- Create: `crates/sola-terminal/src/menu.rs`
- Modify: `crates/sola-terminal/src/main.rs`

- [ ] **Step 1: Create `crates/sola-terminal/src/menu.rs`**

Move the entire `mod menu { ... }` block out of `main.rs` into its own file. Content:

```rust
use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::{KeyChord, KeyCode};

use crate::TerminalApp;
use sola_app::SolaApp;

/// Build the terminal app menu reflecting the actual tab count.
/// Tabs 1-9 get Cmd+N shortcuts; tabs 10+ have no shortcut.
pub fn terminal_menu(tab_count: usize) -> AppMenuPayload {
    AppMenuPayload {
        app_id: TerminalApp::APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Terminal".into(),
                items: vec![
                    MenuItem::Action {
                        id: "about".into(),
                        label: "About Terminal".into(),
                        shortcut: None,
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                    MenuItem::Action {
                        id: "quit".into(),
                        label: "Quit Terminal".into(),
                        shortcut: Some(KeyCode::Q.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
            MenuDefinition {
                label: "Shell".into(),
                items: vec![MenuItem::Action {
                    id: "new_tab".into(),
                    label: "New Tab".into(),
                    shortcut: Some(KeyCode::T.meta()),
                    disabled: false,
                    checked: false,
                }],
            },
            MenuDefinition {
                label: "Tabs".into(),
                items: (0..tab_count).map(tab_item).collect(),
            },
        ],
    }
}

fn tab_item(index: usize) -> MenuItem {
    MenuItem::Action {
        id: format!("select_tab_{index}"),
        label: format!("Tab {}", index + 1),
        shortcut: tab_shortcut(index),
        disabled: false,
        checked: false,
    }
}

fn tab_shortcut(index: usize) -> Option<KeyChord> {
    let key = match index {
        0 => KeyCode::KEY_1,
        1 => KeyCode::KEY_2,
        2 => KeyCode::KEY_3,
        3 => KeyCode::KEY_4,
        4 => KeyCode::KEY_5,
        5 => KeyCode::KEY_6,
        6 => KeyCode::KEY_7,
        7 => KeyCode::KEY_8,
        8 => KeyCode::KEY_9,
        _ => return None,
    };
    Some(key.meta())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tab_items(payload: &AppMenuPayload) -> usize {
        payload
            .menus
            .iter()
            .find(|m| m.label == "Tabs")
            .map(|m| m.items.len())
            .unwrap_or(0)
    }

    #[test]
    fn empty_menu_has_no_tab_items() {
        assert_eq!(count_tab_items(&terminal_menu(0)), 0);
    }

    #[test]
    fn single_tab_menu_has_one_item() {
        let menu = terminal_menu(1);
        assert_eq!(count_tab_items(&menu), 1);
    }

    #[test]
    fn nine_tabs_get_shortcuts() {
        let menu = terminal_menu(9);
        let tabs = menu.menus.iter().find(|m| m.label == "Tabs").unwrap();
        for item in &tabs.items {
            if let MenuItem::Action { shortcut, .. } = item {
                assert!(shortcut.is_some(), "tabs 1-9 should have shortcuts");
            } else {
                panic!("expected Action items only");
            }
        }
    }

    #[test]
    fn tenth_tab_has_no_shortcut() {
        let menu = terminal_menu(12);
        let tabs = menu.menus.iter().find(|m| m.label == "Tabs").unwrap();
        // First 9 should have shortcuts; 10-12 should not.
        for (i, item) in tabs.items.iter().enumerate() {
            let MenuItem::Action { shortcut, .. } = item else {
                panic!("expected Action");
            };
            if i < 9 {
                assert!(shortcut.is_some(), "tab {} expected shortcut", i + 1);
            } else {
                assert!(shortcut.is_none(), "tab {} should have no shortcut", i + 1);
            }
        }
    }
}
```

- [ ] **Step 2: Remove the inline `mod menu` from `main.rs`**

In `crates/sola-terminal/src/main.rs`:

a) Add `mod menu;` to the module declarations near the top, alongside `mod commands; mod pty; mod state; mod tmux;`.

b) Delete the entire inline `mod menu { ... }` block.

c) Confirm the call sites still resolve: `menu::terminal_menu(0)` in `new`, `menu::terminal_menu(reconciled.len())` in `on_terminal_sessions`. (Same module path either way.)

- [ ] **Step 3: Run unit tests**

Run: `cargo test -p sola-terminal menu::`
Expected: PASS (4 tests).

- [ ] **Step 4: Build**

Run: `cargo make build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-terminal/src/main.rs crates/sola-terminal/src/menu.rs
git commit -m "$(cat <<'EOF'
refactor(sola-terminal): extract menu.rs with dynamic tab count

Move the inline mod menu out of main.rs into its own file. Add unit
tests covering empty / single-tab / 9-tab / 12-tab menus, including
the cap on Cmd+1..=Cmd+9 shortcuts.
EOF
)"
```

---

## Task 10: Register `sola-terminal` in `builtin_apps()`

**Files:**
- Modify: `crates/sola-core/src/applications.rs`

- [ ] **Step 1: Add the entry**

Edit `crates/sola-core/src/applications.rs::builtin_apps`. The current vec is:

```rust
pub fn builtin_apps() -> Vec<Application> {
    vec![
        Application {
            app_id: "sola-settings".into(),
            label: "Settings".into(),
            command: "/opt/sola/bin/sola-settings".into(),
            icon: "lucide/settings".into(),
        },
        Application {
            app_id: "sola-monitor".into(),
            label: "Monitor".into(),
            command: "/opt/sola/bin/sola-monitor".into(),
            icon: "lucide/monitor".into(),
        },
    ]
}
```

Add the terminal entry after `sola-monitor` (alphabetical order is fine if existing entries are alphabetical; otherwise just append):

```rust
        Application {
            app_id: "sola-terminal".into(),
            label: "Terminal".into(),
            command: "/opt/sola/bin/sola-terminal".into(),
            icon: "lucide/terminal".into(),
        },
```

- [ ] **Step 2: Run sola-core tests**

Run: `cargo test -p sola-core`
Expected: PASS. Any tests asserting the count of `builtin_apps()` should be updated to match (likely need `+1` somewhere; let the failure tell you the path).

- [ ] **Step 3: Build the whole workspace**

Run: `cargo make build`
Expected: PASS.

- [ ] **Step 4: Install everything**

Run: `cargo make install`
Expected: every binary including `sola-terminal` ends up in `/opt/sola/bin/`.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-core/src/applications.rs
git commit -m "$(cat <<'EOF'
feat(sola-core): register sola-terminal as a builtin application

Add Terminal to builtin_apps() so the launcher and Settings panel
recognize it. Uses lucide/terminal for the icon to match the
launcher's existing fallback for terminal-class apps.
EOF
)"
```

---

## Task 11: Final smoke verification

**Files:** none (manual checklist)

This task does not modify code; it confirms the system works end-to-end. Each substep produces a concrete observable.

- [ ] **Step 1: Launch sola from a TTY**

Switch to TTY1 (Ctrl+Alt+F1), log in if needed, then run:

```bash
RUST_LOG=info /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
```

Expected: shell, monitor, settings, river all start cleanly. No errors mentioning `TerminalConfig` or `TerminalSessions` (the bus may log "no sticky for X" on first ever startup — that's fine).

- [ ] **Step 2: Open the launcher and confirm Terminal is listed**

Trigger the launcher (Meta+Space or whatever the bound chord is). Type "term".

Expected: "Terminal" appears in the result list with the `lucide/terminal` icon.

- [ ] **Step 3: Open Terminal, confirm one tab spawns and the menu shows `Tab 1`**

Click "Terminal" in the launcher.

Expected:
- Terminal window opens.
- The sidebar contains exactly one tab; the active terminal pane shows a shell prompt.
- The menubar's "Tabs" menu contains exactly `Tab 1` (with Cmd+1 shortcut). It does not show Tab 2-9.

- [ ] **Step 4: Open a second tab; confirm menu grows; close it; confirm menu shrinks**

Press Cmd+T (or use Shell → New Tab). Then check Tabs menu: `Tab 1`, `Tab 2`. Close the second tab from the sidebar context menu (or by exiting its shell with `exit`). Tabs menu should show `Tab 1` only again.

- [ ] **Step 5: Resize sidebar; restart terminal; confirm width persists**

Drag the sidebar to a clearly different width (e.g. very narrow or very wide). Quit the terminal (Cmd+Q). Reopen Terminal from the launcher.

Expected: sidebar comes back at the new width.

Verify on disk:

```bash
grep -A2 "TerminalConfig" ~/.config/sola/state.toml
```

Expected: a `[TerminalConfig]` section with the new `sidebar_width`.

- [ ] **Step 6: Spawn 3 tabs, change cwd in each, restart sola entirely, confirm reattach + cwds**

In tab 1: `cd /tmp`. Tab 2: `cd ~/.config`. Tab 3: `cd /etc`. Quit sola entirely (Meta+Shift+Q or whatever the system shutdown chord is). Run `sola` again from the TTY. Open Terminal from the launcher.

Expected:
- Three tabs in the sidebar.
- Each tab's label is the cwd basename: `tmp`, `.config`, `etc`.
- Pressing return in each tab shows the cwd persisted (`pwd` returns the same paths).

Verify on disk:

```bash
grep -A20 "TerminalSessions" ~/.config/sola/state.toml
```

Expected: a `[TerminalSessions]` section listing three tabs with the right tmux session names and cwds.

- [ ] **Step 7: Right-click a tab; confirm there is no Rename option**

Right-click a tab in the sidebar.

Expected: any context menu shown does not include a Rename action. (If no context menu shows at all, that's also fine — the rename UX was the only consumer.)

- [ ] **Step 8: Final cleanup verification — confirm no orphaned `terminal-state.json`**

Run: `ls -la ~/.config/sola/terminal-state.json 2>/dev/null && echo PRESENT || echo ABSENT`

Expected: `ABSENT`. (We didn't migrate any old file because the terminal was absent from master; if a file is present on this host from a previous worktree experiment, delete it manually: `rm ~/.config/sola/terminal-state.json`.)

- [ ] **Step 9: No commit**

Smoke verification produces no code changes. Skip the commit step.

---

## Done

All tasks complete. The branch `feature/terminal-port` should contain ten commits (one per code task). Inspect with:

```bash
git log --oneline master..HEAD
```

Expected: 10 commits in order matching the task list above (move + 4 + extra fixes for Task 7's bus_proxy work) — exact count depends on whether sub-fixups in Task 7 were folded together; that's fine.

Final manual step is to ask the user for permission to merge `feature/terminal-port` into `master`.
