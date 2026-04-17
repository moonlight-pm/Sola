# sola-session Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move user-app lifecycle (spawn, close, reap) out of `sola` into a new `sola-session` crate; add bus topic subscriptions so `sola`'s message queue no longer floods; fix the `BusClient` notify-pipe deadlock; add Meta+Q close-app flow; persist and restore the running-app set across restarts.

**Architecture:** Bus gains `Subscribe` + `Identify` control messages and a per-client topic filter. `BusClient::set_app_id` now emits `Identify`, and `BusClient::subscribe` is the only way to receive messages. `sola-app` gains a `BusRegistry` so handlers register per-topic (subscription = union of registered kinds). New `sola-session` daemon owns user-app spawn/close/reap. Shell gains a `session.json` file (written on every state change) and a restore loop gated by `ClientConnected("sola-session")`. Compositor subscribes to `CloseApp` and sends `xdg_toplevel.close` for matching windows. sola-app framework stops the default GTK close-then-quit path so sola apps only exit on bus `CloseApp`.

**Tech Stack:** Rust (workspace crates), synchronous bus + GLib main loops. JSON via `serde_json`, binary payloads via `postcard`. Testing: `cargo test` (unit tests in `#[cfg(test)]`), `cargo make deploy` for integration verification on the local machine.

---

## File Map

New files:
- `crates/sola-session/Cargo.toml`
- `crates/sola-session/src/main.rs` — daemon entry point, event loop
- `crates/sola-session/src/session.rs` — spawn/track/reap + close state machine
- `crates/sola-session/src/env.rs` — WAYLAND_DISPLAY / DISPLAY resolution
- `crates/sola-app/src/bus_registry.rs` — per-topic handler registry
- `apps/shell/src/session.rs` — SessionEntry + session.json I/O

Modified files:
- `crates/sola-bus/src/topics.rs` — new topics, `LaunchApp` shape change, `Topic::kind`
- `crates/sola-bus/src/topic.rs` — extend `define_topics!` to also generate `TopicKind`
- `crates/sola-bus/src/message.rs` — reserved control topic name constants
- `crates/sola-bus/src/main.rs` — subscription filter, roster tracking, Identify/Subscribe handling
- `crates/sola-bus/src/client.rs` — `subscribe()`, `identify()`, auto-identify from `set_app_id`, non-blocking notify write
- `crates/sola-bus/src/lib.rs` — export new API
- `crates/sola-app/src/lib.rs` — SolaApp trait changes, run() wiring
- `crates/sola-app/src/window.rs` — close_request handler returning Propagation::Stop
- `crates/sola/src/main.rs` — remove launch/reap logic, update MANAGED, subscribe to Shutdown only
- `crates/sola-river/src/main.rs` — register_bus conversion
- `crates/sola-river/src/client/manage.rs` — CloseApp dispatch to xdg_toplevel.close
- `apps/shell/src/app.rs` — register_bus conversion, CloseApp handler, session restore, Meta+Q
- `apps/shell/src/keys.rs` — Meta+Q chord
- `apps/shell/src/main.rs` — wire session module
- `apps/terminal/src/*` — register_bus conversion
- `apps/browser/**` — register_bus conversion
- `apps/agent/**` — register_bus conversion
- `apps/monitor/**` — register_bus conversion, subscribe-all
- `applications.json` (shell config) — audit app_id values to match Wayland app_id

Deleted code: `launch_user_app`, `UserApp`, `user_apps`, LaunchApp handling in `sola/src/main.rs`.

---

## Phase 1 — Bus protocol

### Task 1.1: Generate `TopicKind` alongside `Topic`

**Files:**
- Modify: `crates/sola-bus/src/topic.rs`
- Test: `crates/sola-bus/src/topics.rs` (inline tests)

- [ ] **Step 1: Extend the `define_topics!` macro's terminal arm to also emit a `TopicKind` enum and `Topic::kind`.**

In `_define_topics_inner!`, add to the terminal expansion (after the `Topic` enum and before the `impl Topic`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum TopicKind {
    $( $unit, )*
    $( $pname, )*
}

impl TopicKind {
    pub const ALL: &'static [TopicKind] = &[
        $( TopicKind::$unit, )*
        $( TopicKind::$pname, )*
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            $( TopicKind::$unit => stringify!($unit), )*
            $( TopicKind::$pname => stringify!($pname), )*
        }
    }
}
```

And inside the existing `impl Topic`, add:

```rust
pub fn kind(&self) -> TopicKind {
    match self {
        $( Topic::$unit => TopicKind::$unit, )*
        $( Topic::$pname(_) => TopicKind::$pname, )*
    }
}
```

- [ ] **Step 2: Add failing tests at the bottom of `crates/sola-bus/src/topics.rs` inside the existing `tests` module.**

```rust
#[test]
fn topic_kind_matches_variant() {
    let t = Topic::Shutdown;
    assert_eq!(t.kind(), TopicKind::Shutdown);
}

#[test]
fn topic_kind_all_includes_shutdown_and_apps() {
    assert!(TopicKind::ALL.iter().any(|k| k.as_str() == "Shutdown"));
    assert!(TopicKind::ALL.iter().any(|k| k.as_str() == "Apps"));
}
```

- [ ] **Step 3: Run tests.**

```
cargo test -p sola-bus
```

Expected: all existing bus tests pass + new tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-bus/src/topic.rs crates/sola-bus/src/topics.rs
git commit -m "feat(bus): generate TopicKind from define_topics macro"
```

---

### Task 1.2: Change `LaunchApp` shape, add `CloseApp` / `ClientConnected` / `ClientDisconnected`

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`
- Modify: `crates/sola/src/main.rs` (call sites)
- Modify: `apps/shell/src/app.rs` (consumers)

- [ ] **Step 1: Update payload types at the top of `topics.rs`.**

Add a new `LaunchAppPayload` and change `LaunchResultPayload` / `UserAppExitedPayload` to include `app_id`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchAppPayload {
    pub app_id: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchResultPayload {
    pub app_id: String,
    pub command: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAppExitedPayload {
    pub app_id: String,
    pub command: String,
    pub code: Option<i32>,
    pub signal: Option<i32>,
}
```

- [ ] **Step 2: Update the `define_topics!` block.**

Replace the `LaunchApp(String)` line with `LaunchApp(LaunchAppPayload)`, and add three new lines for the new topics. Place the new entries next to related topics:

```
LaunchApp(LaunchAppPayload),
LaunchResult(LaunchResultPayload),
UserAppExited(UserAppExitedPayload),
CloseApp(String),
ClientConnected(String),
ClientDisconnected(String),
```

- [ ] **Step 3: Update `crates/sola/src/main.rs` call sites to use new struct fields.**

Find `Topic::LaunchApp(command)` match arm in the main loop. Replace with:

```rust
Topic::LaunchApp(payload) => {
    launch_user_app(&payload.app_id, &payload.command, &mut user_apps, &mut bus);
}
```

Change `launch_user_app` signature to `fn launch_user_app(app_id: &str, command: &str, ...)`. Update `emit_launch_result` and the `UserAppExited` emission to populate `app_id` + `command` in the new payloads. (These call sites are being deleted in Phase 5, but the crate must build in between.)

- [ ] **Step 4: Update `apps/shell/src/app.rs`.**

Find the two pattern-match arms `Topic::LaunchResult(LaunchResultPayload { command, ok, error })` and `Topic::UserAppExited(UserAppExitedPayload { command, code, signal })`. Expand to include `app_id`:

```rust
Topic::LaunchResult(LaunchResultPayload { app_id, command, ok, error }) => { /* body unchanged */ }
Topic::UserAppExited(UserAppExitedPayload { app_id, command, code, signal }) => { /* body unchanged */ }
```

The bodies don't need to use `app_id` yet — just unpack it.

- [ ] **Step 5: Update `apps/shell/src/app.rs::launch_and_close` to emit the new shape.**

```rust
ctx.emit(Topic::LaunchApp(sola_bus::topics::LaunchAppPayload {
    app_id: app_id.to_string(),
    command: app.command.clone(),
}));
```

