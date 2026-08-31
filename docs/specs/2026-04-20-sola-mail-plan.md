# sola-mail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the Cogsworth mail app (`../Cogsworth/apps/mail` + `../Cogsworth/crates/cogsworth-mail`) to a Sola app named `sola-mail` — Rust host on `sola-app`, frontend on Arrow.js.

**Architecture:** Single-window `SolaApp` with `AsyncDispatcher` + `AppHandler` for IMAP/SMTP work. Mail library inlined under `apps/mail/src/` (no separate workspace crate). Mpsc event bridge drained through `glib::timeout_add_local` pushes IDLE / connection events into `send_raw_json_to_js`. Menu is sticky `Topic::SetAppMenu` with one item (`Quit Mail`); vim-style message shortcuts stay in frontend `keydown`.

**Tech Stack:** Rust, `sola-app`, GTK4, WebKit6, tokio, `imap` + `rustls-connector`, `lettre`, `mail-parser`, `ureq` (wicket), Arrow.js, `@sola/ipc`, `@sola/store`.

**Spec:** `docs/specs/2026-04-20-sola-mail-design.md`

**Worktree:** `.worktrees/sola-mail` (branch: `sola-mail`) — already checked out.

**Reference app for patterns:** `.worktrees/sola-agent/apps/agent/` (current `AsyncDispatcher` pattern). Also `apps/terminal/web/src/` and `apps/browser/web/src/` for Arrow.js conventions.

**Port source:** `../Cogsworth/apps/mail/` + `../Cogsworth/crates/cogsworth-mail/`.

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `Cargo.toml` (workspace root) | Modify | Add `apps/mail` to `[workspace.members]`; pin `imap`, `lettre`, `mail-parser`, `rustls-connector`, `ureq`, `rustls`, `toml` in `[workspace.dependencies]` |
| `apps/mail/Cargo.toml` | Create | Crate manifest |
| `apps/mail/src/main.rs` | Create | `MailApp` (SolaApp impl), menu, startup auto-connect, event bridge |
| `apps/mail/src/handler.rs` | Create | `MailHandler` (AppHandler impl), async commands |
| `apps/mail/src/state.rs` | Create | `MailState` — client, config, idle/keepalive, event channels |
| `apps/mail/src/config.rs` | Create | `MailConfig` load/save + tests (ported from Cogsworth) |
| `apps/mail/src/rules.rs` | Create | `MailRule`, `MailRuleCondition`, matching fns + tests |
| `apps/mail/src/imap.rs` | Create | `ImapClient` (port of `cogsworth-mail/client.rs`) |
| `apps/mail/src/idle.rs` | Create | IDLE watcher (port of `cogsworth-mail/idle.rs`) |
| `apps/mail/src/sender.rs` | Create | SMTP send (port of `cogsworth-mail/sender.rs`) |
| `apps/mail/src/wicket.rs` | Create | Alias fetcher (port of `cogsworth-mail/wicket.rs`) |
| `apps/mail/src/menu.rs` | Create | `mail_menu()` returning `AppMenuPayload` |
| `apps/mail/web/index.html` | Create | Entry HTML |
| `apps/mail/web/src/main.ts` | Create | Entry — boots app |
| `apps/mail/web/src/app.ts` | Create | Reactive state tree, wiring, keyboard |
| `apps/mail/web/src/types.ts` | Create | `Folder`, `MessageSummary`, `MessageBody`, `MailRule`; `matchesRule`, `matchesAnySmartMailbox` |
| `apps/mail/web/src/components/folder-list.ts` | Create | Folder list component |
| `apps/mail/web/src/components/message-list.ts` | Create | Message list with search, infinite scroll, bulk actions |
| `apps/mail/web/src/components/message-view.ts` | Create | Body render, toolbar, link interception |
| `apps/mail/web/src/components/compose-view.ts` | Create | Compose / reply form |
| `apps/mail/web/src/components/toast.ts` | Create | Toast error banner |
| `apps/mail/web/src/theme.css` | Create | CSS custom properties + component styles |
| `crates/sola-make/src/` | Modify (if needed) | Register `mail` as a build/deploy target if apps aren't auto-discovered |

---

## Build sequence overview

14 tasks. Each ends with a `cargo check` (or `cargo test`) pass and a commit. Task 13 is a full manual smoke test on a TTY; user runs the install.

---

## Task 1: Scaffold `apps/mail` crate

**Files:**
- Create: `apps/mail/Cargo.toml`
- Create: `apps/mail/src/main.rs`
- Modify: `Cargo.toml` (workspace members + workspace dependencies)

- [ ] **Step 1: Add workspace dependencies**

In root `Cargo.toml`, under `[workspace.dependencies]`, add (match Cogsworth's versions — check `../Cogsworth/Cargo.toml` and `../Cogsworth/crates/cogsworth-mail/Cargo.toml`):

```toml
imap = "3.0.0-alpha.15"
rustls-connector = "0.21"
rustls = { version = "0.23", features = ["aws_lc_rs"] }
lettre = { version = "0.11", default-features = false, features = ["smtp-transport", "builder", "rustls-tls", "hostname"] }
mail-parser = "0.9"
ureq = { version = "2", features = ["tls"] }
toml = "0.8"
base64 = "0.22"
```

Then add `"apps/mail"` to `[workspace.members]`.

- [ ] **Step 2: Create `apps/mail/Cargo.toml`**

```toml
[package]
name = "sola-mail"
version.workspace = true
edition.workspace = true

[[bin]]
name = "sola-mail"
path = "src/main.rs"

[dependencies]
sola-app = { path = "../../crates/sola-app" }
sola-bus = { path = "../../crates/sola-bus" }
sola-core = { path = "../../crates/sola-core" }

gtk4 = "0.9"

tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }

serde = { version = "1", features = ["derive"] }
serde_json = "1"

tracing = "0.1"
anyhow = "1"
async-trait = "0.1"

imap = { workspace = true }
rustls = { workspace = true }
rustls-connector = { workspace = true }
lettre = { workspace = true }
mail-parser = { workspace = true }
ureq = { workspace = true }
toml = { workspace = true }
base64 = { workspace = true }
```

- [ ] **Step 3: Stub `apps/mail/src/main.rs`**

```rust
fn main() {
    println!("sola-mail stub");
}
```

- [ ] **Step 4: Verify build**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 5: Check sola-make registration**

Read `crates/sola-make/src/` and check whether apps are auto-discovered or require a listing. If `mail` needs to be added somewhere, add it so `cargo make build mail` and `cargo make install mail` resolve.

Run: `cargo make build mail`
Expected: Succeeds.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml apps/mail/ crates/sola-make/
git commit -m "feat(mail): scaffold sola-mail crate"
```

---

## Task 2: Port `config.rs` with tests

Port verbatim from `../Cogsworth/crates/cogsworth-mail/src/config.rs` with **one change**: `MailConfig::config_path()` returns `${XDG_CONFIG_HOME:-~/.config}/sola/mail.toml` instead of `.../cogsworth/mail.toml`.

**Files:**
- Create: `apps/mail/src/config.rs`
- Modify: `apps/mail/src/main.rs` (add `mod config;`)

- [ ] **Step 1: Copy the file**

Read `../Cogsworth/crates/cogsworth-mail/src/config.rs` and copy it to `apps/mail/src/config.rs`.

- [ ] **Step 2: Fix the config path**

In `config_path()`, replace the `"cogsworth"` literal with `"sola"`. Match the exact surrounding code from the Cogsworth version. Keep everything else (tests and all) identical.

- [ ] **Step 3: Add `mod config;` to `main.rs`**

Replace `main.rs` with:
```rust
mod config;

fn main() {
    println!("sola-mail stub");
}
```

- [ ] **Step 4: Update tests that assert the config path**

Search tests for `"cogsworth"` and replace with `"sola"` in assertions. Verify the legacy `[wicket]` test still parses — the config *schema* doesn't change, only the file path does.

- [ ] **Step 5: Run tests**

Run: `cargo test -p sola-mail config::`
Expected: all tests pass (parse_valid_config, parse_legacy_wicket_config, parse_partial_config, parse_empty_config, parse_rule_config, serialize_full_config).

- [ ] **Step 6: Commit**

```bash
git add apps/mail/src/config.rs apps/mail/src/main.rs
git commit -m "feat(mail): port MailConfig with sola config path"
```

---

## Task 3: Port `rules.rs` (types + matching) with tests

Cogsworth places the types in `types.rs` and matching in `apps/mail/src/mail_bridge.rs::rule_matches_message`. We consolidate both into `rules.rs`.

**Files:**
- Create: `apps/mail/src/rules.rs`
- Modify: `apps/mail/src/main.rs` (add `mod rules;`)

- [ ] **Step 1: Copy the types**

From `../Cogsworth/crates/cogsworth-mail/src/types.rs`, copy `MailRule` and `MailRuleCondition` (with their derives and the `#[serde(rename = "match")]`) into `apps/mail/src/rules.rs`. Also copy the `Folder`, `MessageSummary`, and `MessageBody` structs — `imap.rs` will need them shortly; keeping them here is fine, or move them later.

- [ ] **Step 2: Port the matcher**

From `../Cogsworth/apps/mail/src/mail_bridge.rs`, copy `rule_matches_message` (and any helpers it depends on — usually a `field_value` helper and the `match_type` switch covering `equals` / `contains` / `domain` / `address`). Rename / scope them to live inside `rules.rs`. Public fn signature:

```rust
pub fn rule_matches(rule: &MailRule, from: &str, subject: &str, to: &str) -> bool
```

- [ ] **Step 3: Write tests**

Cogsworth doesn't have tests for these. Write tests that cover the four `match_type` variants across `from` / `subject` fields:

```rust
#[test]
fn domain_match_matches_any_address_in_domain() {
    let rule = rule("from", "domain", "github.com");
    assert!(rule_matches(&rule, "noreply@github.com", "", ""));
    assert!(rule_matches(&rule, "Bot <bot@github.com>", "", ""));
    assert!(!rule_matches(&rule, "someone@example.com", "", ""));
}

#[test]
fn address_match_requires_exact_address() {
    let rule = rule("from", "address", "a@b.com");
    assert!(rule_matches(&rule, "a@b.com", "", ""));
    assert!(rule_matches(&rule, "A <a@b.com>", "", ""));
    assert!(!rule_matches(&rule, "a@b.co", "", ""));
}

#[test]
fn contains_match_substring_case_insensitive() {
    let rule = rule("subject", "contains", "invoice");
    assert!(rule_matches(&rule, "", "Your INVOICE #1", ""));
    assert!(!rule_matches(&rule, "", "Receipt", ""));
}

#[test]
fn equals_match_full_string() {
    let rule = rule("subject", "equals", "ping");
    assert!(rule_matches(&rule, "", "ping", ""));
    assert!(!rule_matches(&rule, "", "ping!", ""));
}

#[test]
fn all_conditions_must_match() {
    let mut r = rule("from", "domain", "example.com");
    r.conditions.push(MailRuleCondition { field: "subject".into(), match_type: "contains".into(), value: "alert".into() });
    assert!(rule_matches(&r, "x@example.com", "alert: down", ""));
    assert!(!rule_matches(&r, "x@example.com", "news", ""));
}

fn rule(field: &str, match_type: &str, value: &str) -> MailRule {
    MailRule {
        name: "t".into(),
        action: "smart_mailbox".into(),
        dest: None,
        conditions: vec![MailRuleCondition { field: field.into(), match_type: match_type.into(), value: value.into() }],
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sola-mail rules::`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add apps/mail/src/rules.rs apps/mail/src/main.rs
git commit -m "feat(mail): port mail rules and matching logic with tests"
```

---

## Task 4: Port `imap.rs` (client)

Port verbatim from `../Cogsworth/crates/cogsworth-mail/src/client.rs`. No logic changes — just import-path adjustments.

**Files:**
- Create: `apps/mail/src/imap.rs`
- Modify: `apps/mail/src/main.rs` (add `mod imap;`)

- [ ] **Step 1: Copy the file**

Read `../Cogsworth/crates/cogsworth-mail/src/client.rs` in full and copy it to `apps/mail/src/imap.rs`.

- [ ] **Step 2: Fix imports**

The Cogsworth version uses `crate::types::{Folder, MessageSummary, MessageBody}`. In `apps/mail/src/imap.rs`, change to `crate::rules::{Folder, MessageSummary, MessageBody}` (or `crate::types::...` if you split types out later — match whichever path rules.rs puts them at).

If the file imports anything else from `crate::...` that doesn't exist yet, leave the import; it'll resolve once `rules.rs` exposes the types.

- [ ] **Step 3: Add `mod imap;` to `main.rs`**

- [ ] **Step 4: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds (no new tests — client has no unit coverage in Cogsworth).

- [ ] **Step 5: Commit**

```bash
git add apps/mail/src/imap.rs apps/mail/src/main.rs
git commit -m "feat(mail): port IMAP client"
```

---

## Task 5: Port `idle.rs`, `sender.rs`, `wicket.rs`

Port all three verbatim from Cogsworth with only import-path adjustments.

**Files:**
- Create: `apps/mail/src/idle.rs`
- Create: `apps/mail/src/sender.rs`
- Create: `apps/mail/src/wicket.rs`
- Modify: `apps/mail/src/main.rs` (add `mod idle; mod sender; mod wicket;`)

- [ ] **Step 1: Copy idle.rs**

From `../Cogsworth/crates/cogsworth-mail/src/idle.rs` → `apps/mail/src/idle.rs`. Update `crate::types::` → `crate::rules::` (or wherever types live), and `crate::client::` → `crate::imap::`.

Note: the IDLE module also applies move rules on new messages (calls `rule_matches_message`). In our layout that function is `crate::rules::rule_matches`; adjust the call sites. Also update any `MAX_INITIAL_SCAN` or backoff constants — keep identical values.

- [ ] **Step 2: Copy sender.rs**

From `../Cogsworth/crates/cogsworth-mail/src/sender.rs` → `apps/mail/src/sender.rs`. No imports from the `crate::` root other than possibly `MailConfig` — adjust to `crate::config::MailConfig`.

- [ ] **Step 3: Copy wicket.rs**

From `../Cogsworth/crates/cogsworth-mail/src/wicket.rs` → `apps/mail/src/wicket.rs`. Should be self-contained (uses `ureq` + `base64`); no `crate::` imports to change.

- [ ] **Step 4: Add modules to `main.rs`**

```rust
mod config;
mod rules;
mod imap;
mod idle;
mod sender;
mod wicket;

fn main() {
    println!("sola-mail stub");
}
```

- [ ] **Step 5: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 6: Commit**

```bash
git add apps/mail/src/
git commit -m "feat(mail): port IDLE, SMTP sender, and wicket alias fetcher"
```

---

## Task 6: State + handler scaffold

Wire the `AsyncDispatcher` glue, but stub the commands — they return `"ok"` or empty data. Real command bodies come in Task 7.

**Files:**
- Create: `apps/mail/src/state.rs`
- Create: `apps/mail/src/handler.rs`
- Modify: `apps/mail/src/main.rs`

- [ ] **Step 1: Create `state.rs`**

```rust
use std::sync::{mpsc, Arc};

use crate::config::MailConfig;
use crate::idle::IdleHandle;
use crate::imap::ImapClient;
use crate::rules::MailRule;

pub struct MailState {
    pub client: tokio::sync::Mutex<Option<Arc<std::sync::Mutex<ImapClient>>>>,
    pub config: tokio::sync::RwLock<Option<MailConfig>>,
    pub idle_handle: tokio::sync::Mutex<Option<IdleHandle>>,
    pub idle_move_rules: Arc<std::sync::Mutex<Vec<MailRule>>>,
    pub keepalive_abort: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub event_tx: mpsc::Sender<String>,
}

impl MailState {
    pub fn new(event_tx: mpsc::Sender<String>) -> Self {
        Self {
            client: Default::default(),
            config: Default::default(),
            idle_handle: Default::default(),
            idle_move_rules: Arc::new(std::sync::Mutex::new(Vec::new())),
            keepalive_abort: Default::default(),
            event_tx,
        }
    }
}
```

- [ ] **Step 2: Create handler stub in `handler.rs`**

```rust
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::state::MailState;

pub struct MailHandler {
    pub state: Arc<MailState>,
}

#[async_trait]
impl sola_app::AppHandler for MailHandler {
    async fn dispatch(&self, cmd: &str, _args: &Value) -> Value {
        match cmd {
            "mail_connect"
            | "mail_test_connection"
            | "mail_list_folders"
            | "mail_list_messages"
            | "mail_search"
            | "mail_fetch_body"
            | "mail_send"
            | "mail_move"
            | "mail_mark_read"
            | "mail_empty_folder"
            | "apply_rules"
            | "open_url" => json!({ "ok": true, "todo": cmd }),
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        }
    }
}
```

- [ ] **Step 3: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 4: Commit**

```bash
git add apps/mail/src/state.rs apps/mail/src/handler.rs apps/mail/src/main.rs
git commit -m "feat(mail): scaffold MailState and MailHandler"
```

---

## Task 7: Implement handler commands

Port the command bodies from `../Cogsworth/apps/mail/src/handler.rs` (and `mail_bridge.rs` for `apply_rules`). Each command uses `tokio::task::spawn_blocking` to run IMAP/SMTP work (same as Cogsworth — blocking crates inside a tokio runtime).

**Files:**
- Modify: `apps/mail/src/handler.rs`
- Modify: `apps/mail/src/state.rs` (only if a helper lands here)

For each of the 11 commands, port the Cogsworth body with these substitutions:
- `state.client_arc.lock().await` stays the same — we have the same field name (`state.client`).
- `state.tx_map` / per-window `Sender`s in Cogsworth become `state.event_tx` here. Any place Cogsworth sent to a specific client via window-id, we push one message through `event_tx` and the frontend's single `on('mail:new', ...)` receives it.
- `tokio::task::spawn_blocking(move || { let mut c = client.lock().unwrap(); c.list_folders() }).await?` — keep this shape.
- For `apply_rules`: port `apply_move_rules_on_idle` from `mail_bridge.rs`; it iterates messages, checks rules with `crate::rules::rule_matches`, calls `client.move_message`. Emit a `mail:new` event through `event_tx` when `moved > 0`.
- For `open_url`: defer to Task 9 — for now return `json!({ "error": "not wired" })`.

- [ ] **Step 1: Port each command**

Work through them in this order (each is one pass: read the Cogsworth source, adapt, cargo-check):
1. `mail_test_connection`
2. `mail_connect` (returns `{ folders, smart_counts, from_addresses, rules }`)
3. `mail_list_folders`
4. `mail_list_messages`
5. `mail_search`
6. `mail_fetch_body`
7. `mail_mark_read`
8. `mail_move`
9. `mail_empty_folder`
10. `mail_send` (also appends to Sent via IMAP)
11. `apply_rules`

After each command, run `cargo check -p sola-mail`.

- [ ] **Step 2: Wire IDLE startup inside `mail_connect`**

After a successful `mail_connect`, call `idle::start_idle(config, on_new)` where `on_new` pushes `{"event":"mail:new"}` into `state.event_tx`. Stash the returned `IdleHandle` in `state.idle_handle`. Keepalive NOOP loop (240s) — spawn a `tokio::task`, stash the handle in `state.keepalive_abort`. On a second connect, abort the old ones first.

- [ ] **Step 3: Run tests**

Run: `cargo test -p sola-mail`
Expected: config + rules tests still pass.

- [ ] **Step 4: Commit**

```bash
git add apps/mail/src/handler.rs apps/mail/src/state.rs
git commit -m "feat(mail): implement IMAP/SMTP commands and IDLE watcher"
```

---

## Task 8: Main entry — SolaApp impl + event bridge + menu

Build `main.rs` modeled on `.worktrees/sola-agent/apps/agent/src/main.rs`. Single window, `AsyncDispatcher`, mpsc event bridge, menu emission, startup auto-connect.

**Files:**
- Modify: `apps/mail/src/main.rs`
- Create: `apps/mail/src/menu.rs`

- [ ] **Step 1: Write `menu.rs`**

```rust
use sola_app::menu::{AppMenuPayload, KeyCode, MenuDefinition, MenuItem};

use crate::MailApp;
use sola_app::SolaApp;

pub fn mail_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: MailApp::APP_ID.into(),
        menus: vec![MenuDefinition {
            label: "Mail".into(),
            items: vec![MenuItem::Action {
                id: "quit".into(),
                label: "Quit Mail".into(),
                shortcut: Some(KeyCode::Q.meta()),
                disabled: false,
                checked: false,
            }],
        }],
    }
}
```

(Exact import paths should match whatever `.worktrees/sola-agent/apps/agent/src/menu.rs` uses — copy the `use` block from there.)

- [ ] **Step 2: Write `main.rs` — skeleton**

Model verbatim on `.worktrees/sola-agent/apps/agent/src/main.rs`. Key points:
- `APP_ASSETS` bundles every file in `web/` (see structure in spec §Structure).
- `MailApp` struct: `main_window: WindowHandle`, `dispatcher: AsyncDispatcher`.
- `SolaApp::APP_ID = "sola-mail"`.
- `new(ctx)`:
  1. Install rustls provider: `let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();`
  2. Create `main_window` with `WindowConfig { title: "main", size: (1280, 820), position: None, decorated: false, transparent: false, assets: APP_ASSETS, initial_state: None, zoned: true, keyboard_target: true }`.
  3. Emit sticky menu: `ctx.emit_sticky(Topic::SetAppMenu(menu::mail_menu()))` (check exact API name by reading how agent does it).
  4. Set up mpsc event bridge: `let (event_tx, event_rx) = mpsc::channel::<String>();` — then clone `main_window` and register `gtk4::glib::timeout_add_local(Duration::from_millis(5), move || { while let Ok(msg) = event_rx.try_recv() { mw.send_raw_json_to_js(&msg); } glib::ControlFlow::Continue });`.
  5. Build `state = Arc::new(MailState::new(event_tx))`.
  6. `let dispatcher = AsyncDispatcher::spawn(MailHandler { state: state.clone() });`
  7. Fire-and-forget startup auto-connect: `dispatcher.dispatch("mail_connect".into(), json!({}), |_| {});`.
  8. Return `MailApp { main_window, dispatcher }`.
- `register_bus(&mut self, bus, _ctx)`: `bus.on(TopicKind::MenuAction, Self::on_menu_action);`
- `on_menu_action(&mut self, msg, _ctx)`: if `msg.action_id == "quit"`, call `std::process::exit(0)`.
- `on_js_command(...)`: forward to `self.dispatcher.dispatch(...)`, on reply call `source.send_to_js(&json!({ "id": id, "result": result }))`.

Copy the agent's exact shape — any divergence here is unintentional. Place a placeholder `web/index.html` and empty `web/src/main.ts` so `asset_bundle!` compiles.

- [ ] **Step 3: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 4: Commit**

```bash
git add apps/mail/
git commit -m "feat(mail): SolaApp entry, event bridge, quit menu"
```

---

## Task 9: `open_url` — handler → bus plumbing

The spec calls this an open item. Resolve it with the same pattern as the event bridge: handler pushes a `Topic::OpenUrl` payload through a `std::sync::mpsc::Sender`, the main-thread glib timeout drains it and calls `ctx.emit(Topic::OpenUrl(...))`.

**Files:**
- Modify: `apps/mail/src/state.rs` (add `open_url_tx`)
- Modify: `apps/mail/src/handler.rs` (`open_url` command pushes on the channel)
- Modify: `apps/mail/src/main.rs` (drain the channel in the existing timeout and emit the bus topic)

- [ ] **Step 1: Read how other apps emit OpenUrl**

Grep the repo for `Topic::OpenUrl` to find the existing emit pattern. Use whichever `AppCtx` method is idiomatic (`ctx.emit`, `ctx.bus().publish`, etc.). If the topic doesn't yet exist for this direction, reuse the browser's existing `OpenUrlRequest` shape.

- [ ] **Step 2: Add `open_url_tx: mpsc::Sender<String>` to `MailState`**

Wire a second `mpsc::channel::<String>()` in `main.rs`. Store sender on state, clone receiver into the timeout.

- [ ] **Step 3: Implement `open_url` in `handler.rs`**

```rust
"open_url" => {
    if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
        let _ = self.state.open_url_tx.send(url.to_string());
        json!("ok")
    } else {
        json!({ "error": "missing url" })
    }
}
```

- [ ] **Step 4: Drain in the glib timeout**

Add a second `while let Ok(url) = open_url_rx.try_recv()` loop alongside the event drain; call the bus emit for each URL.

- [ ] **Step 5: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 6: Commit**

```bash
git add apps/mail/src/
git commit -m "feat(mail): wire open_url command to Topic::OpenUrl"
```

---

## Task 10: Frontend — types + entry + app skeleton

Frontend port starts here. Produce a working skeleton that connects, renders folders, and handles loading / fatal errors. Message list, view, compose follow in Tasks 11 + 12.

**Files:**
- Create: `apps/mail/web/index.html`
- Create: `apps/mail/web/src/main.ts`
- Create: `apps/mail/web/src/types.ts`
- Create: `apps/mail/web/src/theme.css`
- Create: `apps/mail/web/src/app.ts`

- [ ] **Step 1: Write `index.html`**

```html
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>sola-mail</title>
  <link rel="stylesheet" href="/src/theme.css">
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