- [ ] **Step 6: Build the workspace.**

```
cargo check --workspace
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add -u
git commit -m "feat(bus): add CloseApp/ClientConnected topics, refactor LaunchApp payload"
```

---

### Task 1.3: Define `Subscribe` and `Identify` control messages

**Files:**
- Modify: `crates/sola-bus/src/message.rs`
- Modify: `crates/sola-bus/src/lib.rs`
- Modify: `crates/sola-bus/src/client.rs`

Encoding choice: **reserved topic names**. Control messages are normal `Message`s with topic strings `"$subscribe"` and `"$identify"` and payloads encoded with `postcard`. Transport is unchanged.

- [ ] **Step 1: Add the constants at the top of `message.rs`.**

```rust
/// Reserved topic names for bus control messages. Prefixed `$` so they
/// can never collide with a Topic variant (Rust identifiers don't start
/// with `$`).
pub const CONTROL_SUBSCRIBE: &str = "$subscribe";
pub const CONTROL_IDENTIFY: &str = "$identify";
```

- [ ] **Step 2: Re-export from `lib.rs`.**

```rust
pub use message::{CONTROL_IDENTIFY, CONTROL_SUBSCRIBE, Message};
```

- [ ] **Step 3: Add private encode helpers in `client.rs`.**

Near the top of `client.rs`, add:

```rust
use crate::topic::{decode_payload, encode_payload};
use crate::topics::TopicKind;

fn encode_subscribe(kinds: &[TopicKind]) -> Message {
    Message::with_payload(
        crate::CONTROL_SUBSCRIBE,
        encode_payload(&kinds.to_vec()),
    )
}

fn encode_identify(app_id: &str) -> Message {
    Message::with_payload(
        crate::CONTROL_IDENTIFY,
        encode_payload(&app_id.to_string()),
    )
}
```

- [ ] **Step 4: Add a unit test in `client.rs`.**

```rust
#[cfg(test)]
mod control_encoding_tests {
    use super::*;
    use crate::topic::decode_payload;
    use crate::topics::TopicKind;

    #[test]
    fn subscribe_roundtrip() {
        let m = encode_subscribe(&[TopicKind::Shutdown, TopicKind::LaunchApp]);
        assert_eq!(m.topic, crate::CONTROL_SUBSCRIBE);
        let kinds: Vec<TopicKind> = decode_payload(&m).unwrap();
        assert_eq!(kinds, vec![TopicKind::Shutdown, TopicKind::LaunchApp]);
    }

    #[test]
    fn identify_roundtrip() {
        let m = encode_identify("sola-shell");
        assert_eq!(m.topic, crate::CONTROL_IDENTIFY);
        let id: String = decode_payload(&m).unwrap();
        assert_eq!(id, "sola-shell");
    }
}
```

- [ ] **Step 5: Run tests.**

```
cargo test -p sola-bus
```

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(bus): add Subscribe and Identify control-message encoding"
```

---

### Task 1.4: Bus server — roster + Identify handling

**Files:**
- Modify: `crates/sola-bus/src/main.rs`

- [ ] **Step 1: Extend `BusState`.**

```rust
struct BusState {
    clients: HashMap<ClientId, mpsc::SyncSender<sola_bus::Message>>,
    sticky: HashMap<(String, String), sola_bus::Message>,
    /// client_id → app_id for clients that have sent Identify.
    roster: HashMap<ClientId, String>,
    /// client_id → topic kinds the client has subscribed to.
    subscriptions: HashMap<ClientId, HashSet<sola_bus::topics::TopicKind>>,
}
```

In `main`, default `roster` and `subscriptions` to `HashMap::new()` when constructing the state.

- [ ] **Step 2: Branch on control topics in `handle_client`'s `Ok(Some(event))` arm.**

```rust
Ok(Some(event)) => {
    log_bus_message(id, &event);

    match event.topic.as_str() {
        sola_bus::CONTROL_IDENTIFY => {
            if let Ok(app_id) =
                sola_bus::topic::decode_payload::<String>(&event)
            {
                handle_identify(id, app_id, state);
            }
        }
        sola_bus::CONTROL_SUBSCRIBE => {
            if let Ok(kinds) = sola_bus::topic::decode_payload::<
                Vec<sola_bus::topics::TopicKind>,
            >(&event)
            {
                handle_subscribe(id, kinds, state);
            }
        }
        _ => {
            let mut bus = state.lock().unwrap();
            if event.sticky {
                let key = (event.topic.clone(), event.source.clone());
                bus.sticky.insert(key, event.clone());
            }
            broadcast(id, &event, &mut bus);
        }
    }
}
```

- [ ] **Step 3: Add `handle_identify`.**

```rust
fn handle_identify(id: ClientId, app_id: String, state: &SharedState) {
    let mut bus = state.lock().unwrap();
    let prev = bus.roster.insert(id, app_id.clone());
    if prev.as_ref() == Some(&app_id) {
        return; // already identified — no broadcast
    }
    info!(client = id, %app_id, "identified");
    let evt = sola_bus::topics::Topic::ClientConnected(app_id).to_message();
    broadcast(id, &evt, &mut bus);
}
```

- [ ] **Step 4: Change the client cleanup at the end of `handle_client`.**

Replace `state.lock().unwrap().clients.remove(&id);` with:

```rust
let mut bus = state.lock().unwrap();
if let Some(app_id) = bus.roster.remove(&id) {
    let evt = sola_bus::topics::Topic::ClientDisconnected(app_id).to_message();
    broadcast(id, &evt, &mut bus);
}
bus.clients.remove(&id);
bus.subscriptions.remove(&id);
```

- [ ] **Step 5: Build.**

```
cargo check -p sola-bus
```

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(bus): track client roster, emit ClientConnected/Disconnected on Identify"
```

---

### Task 1.5: Bus server — subscription filter + subscribe replay

**Files:**
- Modify: `crates/sola-bus/src/main.rs`

- [ ] **Step 1: Implement `handle_subscribe`.**

```rust
fn handle_subscribe(
    id: ClientId,
    kinds: Vec<sola_bus::topics::TopicKind>,
    state: &SharedState,
) {
    let new_kinds: HashSet<_> = kinds.into_iter().collect();
    let mut bus = state.lock().unwrap();
    let prev = bus
        .subscriptions
        .insert(id, new_kinds.clone())
        .unwrap_or_default();
    let added: HashSet<_> = new_kinds.difference(&prev).copied().collect();
    info!(
        client = id,
        count = new_kinds.len(),
        added = added.len(),
        "subscribed"
    );

    let Some(tx) = bus.clients.get(&id).cloned() else {
        return;
    };

    // Replay stickies whose kind is newly subscribed.
    for msg in bus.sticky.values() {
        if let Some(kind) = sola_bus::topics::Topic::parse(msg).map(|t| t.kind()) {
            if added.contains(&kind) {
                let _ = tx.try_send(msg.clone());
            }
        }
    }

    // Roster replay for ClientConnected.
    if added.contains(&sola_bus::topics::TopicKind::ClientConnected) {
        for app_id in bus.roster.values() {
            let evt = sola_bus::topics::Topic::ClientConnected(app_id.clone()).to_message();
            let _ = tx.try_send(evt);
        }
    }
}
```

- [ ] **Step 2: Filter `broadcast` by recipient subscription.**

Replace `broadcast` with:

```rust
fn broadcast(sender: ClientId, event: &sola_bus::Message, bus: &mut BusState) {
    let kind = match sola_bus::topics::Topic::parse(event) {
        Some(t) => t.kind(),
        None => {
            warn!(topic = %event.topic, "broadcast dropping unknown topic");
            return;
        }
    };
    for (&id, tx) in bus.clients.iter() {
        if id == sender {
            continue;
        }
        let wants = bus
            .subscriptions
            .get(&id)
            .is_some_and(|s| s.contains(&kind));
        if !wants {
            continue;
        }
        match tx.try_send(event.clone()) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                warn!(client = id, topic = %event.topic, "dropped (queue full)");
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {}
        }
    }
}
```