- [ ] **Step 2: Write `types.ts`**

TypeScript types mirroring the Rust structs. Include the pure helpers:

```ts
export interface Folder { name: string; unread: number; total: number; }
export interface MessageSummary {
  uid: number; from: string; to: string; subject: string;
  date: string; seen: boolean; forwarded_for?: string | null;
}
export interface MessageBody {
  uid: number; from: string; to: string; cc: string;
  subject: string; date: string;
  html: string | null; text: string;
  in_reply_to: string | null; message_id: string | null;
}
export interface MailRuleCondition { field: string; match: string; value: string; }
export interface MailRule { name: string; action: string; dest?: string | null; conditions: MailRuleCondition[]; }

export function smartMailboxNames(rules: MailRule[]): string[] {
  return rules.filter(r => r.action === 'smart_mailbox').map(r => r.name);
}

export function matchesRule(rule: MailRule, msg: Pick<MessageSummary,'from'|'subject'|'to'>): boolean {
  // port logic from Cogsworth MailWindow.svelte:matchesRule
  // (domain / address / contains / equals across from/subject/to)
}

export function matchesAnySmartMailbox(rules: MailRule[], msg: Pick<MessageSummary,'from'|'subject'|'to'>): boolean {
  return rules.some(r => r.action === 'smart_mailbox' && matchesRule(r, msg));
}
```