- [ ] **Step 3: Delete the old sticky-replay-on-connect block.**

In `main`'s `for stream in listener.incoming()` body, remove the `for msg in bus.sticky.values()` loop that replays on connect. Replay now happens on subscribe.

- [ ] **Step 4: Build.**

```
cargo check -p sola-bus
```

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(bus): filter broadcasts by subscription; replay on subscribe"
```

---

### Task 1.6: Integration test — subscription filter + roster replay

**Files:**
- Create: `crates/sola-bus/tests/subscriptions.rs`
- Modify: `crates/sola-bus/Cargo.toml` (tempfile dev-dep)

- [ ] **Step 1: Add `tempfile` dev-dep.**

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write the integration test.**

```rust
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sola_bus::topics::{LaunchAppPayload, Topic, TopicKind};
use sola_bus::BusClient;

fn start_bus(path: PathBuf) -> std::process::Child {
    let exe = env!("CARGO_BIN_EXE_sola-bus");
    std::process::Command::new(exe)
        .env("SOLA_BUS_PATH", &path)
        .env("RUST_LOG", "sola_bus=warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn bus")
}

fn wait_for_socket(path: &PathBuf) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if path.exists() { return; }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("bus socket never appeared");
}

#[test]
fn subscribed_client_receives_filtered_topics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let mut bus = start_bus(path.clone());
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("a");
    a.connect_to(&socket).unwrap();
    a.subscribe(&[TopicKind::Shutdown]).unwrap();

    let mut b = BusClient::new();
    b.set_app_id("b");
    b.connect_to(&socket).unwrap();

    b.emit(Topic::Shutdown).unwrap();
    b.emit(Topic::LaunchApp(LaunchAppPayload {
        app_id: "brave".into(),
        command: "brave".into(),
    })).unwrap();

    let deadline = Instant::now() + Duration::from_millis(500);
    let mut got_shutdown = false;
    let mut got_launch = false;
    while Instant::now() < deadline {
        if let Some(m) = a.recv_timeout(Duration::from_millis(50)) {
            match m.topic.as_str() {
                "Shutdown" => got_shutdown = true,
                "LaunchApp" => got_launch = true,
                _ => {}
            }
        }
    }
    assert!(got_shutdown);
    assert!(!got_launch);

    let _ = bus.kill();
}

#[test]
fn subscribe_replays_roster() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bus");
    let mut bus = start_bus(path.clone());
    wait_for_socket(&path);
    let socket = path.to_string_lossy().to_string();

    let mut a = BusClient::new();
    a.set_app_id("sola-session");
    a.connect_to(&socket).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let mut b = BusClient::new();
    b.set_app_id("b");
    b.connect_to(&socket).unwrap();
    b.subscribe(&[TopicKind::ClientConnected]).unwrap();

    let deadline = Instant::now() + Duration::from_millis(500);
    let mut saw = false;
    while Instant::now() < deadline {
        if let Some(m) = b.recv_timeout(Duration::from_millis(50)) {
            if let Some(Topic::ClientConnected(app_id)) = Topic::parse(&m) {
                if app_id == "sola-session" { saw = true; break; }
            }
        }
    }
    assert!(saw);

    let _ = bus.kill();
}
```

`CARGO_BIN_EXE_sola-bus` is set automatically by cargo for integration tests that depend on a binary target in the same package. No extra Cargo config needed.

- [ ] **Step 3: Run. These tests will fail because `BusClient::subscribe` doesn't yet exist — that's Task 1.7.**

```
cargo test -p sola-bus --test subscriptions
```

Expected: compile error on `a.subscribe(...)`.

- [ ] **Step 4: Commit (failing tests committed intentionally — they gate Task 1.7).**

```bash
git add -u
git commit -m "test(bus): add subscription + roster-replay integration tests (pending 1.7)"
```

---

### Task 1.7: `BusClient` — `subscribe()`, `identify()`, auto-identify

**Files:**
- Modify: `crates/sola-bus/src/client.rs`

- [ ] **Step 1: Add public methods.**

After `emit_sticky`, add:

```rust
pub fn subscribe(&mut self, kinds: &[crate::topics::TopicKind]) -> io::Result<()> {
    let msg = encode_subscribe(kinds);
    self.send(&msg)
}

pub fn identify(&mut self) -> io::Result<()> {
    if self.app_id.is_empty() {
        return Ok(());
    }
    let msg = encode_identify(&self.app_id);
    self.send(&msg)
}
```

- [ ] **Step 2: Make `set_app_id` trigger `Identify`.**

```rust
pub fn set_app_id(&mut self, id: impl Into<String>) {
    self.app_id = id.into();
    let _ = self.identify();
}
```

Since `send` queues when disconnected, the identify is queued until connect.

- [ ] **Step 3: Run the failing tests.**

```
cargo test -p sola-bus --test subscriptions
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "feat(bus): BusClient gains subscribe() and auto-Identify on set_app_id"
```

---

### Task 1.8: Notify-pipe deadlock fix

**Files:**
- Modify: `crates/sola-bus/src/client.rs`

- [ ] **Step 1: Set the notify write end non-blocking.**

In `connect_to`, after creating the socket pair:

```rust
let (notify_read, notify_write) = UnixStream::pair()?;
notify_read.set_nonblocking(true)?;
notify_write.set_nonblocking(true)?; // NEW
```

- [ ] **Step 2: Replace the blocking write in `read_loop` with a non-blocking `write`.**

Change:

```rust
let _ = notify.write_all(&[1u8]);
```

to:

```rust
if let Err(e) = notify.write(&[1u8]) {
    if e.kind() != io::ErrorKind::WouldBlock {
        warn!("notify pipe write error: {e}");
    }
}
```

Use `write` (single byte write, no partial-write concern) rather than `write_all`.

- [ ] **Step 3: Add a unit test verifying non-blocking socket-pair behavior.**

In `client.rs`:

```rust
#[cfg(test)]
mod notify_tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    #[test]
    fn nonblocking_write_does_not_deadlock() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        writer.set_nonblocking(true).unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut written = 0usize;
        let mut blocked = 0usize;
        while Instant::now() < deadline && written < 200_000 {
            match writer.write(&[1u8]) {
                Ok(_) => written += 1,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => blocked += 1,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }

        let mut buf = [0u8; 1024];
        let _ = (&reader).read(&mut buf);

        assert!(blocked > 0);
        assert!(written > 0);
    }
}
```

- [ ] **Step 4: Run.**

```
cargo test -p sola-bus
```

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "fix(bus): non-blocking notify-pipe write prevents reader-thread deadlock"
```

---

## Phase 2 — sola-app handler registration

### Task 2.1: Implement `BusRegistry`

**Files:**
- Create: `crates/sola-app/src/bus_registry.rs`
- Modify: `crates/sola-app/src/lib.rs`
- Modify: `crates/sola-app/src/ctx.rs` (add `shutdown` if missing and a test-only `dummy`)

- [ ] **Step 1: Create the registry.**