Copy the `matchesRule` body directly from `../Cogsworth/apps/mail/frontend/src/lib/MailWindow.svelte` — it's a small switch.

- [ ] **Step 3: Write `theme.css` (starter)**

Start with only CSS custom properties. Component styles get added in Task 14. Match the palette from `apps/terminal/web/src/theme.css` where possible so the app feels native to Sola.

- [ ] **Step 4: Write `main.ts`**

Match the shape of `apps/agent/web/src/main.ts` / `apps/browser/web/src/main.ts`:

```ts
import { invoke, on } from '@sola/ipc';
import { createApp } from './app';

createApp(document.getElementById('app')!);
```

- [ ] **Step 5: Write `app.ts` skeleton**

```ts
import { reactive, html, watch } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import type { Folder, MessageSummary, MessageBody, MailRule } from './types';

export function createApp(root: HTMLElement) {
  const state = reactive({
    folders: [] as Folder[],
    smartCounts: [] as Folder[],
    selectedFolder: 'INBOX',
    messages: [] as MessageSummary[],
    inboxMessages: [] as MessageSummary[],
    totalMessages: 0,
    selectedUid: null as number | null,
    messageBody: null as MessageBody | null,
    composing: false,
    replyTo: null as MessageBody | null,
    fromAddresses: [] as string[],
    rules: [] as MailRule[],
    loading: true,
    fatalError: null as string | null,
    toastError: null as string | null,
    searchQuery: '',
    searchActive: false,
    searchTotal: 0,
    isLoadingMore: false,
    bulkInProgress: false,
    folderLoading: false,
    lastMove: null as { uid: number; fromFolder: string; toFolder: string } | null,
  });

  html`
    <div class="mail-app">
      ${() => state.fatalError
        ? html`<div class="fatal">${() => state.fatalError}</div>`
        : state.loading
          ? html`<div class="loading">Connecting…</div>`
          : html`<div class="main">folders: ${() => state.folders.length}</div>`}
    </div>
  `(root);

  (async () => {
    try {
      const res = await invoke('mail_connect');
      state.folders = res.folders ?? [];
      state.smartCounts = res.smart_counts ?? [];
      state.fromAddresses = res.from_addresses ?? [];
      state.rules = res.rules ?? [];
      state.loading = false;
    } catch (e: any) {
      state.fatalError = String(e?.message ?? e);
      state.loading = false;
    }
  })();

  on('mail:new', () => { /* Task 13 */ });
}
```

- [ ] **Step 6: Compile + run**

Run: `cargo check -p sola-mail`
Expected: Succeeds. (No frontend build step — `asset_bundle!` bundles sources at compile time.)

- [ ] **Step 7: Commit**

```bash
git add apps/mail/web/
git commit -m "feat(mail): frontend skeleton — connect flow, loading, fatal error"
```

---

## Task 11: Folder list component

**Files:**
- Create: `apps/mail/web/src/components/folder-list.ts`
- Modify: `apps/mail/web/src/app.ts` (create target + call component)

- [ ] **Step 1: Design the component contract**

Port `../Cogsworth/apps/mail/frontend/src/lib/mail/FolderList.svelte`. Key behaviours:
- FOLDER_ORDER = `{ INBOX: 0, Sent: 1, Drafts: 2, Archive: 3, Junk: 4, Trash: 5 }`. Unlisted folders sort alphabetically after.
- Active folder has a cyan `::before` indicator.
- Count display: `{#if folder.unread > 0}{folder.unread}/{/if}{folder.total}`.
- Smart mailbox section is shown when there are rules with `action === 'smart_mailbox'`. Smart folder id: `smart:${rule.name}`. Show the count from `state.smartCounts`.

Accessor-closure config:

```ts
export interface FolderListConfig {
  folders: () => Folder[];
  smartCounts: () => Folder[];
  smartMailboxNames: () => string[];
  selected: () => string;
  onSelect: (folder: string) => void;
}

export function createFolderList(cfg: FolderListConfig, target: HTMLElement): void { /* ... */ }
```