```rust
use std::collections::HashMap;

use sola_bus::topics::{Topic, TopicKind};

use crate::ctx::AppCtx;

pub type BusHandler<A> = fn(&mut A, &Topic, &mut AppCtx);

pub struct BusRegistry<A> {
    handlers: HashMap<TopicKind, BusHandler<A>>,
    subscribe_all: bool,
}

impl<A> BusRegistry<A> {
    pub fn new() -> Self {
        Self { handlers: HashMap::new(), subscribe_all: false }
    }

    pub fn on(&mut self, kind: TopicKind, handler: BusHandler<A>) {
        if self.handlers.insert(kind, handler).is_some() {
            if cfg!(debug_assertions) {
                panic!("duplicate bus handler for {:?}", kind);
            } else {
                tracing::warn!(?kind, "duplicate bus handler; last registration wins");
            }
        }
    }

    pub fn subscribe_all(&mut self) {
        self.subscribe_all = true;
    }

    pub fn kinds(&self) -> Vec<TopicKind> {
        if self.subscribe_all {
            TopicKind::ALL.to_vec()
        } else {
            self.handlers.keys().copied().collect()
        }
    }

    pub fn dispatch(&self, topic: &Topic, app: &mut A, ctx: &mut AppCtx) {
        if let Some(handler) = self.handlers.get(&topic.kind()) {
            handler(app, topic, ctx);
        }
    }
}

impl<A> Default for BusRegistry<A> {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 2: Add `AppCtx::shutdown` if missing.**

In `crates/sola-app/src/ctx.rs`:

```rust
impl AppCtx {
    /// Request the app to exit. Triggers `on_shutdown` then GTK quit.
    pub fn shutdown(&mut self) {
        // Existing shutdown plumbing goes here. If there is already an
        // equivalent method (e.g. the handler for Topic::Shutdown), wrap
        // it and keep behavior identical.
    }
}
```

Use whatever the existing shutdown trigger is in `sola-app::run` (search for `gtk::Application::quit` or similar). If `AppCtx::shutdown` already exists, skip this step.

- [ ] **Step 3: Add a test-only constructor.**

```rust
#[cfg(test)]
impl AppCtx {
    pub fn dummy() -> Self {
        // Construct a minimal no-op ctx. Any method call other than those
        // used in unit tests should panic.
        panic!("AppCtx::dummy not yet wired — fill this in when first needed")
    }
}
```

If tests here need to call ctx methods other than `shutdown`, populate as needed.

- [ ] **Step 4: Add unit tests at the bottom of `bus_registry.rs`.**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct TestApp { count: u32, last: Option<String> }

    fn on_close(app: &mut TestApp, topic: &Topic, _ctx: &mut AppCtx) {
        if let Topic::CloseApp(a) = topic {
            app.count += 1;
            app.last = Some(a.clone());
        }
    }

    #[test]
    fn kinds_reflects_registered() {
        let mut reg: BusRegistry<TestApp> = BusRegistry::new();
        reg.on(TopicKind::CloseApp, on_close);
        let kinds = reg.kinds();
        assert!(kinds.contains(&TopicKind::CloseApp));
        assert_eq!(kinds.len(), 1);
    }

    #[test]
    fn subscribe_all_overrides_registered() {
        let mut reg: BusRegistry<TestApp> = BusRegistry::new();
        reg.on(TopicKind::CloseApp, on_close);
        reg.subscribe_all();
        let kinds = reg.kinds();
        assert_eq!(kinds.len(), TopicKind::ALL.len());
    }
}
```

Omit the dispatch test if `AppCtx::dummy` is non-trivial — kinds coverage is enough to prove the API.

- [ ] **Step 5: Wire into `lib.rs`.**

```rust
pub mod bus_registry;
pub use bus_registry::{BusHandler, BusRegistry};
```

- [ ] **Step 6: Run.**

```
cargo test -p sola-app
```

- [ ] **Step 7: Commit**

```bash
git add -u
git commit -m "feat(sola-app): BusRegistry for per-topic handler registration"
```

---

### Task 2.2: `SolaApp` trait — `register_bus` replaces `on_bus_event`

**Files:**
- Modify: `crates/sola-app/src/lib.rs`

- [ ] **Step 1: Replace the trait methods.**

In `trait SolaApp`, remove:

```rust
fn on_bus_event(&mut self, topic: &Topic, ctx: &mut AppCtx) { … }
```

Add:

```rust
fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx)
where
    Self: Sized,
{
    bus.on(TopicKind::CloseApp, Self::on_close_app);
}

fn on_close_app(&mut self, topic: &Topic, ctx: &mut AppCtx)
where
    Self: Sized,
{
    if let Topic::CloseApp(app_id) = topic {
        if app_id == Self::APP_ID {
            ctx.shutdown();
        }
    }
}
```

Keep `on_raw_bus_message`. Keep `on_shutdown`.

- [ ] **Step 2: Wire the registry into `run::<A>`.**

Near the top of `run`, after constructing `app` and `ctx`:

```rust
let mut registry: BusRegistry<A> = BusRegistry::new();
app.register_bus(&mut registry, &mut ctx);
let subscription_kinds = registry.kinds();
```

After `bus.connect()` (or equivalent):

```rust
if let Err(e) = bus.subscribe(&subscription_kinds) {
    tracing::warn!("failed to subscribe: {e}");
}
```

In the incoming-message dispatch, replace the `app.on_bus_event(&topic, ctx)` call with:

```rust
registry.dispatch(&topic, &mut app, &mut ctx);
```

Keep `on_raw_bus_message` called before the registry dispatch.

- [ ] **Step 3: Build the framework.**

```
cargo check -p sola-app
```

Expected: the framework compiles. Every downstream crate that uses `on_bus_event` will now fail to build until Phase 3.

- [ ] **Step 4: Commit (workspace build is intentionally broken until Phase 3).**

```bash
git add -u
git commit -m "feat(sola-app): SolaApp uses BusRegistry; on_bus_event removed"
```

---

### Task 2.3: Block `xdg_toplevel.close` from exiting sola apps

**Files:**
- Modify: `crates/sola-app/src/window.rs`

- [ ] **Step 1: Install a `close_request` handler on each window.**

Find the function that creates the window (look for `gtk4::ApplicationWindow::new` or `Window::builder`). Right after window creation, add:

```rust
use gtk4::glib::Propagation;
use gtk4::prelude::*;
window.connect_close_request(|_win| Propagation::Stop);
```

- [ ] **Step 2: Build.**