- [ ] **Step 2: Template**

Use `html\`...\`` with `@click="${()=>cfg.onSelect(f.name)}"` on each row. Reassign `state.folders` whenever the parent updates it; Arrow re-renders via accessor closures.

- [ ] **Step 3: Wire into `app.ts`**

Add folder-list target div into the main template, call `createFolderList({ ... }, target)` inside `createApp`. `onSelect` sets `state.selectedFolder` and triggers a `mail_list_messages` fetch (create a top-level `loadFolder(name)` helper in `app.ts`).

- [ ] **Step 4: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 5: Commit**

```bash
git add apps/mail/web/src/
git commit -m "feat(mail): folder-list component"
```

---

## Task 12: Message list component

Port `MessageList.svelte` — search bar, infinite scroll, bulk actions.

**Files:**
- Create: `apps/mail/web/src/components/message-list.ts`
- Modify: `apps/mail/web/src/app.ts` (wire in + bulk action handlers)

- [ ] **Step 1: Component contract**

```ts
export interface MessageListConfig {
  messages: () => MessageSummary[];
  selectedUid: () => number | null;
  hasMore: () => boolean;
  isLoadingMore: () => boolean;
  folderLoading: () => boolean;
  searchActive: () => boolean;
  searchTotal: () => number;
  folderName: () => string;
  isSmartMailbox: () => boolean;
  isBulkOperating: () => boolean;
  onSelect: (uid: number) => void;
  onSearch: (query: string) => void;
  onClearSearch: () => void;
  onLoadMore: () => void;
  onArchiveAll: () => void;
  onTrashAll: () => void;
  onEmptyFolder: () => void;
}
```

- [ ] **Step 2: Internal state**

Local `reactive({ input: '', autoLoadCount: 0 })`. Bind the search `<input>` via `@input` to update `input`; Enter triggers `cfg.onSearch(input)`; Escape triggers `cfg.onClearSearch()` and resets `input`.

- [ ] **Step 3: Infinite scroll**

Attach a scroll listener: when `scrollHeight - scrollTop - clientHeight < 100 && cfg.hasMore() && !cfg.isLoadingMore()`, call `cfg.onLoadMore()`.

After render, if messages don't fill viewport and `cfg.hasMore()`, auto-load-more — cap via the local `autoLoadCount` at 3 (matches Cogsworth).

- [ ] **Step 4: Sender + date formatters**

Port `senderName` (extract name from `"Name <email>"`, 24-char cap) and `formatDate` (relative: `now`, `Xm`, `Xh`, `Xd`, `MonDay`) from `MessageList.svelte`.

- [ ] **Step 5: Bulk action bar**

Conditionally render at top:
- Smart mailbox: `[Archive all] [Trash all]`
- Junk/Trash folder: `[Permanently delete all]`

All buttons `disabled` when `cfg.isBulkOperating()`.

- [ ] **Step 6: Wire into `app.ts`**

Add commands to `app.ts`:
- `loadFolder(name)`: sets `folderLoading`, calls `mail_list_messages`, assigns `state.messages`, keeps `inboxMessages` in sync when name is INBOX, clears search state.
- `searchMessages(query)`: `mail_search`.
- `loadMore()`: paginated `mail_list_messages` or `mail_search`.
- `bulkMove(dest)`, `emptyFolder()`: use `bulkInProgress` guard.

- [ ] **Step 7: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 8: Commit**

```bash
git add apps/mail/web/src/
git commit -m "feat(mail): message-list component with search, infinite scroll, bulk actions"
```

---

## Task 13: Message view + compose view + toast + keyboard + IDLE handler

Three components plus the final wiring. Do them in one task because they're interdependent (MessageView triggers Compose for reply; both share the toast for error surfacing; keyboard handler targets the selected message).

**Files:**
- Create: `apps/mail/web/src/components/message-view.ts`
- Create: `apps/mail/web/src/components/compose-view.ts`
- Create: `apps/mail/web/src/components/toast.ts`
- Modify: `apps/mail/web/src/app.ts`

- [ ] **Step 1: `message-view.ts`**

Port `MessageView.svelte`. The iframe-with-srcdoc pattern ports directly:
- `<iframe sandbox="allow-same-origin allow-scripts" srcdoc="${sanitized}">`.
- `sanitizeHtml` strips `<script>`, `<form>`, and `on*` attributes (copy the regex from Cogsworth).
- Inside the iframe, a tiny inline script postMessages `{type:'open-url', url}` on anchor clicks; parent listens on `window.addEventListener('message', ...)`, filters `type === 'open-url'`, calls `invoke('open_url', { url })`.
- Keydown forwarding from iframe to parent window (so j/i/a/d/u/w/s shortcuts still work when the iframe has focus).
- Toolbar: `New` / `Reply` / `Reply All` / `Delete`.
- Header: From / To / CC / Subject.

Config:
```ts
{
  body: () => MessageBody | null,
  onNew: () => void,          // open empty Compose
  onReply: (all: boolean) => void,
  onDelete: () => void,
}
```

- [ ] **Step 2: `compose-view.ts`**

Port `ComposeView.svelte`. Fields: from (dropdown from `fromAddresses`), to, cc, subject, body (plain textarea). Send button calls `mail_send`; Close button resets state.