```
cargo check -p sola-app
```

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "feat(sola-app): suppress default close_request so only CloseApp exits"
```

---

## Phase 3 — Convert existing apps to register_bus

Each task below follows the same pattern: extract former `on_bus_event` match arms into per-topic methods, then register them via `register_bus`.

### Task 3.1: Convert `sola-shell`

**Files:**
- Modify: `apps/shell/src/app.rs`

- [ ] **Step 1: Inventory arms.**

Locate `fn on_bus_event` in `apps/shell/src/app.rs`. List every `Topic::X =>` arm. Typical set (verify against current code):

- `Apps`, `OutputGeometry`
- `MouseEntered`, `MouseClicked`, `MouseLeft`
- `Chord`, `ChordReleased`
- `LaunchResult`, `UserAppExited`
- `SetAppMenu`, `MenuAction`
- `ClientConnected` (NEW — handler stub for now)

- [ ] **Step 2: Extract each arm into a named method.**

Example for `Apps`:

```rust
fn on_apps(&mut self, topic: &Topic, ctx: &mut AppCtx) {
    let Topic::Apps(apps) = topic else { return };
    // … body unchanged from the former match arm …
}
```

Do this for every arm. Keep bodies identical.

- [ ] **Step 3: Add a stub for `ClientConnected` (filled in by Task 6.2).**

```rust
fn on_client_connected(&mut self, _topic: &Topic, _ctx: &mut AppCtx) {
    // Filled in by Task 6.2 (session restore).
}
```

- [ ] **Step 4: Implement `register_bus`.**

```rust
fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
    // Default CloseApp handler is inherited from the trait.
    bus.on(TopicKind::Apps, Self::on_apps);
    bus.on(TopicKind::OutputGeometry, Self::on_output_geometry);
    bus.on(TopicKind::MouseEntered, Self::on_mouse_entered);
    bus.on(TopicKind::MouseClicked, Self::on_mouse_clicked);
    bus.on(TopicKind::MouseLeft, Self::on_mouse_left);
    bus.on(TopicKind::Chord, Self::on_chord);
    bus.on(TopicKind::ChordReleased, Self::on_chord_released);
    bus.on(TopicKind::LaunchResult, Self::on_launch_result);
    bus.on(TopicKind::UserAppExited, Self::on_user_app_exited);
    bus.on(TopicKind::SetAppMenu, Self::on_set_app_menu);
    bus.on(TopicKind::MenuAction, Self::on_menu_action);
    bus.on(TopicKind::ClientConnected, Self::on_client_connected);
    bus.on(TopicKind::ClientDisconnected, Self::on_client_disconnected);
}
```

Add a stub for `on_client_disconnected` (filled in by Task 6.2):

```rust
fn on_client_disconnected(&mut self, _topic: &Topic, _ctx: &mut AppCtx) {
    // Filled in by Task 6.2.
}
```

- [ ] **Step 5: Delete the old `on_bus_event`.**

- [ ] **Step 6: Build.**

```
cargo check -p sola-shell
```

- [ ] **Step 7: Commit**

```bash
git add -u
git commit -m "refactor(shell): use register_bus for per-topic handlers"
```

---

### Task 3.2: Convert `sola-terminal`

**Files:**
- Modify: `apps/terminal/src/` (SolaApp impl)

Same mechanical pattern as Task 3.1.

- [ ] **Step 1: Inventory `on_bus_event` arms.**
- [ ] **Step 2: Extract each arm into a method.**
- [ ] **Step 3: Implement `register_bus` with one `bus.on(...)` per former arm.**
- [ ] **Step 4: Delete `on_bus_event`.**
- [ ] **Step 5: `cargo check -p sola-terminal`.**
- [ ] **Step 6: Commit**

```bash
git commit -m "refactor(terminal): use register_bus"
```

---

### Task 3.3: Convert `sola-browser` and `sola-agent`

Same pattern as 3.1/3.2. Commit once per crate.

```bash
git commit -m "refactor(browser): use register_bus"
git commit -m "refactor(agent): use register_bus"
```

---

### Task 3.4: Convert `sola-monitor` with subscribe-all

**Files:**
- Modify: `apps/monitor/src/` (SolaApp impl)

- [ ] **Step 1: Leave `on_raw_bus_message` in place.**

- [ ] **Step 2: Implement `register_bus`:**

```rust
fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
    bus.subscribe_all();
    bus.on(TopicKind::CloseApp, Self::on_close_app);
}
```

- [ ] **Step 3: `cargo check -p sola-monitor`.**

- [ ] **Step 4: Commit**

```bash
git commit -m "refactor(monitor): subscribe-all via BusRegistry"
```

---

### Task 3.5: Convert `sola-river` (compositor)

`sola-river` does not use the `sola-app` framework. It owns its own bus client directly.

**Files:**
- Modify: `crates/sola-river/src/main.rs` (or wherever `BusClient` is constructed)

- [ ] **Step 1: Add an explicit `bus.subscribe(&[...])` after connect.**

Read the current bus message dispatch in sola-river. Identify every topic kind it consumes. Add the subscription call. Expected set (audit against current code):

```rust
bus.subscribe(&[
    TopicKind::Composition,
    TopicKind::Frame,
    TopicKind::Focus,
    TopicKind::RegisteredChords,
    TopicKind::Copy,
    TopicKind::Paste,
    TopicKind::Shutdown,
    TopicKind::CloseApp, // NEW — handler added in Phase 7
])?;
```

- [ ] **Step 2: Build.**

```
cargo check -p sola-river
```

- [ ] **Step 3: Commit**

```bash
git commit -m "refactor(river): subscribe explicitly to consumed topics"
```

---

### Task 3.6: Workspace build check

- [ ] **Step 1:**

```
cargo check --workspace && cargo test --workspace
```

Expected: all crates build; all tests pass.

- [ ] **Step 2: Commit any fix-ups.**

---

## Phase 4 — sola-session crate

### Task 4.1: Scaffold

**Files:**
- Create: `crates/sola-session/Cargo.toml`
- Create: `crates/sola-session/src/main.rs`
- Create: `crates/sola-session/src/session.rs`
- Create: `crates/sola-session/src/env.rs`
- Modify: root `Cargo.toml`

- [ ] **Step 1: `Cargo.toml`.**

```toml
[package]
name = "sola-session"
version = "0.1.0"
edition = "2021"

[dependencies]
sola-bus = { path = "../sola-bus" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"
libc = "0.2"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Add `"crates/sola-session"` to the workspace `members` in root `Cargo.toml`.**

- [ ] **Step 3: `main.rs` entry point.**

```rust
mod env;
mod session;

fn main() {
    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola-session.log");
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_session=info".into());
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!("sola-session starting");
    session::run();
}
```

- [ ] **Step 4: Stub `session.rs`.**

```rust
pub fn run() {
    tracing::info!("sola-session event loop (stub)");
}
```

- [ ] **Step 5: Stub `env.rs`.**

```rust
pub fn wayland_socket() -> Option<String> { None }
pub fn x_display() -> Option<String> { None }
```

- [ ] **Step 6: Build.**

```
cargo check -p sola-session
```

- [ ] **Step 7: Commit**

```bash
git add crates/sola-session Cargo.toml
git commit -m "feat(sola-session): scaffold crate"
```

---

### Task 4.2: Env resolution

**Files:**
- Modify: `crates/sola-session/src/env.rs`

- [ ] **Step 1: Copy the current `resolve_wayland_socket` and `resolve_x_display` bodies from `crates/sola/src/main.rs` into `env.rs`, renaming to `wayland_socket` and `x_display`.**

Do not change logic. Both functions read the published socket name file written by sola's river launcher (default paths `/run/user/$UID/sola-wayland` and `/run/user/$UID/sola-display`).

- [ ] **Step 2: Build.**

```
cargo check -p sola-session
```

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "feat(sola-session): env resolution helpers"
```

---

### Task 4.3: `Session` struct + `launch`

**Files:**
- Modify: `crates/sola-session/src/session.rs`

- [ ] **Step 1: Imports and types.**

```rust
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use sola_bus::BusClient;
use sola_bus::topics::{
    LaunchAppPayload, LaunchResultPayload, Topic, TopicKind, UserAppExitedPayload,
};

use crate::env;

#[derive(Debug)]
pub enum CloseState {
    Live,
    Closing { since: Instant },
    Terminated { since: Instant },
    Killed,
}

pub struct ChildRecord {
    pub app_id: String,
    pub command: String,
    pub child: Child,
    pub launched_at: Instant,
    pub state: CloseState,
}

pub struct Session {
    bus: BusClient,
    children: HashMap<String, Vec<ChildRecord>>,
}
```

- [ ] **Step 2: Implement `Session::new` and emit helpers.**

```rust
impl Session {
    pub fn new() -> Self {
        let mut bus = BusClient::new();
        bus.set_app_id("sola-session");
        Self { bus, children: HashMap::new() }
    }

    fn emit_launch_result(&mut self, app_id: &str, command: &str, ok: bool, error: Option<String>) {
        let _ = self.bus.emit(Topic::LaunchResult(LaunchResultPayload {
            app_id: app_id.to_string(),
            command: command.to_string(),
            ok,
            error,
        }));
    }

    fn emit_exited(&mut self, app_id: &str, command: &str, status: std::process::ExitStatus) {
        use std::os::unix::process::ExitStatusExt;
        let payload = UserAppExitedPayload {
            app_id: app_id.to_string(),
            command: command.to_string(),
            code: status.code(),
            signal: status.signal(),
        };
        let _ = self.bus.emit(Topic::UserAppExited(payload));
    }
}
```

- [ ] **Step 3: Implement `launch`. Reuse the existing spawn logic from `crates/sola/src/main.rs:228-290` verbatim — same env handling, same PDEATHSIG call, same error paths — just port it into this method, using the new struct fields. The only differences from the current code:**

- Record is stored in `self.children[app_id]` instead of a flat `Vec<UserApp>`.
- `emit_launch_result` uses the new payload type.

Pseudo-outline (write out in full in the actual file; this is a guide, not a substitute):

```rust
impl Session {
    pub fn launch(&mut self, payload: LaunchAppPayload) {
        info!(app_id = %payload.app_id, command = %payload.command, "launch");
        // - Parse command into program + args (same as current launch_user_app).
        // - Build Command, set WAYLAND_DISPLAY (env::wayland_socket), DISPLAY (env::x_display).
        // - Same unsafe pre_exec block with prctl(PR_SET_PDEATHSIG, SIGTERM) as in
        //   crates/sola/src/main.rs:264-271.
        // - On success: push a ChildRecord into self.children[app_id], emit_launch_result(ok=true).
        // - On failure: emit_launch_result(ok=false, error=e.to_string()).
    }
}
```

- [ ] **Step 4: Build.**

```
cargo check -p sola-session
```

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(sola-session): Session struct and LaunchApp handler"
```

---

### Task 4.4: Reaping

**Files:**
- Modify: `crates/sola-session/src/session.rs`

- [ ] **Step 1: Add `tick` and `reap_exited`.**

```rust
impl Session {
    pub fn tick(&mut self) {
        self.reap_exited();
        self.run_close_timers();
    }

    fn reap_exited(&mut self) {
        let mut to_emit: Vec<(String, String, std::process::ExitStatus)> = Vec::new();
        for (_app_id, records) in self.children.iter_mut() {
            records.retain_mut(|r| {
                match r.child.try_wait() {
                    Ok(Some(status)) => {
                        to_emit.push((r.app_id.clone(), r.command.clone(), status));
                        false
                    }
                    Ok(None) => true,
                    Err(e) => {
                        warn!(app_id = %r.app_id, pid = r.child.id(), %e, "try_wait failed");
                        true
                    }
                }
            });
        }
        self.children.retain(|_, v| !v.is_empty());
        for (app_id, command, status) in to_emit {
            info!(%app_id, ?status, "user app exited");
            self.emit_exited(&app_id, &command, status);
        }
    }

    fn run_close_timers(&mut self) {
        // Filled in by Task 4.5.
    }
}
```

- [ ] **Step 2: Build.**

```
cargo check -p sola-session
```

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "feat(sola-session): reap children and emit UserAppExited"
```

---

### Task 4.5: Close state machine

**Files:**
- Modify: `crates/sola-session/src/session.rs`

- [ ] **Step 1: Constants and `close`.**

```rust
const GRACEFUL: Duration = Duration::from_secs(5);
const FORCE_AFTER_TERM: Duration = Duration::from_secs(5);

impl Session {
    pub fn close(&mut self, app_id: &str) {
        let Some(records) = self.children.get_mut(app_id) else {
            info!(%app_id, "CloseApp: no live children");
            return;
        };
        for r in records.iter_mut() {
            if matches!(r.state, CloseState::Live) {
                info!(%app_id, pid = r.child.id(), "CloseApp: graceful period started");
                r.state = CloseState::Closing { since: Instant::now() };
            }
        }
    }
}
```

- [ ] **Step 2: Implement `run_close_timers`.**

```rust
impl Session {
    fn run_close_timers(&mut self) {
        let now = Instant::now();
        for (_app_id, records) in self.children.iter_mut() {
            for r in records.iter_mut() {
                match r.state {
                    CloseState::Closing { since } if now.duration_since(since) >= GRACEFUL => {
                        info!(pid = r.child.id(), app_id = %r.app_id, "sending SIGTERM");
                        unsafe { libc::kill(r.child.id() as i32, libc::SIGTERM); }
                        r.state = CloseState::Terminated { since: now };
                    }
                    CloseState::Terminated { since } if now.duration_since(since) >= FORCE_AFTER_TERM => {
                        info!(pid = r.child.id(), app_id = %r.app_id, "sending SIGKILL");
                        unsafe { libc::kill(r.child.id() as i32, libc::SIGKILL); }
                        r.state = CloseState::Killed;
                    }
                    _ => {}
                }
            }
        }
    }
}
```

- [ ] **Step 3: Wire `handle` and the event loop.**

```rust
impl Session {
    pub fn handle(&mut self, topic: Topic) {
        match topic {
            Topic::LaunchApp(p) => self.launch(p),
            Topic::CloseApp(app_id) => self.close(&app_id),
            Topic::Shutdown => std::process::exit(0),
            _ => {}
        }
    }
}

pub fn run() {
    let mut session = Session::new();
    loop {
        match session.bus.connect() {
            Ok(()) => break,
            Err(e) => {
                warn!(%e, "bus connect failed, retrying");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    let _ = session.bus.subscribe(&[
        TopicKind::LaunchApp,
        TopicKind::CloseApp,
        TopicKind::Shutdown,
    ]);

    info!("sola-session connected to bus");
    let poll = Duration::from_millis(500);
    loop {
        while let Some(msg) = session.bus.try_recv() {
            if let Some(topic) = Topic::parse(&msg) {
                session.handle(topic);
            }
        }
        if let Some(msg) = session.bus.recv_timeout(poll) {
            if let Some(topic) = Topic::parse(&msg) {
                session.handle(topic);
            }
        }
        session.tick();
    }
}
```

- [ ] **Step 4: Build.**

```
cargo check -p sola-session
```

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(sola-session): close state machine (5s SIGTERM, +5s SIGKILL)"
```

---

### Task 4.6: Integration test — spawn + close + reap

**Files:**
- Create: `crates/sola-session/tests/session_lifecycle.rs`

- [ ] **Step 1: Write the test.**

```rust
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sola_bus::topics::{LaunchAppPayload, Topic, TopicKind};
use sola_bus::BusClient;

fn start_bus(path: PathBuf) -> std::process::Child {
    let exe = env!("CARGO_BIN_EXE_sola-bus");
    std::process::Command::new(exe)
        .env("SOLA_BUS_PATH", &path)
        .spawn().unwrap()
}

fn start_session(bus_path: PathBuf) -> std::process::Child {
    let exe = env!("CARGO_BIN_EXE_sola-session");
    std::process::Command::new(exe)
        .env("SOLA_BUS_PATH", &bus_path)
        .spawn().unwrap()
}

#[test]
fn spawn_and_close_sleep() {
    let dir = tempfile::tempdir().unwrap();
    let bus_path = dir.path().join("bus");
    let mut bus = start_bus(bus_path.clone());
    while !bus_path.exists() { std::thread::sleep(Duration::from_millis(20)); }
    let mut session = start_session(bus_path.clone());

    let socket = bus_path.to_string_lossy().to_string();
    let mut client = BusClient::new();
    client.set_app_id("test");
    client.connect_to(&socket).unwrap();
    client.subscribe(&[
        TopicKind::LaunchResult,
        TopicKind::UserAppExited,
        TopicKind::ClientConnected,
    ]).unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut session_ready = false;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(100)) {
            if let Some(Topic::ClientConnected(a)) = Topic::parse(&m) {
                if a == "sola-session" { session_ready = true; break; }
            }
        }
    }
    assert!(session_ready);

    client.emit(Topic::LaunchApp(LaunchAppPayload {
        app_id: "sleep".into(),
        command: "/usr/bin/sleep 60".into(),
    })).unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut launched = false;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(100)) {
            if let Some(Topic::LaunchResult(p)) = Topic::parse(&m) {
                assert!(p.ok);
                launched = true;
                break;
            }
        }
    }
    assert!(launched);

    client.emit(Topic::CloseApp("sleep".into())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut exited = false;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(100)) {
            if matches!(Topic::parse(&m), Some(Topic::UserAppExited(_))) {
                exited = true;
                break;
            }
        }
    }
    assert!(exited);

    let _ = session.kill();
    let _ = bus.kill();
}
```

- [ ] **Step 2: Run.**

```
cargo test -p sola-session --test session_lifecycle
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "test(sola-session): spawn+close+reap integration test"
```

---

## Phase 5 — sola crate cleanup

### Task 5.1: Remove user-app handling

**Files:**
- Modify: `crates/sola/src/main.rs`

- [ ] **Step 1: Delete the symbols and all references.**

- `struct UserApp`
- `fn launch_user_app(...)`
- `fn reap_user_apps(...)`
- `fn emit_launch_result(...)`
- `let mut user_apps: Vec<UserApp>` binding
- The `Topic::LaunchApp(...)` match arm
- The call to `reap_user_apps(...)` in the main loop
- `fn resolve_wayland_socket`, `fn resolve_x_display` — if only user-app spawn used them, delete; otherwise keep.

- [ ] **Step 2: Subscribe only to Shutdown.**

In the main loop, after `bus.connect()` succeeds for the first time:

```rust
let _ = bus.subscribe(&[sola_bus::topics::TopicKind::Shutdown]);
```

Important: without this call, sola will receive no bus messages at all after Phase 1.

- [ ] **Step 3: Build.**

```
cargo check -p sola
```

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "refactor(sola): drop user-app spawn/reap; subscribe to Shutdown only"
```

---

### Task 5.2: Update `MANAGED`

**Files:**
- Modify: `crates/sola/src/main.rs`

- [ ] **Step 1: Replace the `MANAGED` slice.**

```rust
const MANAGED: &[&str] = &[
    "sola-bus",
    "sola-river",
    "sola-shell",
    "sola-session",
];
```

`sola-terminal` is removed from the list — it becomes a session-restored user app.

- [ ] **Step 2: Build.**

```
cargo check -p sola
```

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "feat(sola): manage sola-session; terminal becomes a user app"
```

---

## Phase 6 — Shell: session persistence + Meta+Q

### Task 6.1: Session module — load / save / types

**Files:**
- Create: `apps/shell/src/session.rs`
- Modify: `apps/shell/src/main.rs` (`mod session;`)

- [ ] **Step 1: Create `session.rs`.**

```rust
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sola_bus::topics::Zone;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEntry {
    pub app_id: String,
    pub zone: Zone,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedSession {
    pub entries: Vec<PersistedEntry>,
}

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub app_id: String,
    pub zone: Zone,
    pub window_id: Option<u32>,
}

impl SessionEntry {
    pub fn persisted(&self) -> PersistedEntry {
        PersistedEntry { app_id: self.app_id.clone(), zone: self.zone }
    }
}

pub fn state_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("{home}/.local/state")));
    base.join("sola").join("session.json")
}

pub fn load() -> Vec<SessionEntry> {
    let path = state_file();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!(%e, "session.json read failed");
            return Vec::new();
        }
    };
    match serde_json::from_slice::<PersistedSession>(&bytes) {
        Ok(s) => s.entries.into_iter().map(|e| SessionEntry {
            app_id: e.app_id, zone: e.zone, window_id: None,
        }).collect(),
        Err(e) => {
            tracing::warn!(%e, "session.json parse failed; backing up");
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let backup = path.with_extension(format!("json.bak-{ts}"));
            let _ = std::fs::rename(&path, &backup);
            Vec::new()
        }
    }
}

pub fn save(entries: &[SessionEntry]) {
    let path = state_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let persisted = PersistedSession {
        entries: entries.iter().map(|e| e.persisted()).collect(),
    };
    let Ok(bytes) = serde_json::to_vec_pretty(&persisted) else {
        tracing::warn!("session.json serialize failed");
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        tracing::warn!(%e, "session.json write failed");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        tracing::warn!(%e, "session.json rename failed");
    }
}
```

- [ ] **Step 2: Add `mod session;` to `apps/shell/src/main.rs`.**

- [ ] **Step 3: Add a unit test at the bottom of `session.rs`.**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_roundtrip() {
        let entries = vec![
            SessionEntry { app_id: "a".into(), zone: Zone::Top, window_id: None },
            SessionEntry { app_id: "b".into(), zone: Zone::Bottom, window_id: Some(7) },
        ];
        let persisted = PersistedSession {
            entries: entries.iter().map(|e| e.persisted()).collect(),
        };
        let json = serde_json::to_string(&persisted).unwrap();
        let back: PersistedSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entries.len(), 2);
        assert_eq!(back.entries[0].app_id, "a");
        assert_eq!(back.entries[1].zone, Zone::Bottom);
    }
}
```

- [ ] **Step 4: Build & test.**

```
cargo test -p sola-shell
```

- [ ] **Step 5: Commit**

```bash
git add apps/shell/src/session.rs apps/shell/src/main.rs
git commit -m "feat(shell): SessionEntry and session.json load/save"
```

---

### Task 6.2: Restore loop + ClientConnected handler

**Files:**
- Modify: `apps/shell/src/app.rs`

- [ ] **Step 1: Add fields to `ShellApp`.**

```rust
use crate::session::{self, SessionEntry};

pub struct ShellApp {
    // … existing fields …
    pub session_entries: Vec<SessionEntry>,
}
```

Initialize in `new`:

```rust
session_entries: session::load(),
```

No `session_restored` flag: the restore loop only launches *pending*
entries (`window_id: None`), so re-firing is naturally idempotent.

- [ ] **Step 2: Replace the `on_client_connected` stub from Task 3.1.**

```rust
fn on_client_connected(&mut self, topic: &Topic, ctx: &mut AppCtx) {
    let Topic::ClientConnected(app_id) = topic else { return };
    if app_id != "sola-session" {
        return;
    }

    // Iterate pending entries (window_id: None). Live entries were matched
    // to windows already; no need to relaunch.
    let mut launches: Vec<(String, String)> = Vec::new();
    self.session_entries.retain(|e| {
        if e.window_id.is_some() {
            return true; // live — keep, don't relaunch
        }
        match self.applications.get(&e.app_id) {
            Some(app) => {
                launches.push((e.app_id.clone(), app.command.clone()));
                true
            }
            None => {
                tracing::warn!(app_id = %e.app_id, "session entry not in applications.json; pruning");
                false
            }
        }
    });

    for (app_id, command) in launches {
        tracing::info!(%app_id, "restoring session app");
        ctx.emit(Topic::LaunchApp(sola_bus::topics::LaunchAppPayload {
            app_id, command,
        }));
    }
    session::save(&self.session_entries);
}
```

- [ ] **Step 3: Also register a `ClientDisconnected` handler to demote all live entries to pending when `sola-session` dies.**

Register in `register_bus` alongside `ClientConnected`:

```rust
bus.on(TopicKind::ClientDisconnected, Self::on_client_disconnected);
```

Implementation:

```rust
fn on_client_disconnected(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
    let Topic::ClientDisconnected(app_id) = topic else { return };
    if app_id != "sola-session" {
        return;
    }
    tracing::warn!("sola-session disconnected; demoting live entries to pending");
    for e in self.session_entries.iter_mut() {
        e.window_id = None;
    }
    session::save(&self.session_entries);
}
```

On reconnect, `on_client_connected` relaunches them via the pending path.

- [ ] **Step 3: Build.**

```
cargo check -p sola-shell
```

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "feat(shell): restore session on ClientConnected(sola-session)"
```

---

### Task 6.3: Window map / zone change / close reconciliation

**Files:**
- Modify: `apps/shell/src/app.rs`

- [ ] **Step 1: Add a reconcile method.**

```rust
fn reconcile_session_entries(&mut self) {
    use std::collections::HashSet;
    let current: HashSet<(String, u32)> = self.known_windows
        .iter()
        .map(|w| (w.app_id.clone(), w.window_id))
        .collect();

    // Window-vanish policy: demote (not remove). Removal is gated on
    // UserAppExited (see on_user_app_exited). This keeps entries
    // recoverable when sola-session dies and takes windows down via PDEATHSIG.
    for e in self.session_entries.iter_mut() {
        if let Some(wid) = e.window_id {
            if !current.contains(&(e.app_id.clone(), wid)) {
                e.window_id = None;
            }
        }
    }

    // For each window, claim a pending entry or create a new one.
    let windows = self.known_windows.clone();
    for w in windows {
        let already = self.session_entries.iter().any(|e| e.window_id == Some(w.window_id));
        if already { continue; }
        let pending_idx = self.session_entries.iter().position(|e|
            e.app_id == w.app_id && e.window_id.is_none()
        );
        match pending_idx {
            Some(i) => {
                self.session_entries[i].window_id = Some(w.window_id);
                let zone = self.session_entries[i].zone;
                self.zoning.assign(w.window_id, zone);
            }
            None => {
                let zone = self.zoning.default_zone_for(&w.app_id);
                self.session_entries.push(SessionEntry {
                    app_id: w.app_id.clone(),
                    zone,
                    window_id: Some(w.window_id),
                });
                self.zoning.assign(w.window_id, zone);
            }
        }
    }

    session::save(&self.session_entries);
}
```

If `self.zoning` does not expose `assign(window_id, zone)` or `default_zone_for(app_id)` in the current codebase, adapt to the existing API. The important invariants:

1. Pending entries with matching `app_id` get claimed in FIFO order.
2. The saved `zone` is applied when a pending entry is claimed.
3. `session::save` runs after every mutation.

- [ ] **Step 2: Call `reconcile_session_entries` at the end of `on_apps`.**

- [ ] **Step 2a: Remove entries on `UserAppExited`.**

Wire this into the existing `on_user_app_exited` handler (Task 3.1):

```rust
fn on_user_app_exited(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
    let Topic::UserAppExited(p) = topic else { return };
    // … existing body (toast / logging) …

    // Prefer removing a live entry for this app_id; fall back to pending.
    let idx = self.session_entries.iter()
        .position(|e| e.app_id == p.app_id && e.window_id.is_some())
        .or_else(|| self.session_entries.iter().position(|e| e.app_id == p.app_id));
    if let Some(i) = idx {
        self.session_entries.remove(i);
        session::save(&self.session_entries);
    }
}
```

- [ ] **Step 3: Hook zone changes.**

Find the existing code path that updates a window's zone (search for `Zone::` assignments in `zoning.rs` or `keys.rs`). After each change:

```rust
fn update_entry_zone(&mut self, window_id: u32, zone: Zone) {
    if let Some(e) = self.session_entries.iter_mut().find(|e| e.window_id == Some(window_id)) {
        if e.zone != zone {
            e.zone = zone;
            session::save(&self.session_entries);
        }
    }
}
```

Call it from every zone-assignment path.

- [ ] **Step 4: Build.**

```
cargo check -p sola-shell
```

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(shell): reconcile session entries on window map/close/zone change"
```

---

### Task 6.4: Meta+Q binding

**Files:**
- Modify: `apps/shell/src/app.rs`
- Modify: `apps/shell/src/keys.rs`

- [ ] **Step 1: Add Meta+Q to the registered chord list.**

In `shell_key_chords` (in `app.rs`), add:

```rust
bindings.push(sola_core::KeyCode::Q.meta());
```

- [ ] **Step 2: Dispatch the chord.**

In `keys::handle_chord`, add:

```rust
if evt.matches(sola_core::KeyCode::Q.meta()) {
    tracing::info!("Meta+Q — close focused app");
    app.close_focused_app(ctx);
    return;
}
```

(Use whichever matching API the existing chord dispatcher uses.)

- [ ] **Step 3: Implement `close_focused_app` on `ShellApp`.**

```rust
pub fn close_focused_app(&mut self, ctx: &mut AppCtx) {
    if self.launcher.active || self.switcher.active || self.menu_open {
        return;
    }
    let Some(wid) = self.focused_window_id else { return };
    let Some(win) = self.known_windows.iter().find(|w| w.window_id == wid) else { return };
    let app_id = win.app_id.clone();
    if app_id == Self::APP_ID {
        // Don't let Meta+Q close the shell itself.
        return;
    }
    tracing::info!(%app_id, "emitting CloseApp");
    ctx.emit(Topic::CloseApp(app_id));
}
```

- [ ] **Step 4: Build.**

```
cargo check -p sola-shell
```

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(shell): Meta+Q emits CloseApp for focused window"
```

---

## Phase 7 — Compositor: CloseApp dispatch

### Task 7.1: Subscribe + send `xdg_toplevel.close`

**Files:**
- Modify: `crates/sola-river/src/client/manage.rs` (or current home of toplevel dispatch)

- [ ] **Step 1: Confirm `CloseApp` is in the subscription list.**

Added in Task 3.5. Verify.

- [ ] **Step 2: Add a handler.**

In the message dispatch arm where other topics are handled:

```rust
Topic::CloseApp(app_id) => {
    let mut count = 0;
    for toplevel in self.mapped_toplevels_for_app(&app_id) {
        toplevel.send_close();
        count += 1;
    }
    tracing::info!(%app_id, count, "CloseApp: sent xdg_toplevel.close");
}
```

Replace `mapped_toplevels_for_app` and `send_close` with the current sola-river API equivalents. The key behaviors:

- Iterate the compositor's toplevel registry.
- For each toplevel whose Wayland `app_id` equals the topic's app_id, send `xdg_toplevel.close`.
- Log the count at info level.

- [ ] **Step 3: Build.**

```
cargo check -p sola-river
```

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "feat(river): forward CloseApp to xdg_toplevel.close"
```

---

## Phase 8 — Integration verification

### Task 8.1: `applications.json` audit

**Files:**
- Modify: `applications.json` (wherever the shell loads it — `config/`, `/etc/sola/`, or `~/.config/sola/shell/`)

- [ ] **Step 1: Build & deploy.**

```
cargo make build
cargo make deploy
```

- [ ] **Step 2: Inside a running session, launch each configured app via the launcher once. In `/opt/sola/log/sola-river.log` (or wherever on-map events land), grep for each app's Wayland `app_id`.**

Example check:

```
grep -E "window mapped|app_id" /opt/sola/log/sola-river.log | tail -50
```

If mapping doesn't already log `app_id`, add a one-line `tracing::info!(%app_id, window_id, "window mapped");` at the top of the river on-map handler, rebuild, redeploy, then audit.

- [ ] **Step 3: For each mismatch, update `applications.json`'s `app_id` field to match the logged Wayland `app_id`. Commit.**

```bash
git commit -m "chore(shell): align applications.json app_ids with Wayland app_ids"
```

---

### Task 8.2: End-to-end smoke

- [ ] **Step 1: Full rebuild & deploy.**

```
cargo make build && cargo make deploy
```

- [ ] **Step 2: From a TTY, run `sola`. Check each case below in order:**

- [ ] Meta+Space opens launcher; launching brave spawns a Brave window (check `/opt/sola/log/sola-session.log` for `user app launched`).
- [ ] Move a window to a zone with Meta+Numpad. Check `~/.local/state/sola/session.json` — the zone appears for that entry.
- [ ] Meta+Q on a non-sola window: window closes; `sola-session.log` shows `user app exited`. Session file drops that entry.
- [ ] Meta+Q on sola-terminal: terminal exits (triggers `on_close_app` → `ctx.shutdown` → terminal's `on_shutdown` persists tabs).
- [ ] Kill `sola-session` manually (`pkill sola-session`); sola restarts it; shell's `ClientConnected("sola-session")` handler re-fires; apps come back.
- [ ] Normal sola exit + restart: full session restored with zones.
- [ ] Inspect `/opt/sola/log/sola.log` for `dropped (queue full)` warnings — should be gone or greatly reduced.

- [ ] **Step 3: For any failing case, open a root-cause investigation ticket before patching.**

- [ ] **Step 4: After all smoke cases pass, final commit.**

```bash
git commit --allow-empty -m "chore: sola-session smoke pass"
```

---

## Self-Review Notes

- **Spec coverage**
  - Bus subscriptions → Tasks 1.1–1.7
  - Notify-pipe fix → Task 1.8
  - Handler registration + close_request block → Tasks 2.1–2.3
  - Existing app conversion → Phase 3
  - sola-session crate → Phase 4
  - sola cleanup → Phase 5
  - Shell persist/restore + Meta+Q → Phase 6
  - Compositor CloseApp dispatch → Phase 7
  - applications.json invariant → Task 8.1

- **Type consistency**
  - `LaunchAppPayload`, `LaunchResultPayload`, `UserAppExitedPayload` all carry `app_id` + `command` in every task that references them.
  - `TopicKind` emitted by macro; `TopicKind::ALL` and `Topic::kind` are used consistently in server, client, and registry.
  - `CloseApp` is `String` (the Wayland app_id) in every subscriber.

- **Deferred decisions (intentional)**
  - sola-session event-loop primitive: 500ms tick + `recv_timeout`. Fine for MVP; can graduate to `calloop`/`glib::MainLoop` later.
  - Control-message encoding: reserved topic names `$subscribe` / `$identify`.
  - `TopicKind::ALL`: macro-generated.