Config:
```ts
{
  fromAddresses: () => string[],
  replyTo: () => MessageBody | null,
  onSend: (msg: { from: string; to: string; cc?: string; subject: string; body: string; in_reply_to?: string }) => Promise<void>,
  onClose: () => void,
}
```

When `replyTo` is non-null on mount, prefill `to = replyTo.from`, `subject = "Re: ..."` (only if subject doesn't already start with `Re:`), and `in_reply_to = replyTo.message_id`. For Reply All, also fill `cc` from original `cc` + other `to` recipients minus self.

- [ ] **Step 3: `toast.ts`**

Simple dismissable banner. Config: `{ message: () => string | null, onDismiss: () => void }`. Auto-dismiss after 5s via `setTimeout`.

- [ ] **Step 4: Keyboard shortcuts in `app.ts`**

```ts
window.addEventListener('keydown', (e) => {
  if (state.composing) return;
  const t = e.target as HTMLElement;
  if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA') return;
  if (e.ctrlKey || e.altKey || e.metaKey) return;

  const uid = state.selectedUid;
  if (uid == null) return;
  switch (e.key) {
    case 'j': moveAndAdvance(uid, 'Junk'); break;
    case 'i': moveAndAdvance(uid, 'INBOX'); break;
    case 'a': moveAndAdvance(uid, 'Archive'); break;
    case 'd': moveAndAdvance(uid, 'Trash'); break;
    case 'u': undoLastMove(); break;
    case 'w': selectPrev(); break;
    case 's': selectNext(); break;
  }
});
```

Implement `moveAndAdvance`, `undoLastMove`, `selectPrev`, `selectNext` based on `MailWindow.svelte`. `moveAndAdvance` records `state.lastMove` before calling `mail_move`.

- [ ] **Step 5: IDLE handler + window focus refresh**

```ts
on('mail:new', () => { refreshFolder(); });

let lastFetch = Date.now();
window.addEventListener('focus', () => {
  if (Date.now() - lastFetch > 60_000) refreshFolder();
});
```

Where `refreshFolder()` calls `mail_list_folders` + `mail_list_messages` for the currently selected folder (skip if searching).

- [ ] **Step 6: Errors → toast**

Wrap every `invoke(...)` in `app.ts` with a helper that catches, sets `state.toastError = e.message`, and rethrows for caller fallback.

- [ ] **Step 7: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 8: Commit**

```bash
git add apps/mail/web/
git commit -m "feat(mail): message/compose views, toast, keyboard shortcuts, IDLE refresh"
```

---

## Task 14: Styling pass + final smoke test

Layer on the styles. Manual smoke test on a TTY — **user installs, you run it.**

**Files:**
- Modify: `apps/mail/web/src/theme.css`
- Possibly split per-component: `apps/mail/web/src/components/*.css`

- [ ] **Step 1: Port Svelte `<style>` blocks**

From each of the five Cogsworth `.svelte` files in `../Cogsworth/apps/mail/frontend/src/lib/`, take the `<style>` block (strip scoped selectors) and paste into either `theme.css` or a dedicated per-component CSS. Keep FolderList's cyan indicator and 160px width.

- [ ] **Step 2: Include CSS files in `asset_bundle!`**

Add to `APP_ASSETS` in `main.rs`.

- [ ] **Step 3: Compile-check**

Run: `cargo check -p sola-mail`
Expected: Succeeds.

- [ ] **Step 4: Commit**

```bash
git add apps/mail/
git commit -m "feat(mail): styling pass"
```

- [ ] **Step 5: Await user deploy + smoke test**

Announce to user: "Ready for install. Run `cargo make install mail` when you're ready, then launch sola."

Smoke test checklist (walk through with user):
1. Startup → folders + INBOX list appear.
2. Click message → body renders, folder counts tick on auto-mark-read.
3. Compose → send → lands in Sent.
4. `d` → message → Trash; selection advances; `u` restores.
5. IDLE receives new mail → toast-free refresh.
6. Search → filtered list; Escape restores.
7. Smart mailbox → rule-matched messages only.
8. Click http link in body → opens in sola-browser.
9. Cmd+Q → process exits.

Fix any issues that surface, commit fixes.

---

## Open items resolved by this plan

- **`open_url` plumbing** → Task 9 uses a second `std::sync::mpsc::Sender<String>` drained in the same glib timeout as the event bridge.
- **`sola-make` registration** → Task 1 step 5 checks this.
- **Workspace dep versions** → Task 1 pins them from Cogsworth's versions.

## Notes for the implementing subagent

- DRY: if you find yourself copy-pasting IMAP logic inside a handler, it likely belongs on `ImapClient` — but Cogsworth already factored that, so the temptation should be rare.
- TDD where useful: `config.rs` and `rules.rs` have unit tests; IMAP/IDLE/SMTP don't (no way to mock the servers usefully). Don't invent tests.
- No speculative abstractions. One window, one event channel, one open_url channel. That's it.
- Frequent commits — every task ends with a commit.
- **Do not deploy.** Stop after Task 14 step 4; the user runs the deploy.
- All edits happen in `.worktrees/sola-mail`. Never touch the project root.
