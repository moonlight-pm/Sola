# Sola Browser Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `apps/browser/` into `crates/sola-browser/`, migrate its persistence to bus topics with a new `namespace` attribute, and add `Delivery::source` so apps can filter self-echoes.

**Architecture:** Workspace-first relocation; two small generic `sola-bus` enhancements (per-topic namespace files with multi-key path interpolation, plus `Delivery::source`); browser switches from `JsonConfig` to keyed persistent topics mirroring `sola-terminal`'s pattern; one-shot migrator preserves existing on-disk JSON.

**Tech Stack:** Rust 2024, GTK4, WebKit6, sola-bus (TOML state), sola-app (`SolaApp` trait), Arrow.js (frontend, untouched).

---

## File Structure

### Created
- `crates/sola-browser/` — moved from `apps/browser/`. Internal layout unchanged: `src/{main,app,chrome,state,tabs}.rs`, `web/...`.
- `crates/sola-browser/src/migrate.rs` — one-shot legacy JSON migrator, separate from `state.rs` so it can be excised in a follow-up.

### Modified
- `crates/sola-bus/src/registry.rs` — `Delivery` gains `source: &'a str`.
- `crates/sola-bus/src/topic.rs` — `define_topics!` macro grows `namespace = "..."` syntax; generates `TopicKind::namespace()` and `TopicKind::key_names()`; emits `Topic::path_for()`.
- `crates/sola-bus/src/state.rs` — load/write/retract route by `Topic::path_for()`; namespaced files store the payload alone (no section header).
- `crates/sola-bus/src/topics.rs` — adds `BrowserTab`, `BrowserConfig`, `BrowserHistory`, `HistoryEntry`.
- `crates/sola-app/src/lib.rs:367` — populate `Delivery.source` from `msg.source`.
- `crates/sola-browser/Cargo.toml` — relative path deps adjusted.
- `crates/sola-browser/src/state.rs` — `TabStore` / `BrowsingHistory` shrink to in-memory caches over bus topics; `JsonConfig` impls deleted.
- `crates/sola-browser/src/app.rs` — handler signatures use `&Delivery`; `realize_active`; persistence triggers via `ctx.emit` / `ctx.retract`.
- `CLAUDE.md` — drop `browser/` from `apps/` listing, add `sola-browser/` to `crates/` listing.

### Deleted
- `apps/browser/Cargo.lock` — superseded by workspace lockfile.

---

## Task 1: Relocate apps/browser → crates/sola-browser; restore green build

The browser predates the `Delivery` wrapper and won't compile against current `sola-bus`. The minimum shape that builds is: move + path fix + handler signatures.

**Files:**
- Move: `apps/browser/` → `crates/sola-browser/`
- Modify: `crates/sola-browser/Cargo.toml`
- Modify: `crates/sola-browser/src/app.rs` (handler signatures)
- Delete: `apps/browser/Cargo.lock`
- Modify: `crates/sola-browser/src/main.rs` (path comments only — `include_str!("../web/...")` stays)

- [ ] **Step 1: Move the directory**

```bash
git mv apps/browser crates/sola-browser
```

- [ ] **Step 2: Fix Cargo.toml dependency paths**

In `crates/sola-browser/Cargo.toml`, change three lines:

```toml
sola-app = { path = "../sola-app" }
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
```

(was `../../crates/sola-app`, etc.)

- [ ] **Step 3: Drop the stale lockfile**

```bash
rm crates/sola-browser/Cargo.lock
```

- [ ] **Step 4: Update browser bus handler signatures**

In `crates/sola-browser/src/app.rs`, change every handler whose signature is `(&mut self, topic: &Topic, ctx: &mut AppCtx)` to `(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx)`. There are two: `on_menu_action` and `on_open_url`. In each body, replace bare `topic` references with `delivery.topic`. For example:

Before:
```rust
fn on_menu_action(&mut self, topic: &Topic, ctx: &mut AppCtx) {
    let Topic::MenuAction(action) = topic else { return };
    // ...
}
```

After:
```rust
fn on_menu_action(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
    let Topic::MenuAction(action) = delivery.topic else { return };
    // ...
}
fn on_open_url(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
    let Topic::OpenUrl(req) = delivery.topic else { return };
    // ...
}
```

Confirm exact register_bus call sites still typecheck — `BusHandler<A, AppCtx>` is `fn(&mut A, &Delivery, &mut AppCtx)`.

- [ ] **Step 5: Build**

```bash
cargo make build
```

Expected: full workspace builds clean. `sola-browser` compiles.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(sola-browser): relocate apps/browser to crates/, align with Delivery handler signature

Workspace member auto-enrolls via members = [\"crates/*\"]. Handlers take
&Delivery; bodies now read delivery.topic. Persistence is unchanged at
this step (still JsonConfig)."
```

---

## Task 2: Add `Delivery::source`

Plumb `Message::source` through to handlers so apps can identify self-echoes.

**Files:**
- Modify: `crates/sola-bus/src/registry.rs` — add `source` field
- Modify: `crates/sola-app/src/lib.rs:367` — populate it from `msg.source`
- Test: `crates/sola-bus/src/registry.rs` (existing dispatch tests)

- [ ] **Step 1: Update existing dispatch tests to set source**

In `crates/sola-bus/src/registry.rs` tests at the bottom, the two `Delivery { topic, retracted }` literals need a `source: ""` field. Mechanical edit; verifies the new field via the type checker.

- [ ] **Step 2: Run tests — expect they fail to compile**

```bash
cargo test -p sola-bus registry::tests
```

Expected: compile error "missing field `source` in initializer of `Delivery`". Confirms the test now requires the new field.

- [ ] **Step 3: Add the `source` field to `Delivery`**

In `crates/sola-bus/src/registry.rs:10`:

```rust
/// A bus delivery to a subscriber. Wraps the parsed `Topic` with a
/// `retracted` flag so handlers for `#[sticky]` / `#[persistent]` topic
/// kinds can branch on add/remove. For ephemeral topic kinds,
/// `retracted` is always `false`.
///
/// `source` is the emitter's `app_id`. Restored stickies replayed at
/// bus startup carry `source = "sola-bus"` (the `BUS_SOURCE` constant).
/// Apps that emit and subscribe to the same topic can filter their own
/// self-echo by comparing `source` to their `APP_ID`.
#[derive(Debug)]
pub struct Delivery<'a> {
    pub topic: &'a Topic,
    pub retracted: bool,
    pub source: &'a str,
}
```

- [ ] **Step 4: Add a new test asserting source is propagated**

In `crates/sola-bus/src/registry.rs` tests, append:

```rust
#[test]
fn dispatch_passes_source() {
    fn capture(app: &mut TestApp, delivery: &Delivery, _ctx: &mut TestCtx) {
        app.last = Some(delivery.topic.kind());
        app.last_source = delivery.source.to_string();
    }
    let mut reg: BusRegistry<TestApp, TestCtx> = BusRegistry::new();
    reg.on(TopicKind::Shutdown, capture);
    let mut app = empty_app();
    let mut ctx = TestCtx;
    let topic = Topic::Shutdown;
    let delivery = Delivery {
        topic: &topic,
        retracted: false,
        source: "test-app",
    };
    reg.dispatch(&delivery, &mut app, &mut ctx);
    assert_eq!(app.last_source, "test-app");
}
```

Add `pub last_source: String` to the `TestApp` struct in the same file (search for `struct TestApp`) and initialize it `String::new()` in `empty_app()`.

- [ ] **Step 5: Run tests**

```bash
cargo test -p sola-bus registry::tests
```

Expected: all pass.

- [ ] **Step 6: Plumb source at the dispatch site**

In `crates/sola-app/src/lib.rs:367`:

```rust
let retracted = topic.kind().behavior().is_sticky() && !msg.sticky;
let delivery = sola_bus::Delivery { topic: &topic, retracted, source: &msg.source };
registry.dispatch(&delivery, app, ctx);
```

- [ ] **Step 7: Verify workspace build**

```bash
cargo make build
```

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(sola-bus,sola-app): plumb Message::source through Delivery

Handlers can now identify self-echoes by comparing Delivery::source to
their own APP_ID. Restored stickies on startup carry the BUS_SOURCE
sentinel (\"sola-bus\"), distinct from any app emitter."
```

---

## Task 3: Add `namespace` attribute to `define_topics!` macro

Extends the macro grammar with `#[persistent(namespace = "…")]` and `#[persistent(keys = […], namespace = "…")]`. Generates `TopicKind::namespace()` and `TopicKind::key_names()` accessors. Path resolution itself comes in Task 4.

The internal accumulator for persistent payloads grows from
`$pp($ppt) keys=[$($ppk)*]` to `$pp($ppt) keys=[$($ppk)*] ns=$ns`,
where `$ns` is either an empty group `()` (no namespace) or a string literal `("…")`. Emit-side codegen branches on `ns` shape.

**Files:**
- Modify: `crates/sola-bus/src/topic.rs`

- [ ] **Step 1: Write failing tests in topic.rs tests module**

Append to the existing `#[cfg(test)] mod tests` near the bottom of `topic.rs`:

```rust
crate::define_topics! {
    Plain2(String),
    #[persistent]
    Singleton(StickyKeyed),
    #[persistent(namespace = "ns/single")]
    NamespacedSingleton(StickyKeyed),
    #[persistent(keys = [id], namespace = "ns/keyed/:id")]
    NamespacedKeyed(StickyKeyed),
}

#[test]
fn namespace_returns_some_only_when_declared() {
    assert_eq!(TopicKind::Singleton.namespace(), None);
    assert_eq!(TopicKind::NamespacedSingleton.namespace(), Some("ns/single"));
    assert_eq!(TopicKind::NamespacedKeyed.namespace(), Some("ns/keyed/:id"));
    // Non-persistent kinds always None.
    assert_eq!(TopicKind::Plain2.namespace(), None);
}

#[test]
fn key_names_in_declaration_order() {
    assert!(TopicKind::Singleton.key_names().is_empty());
    assert_eq!(TopicKind::NamespacedKeyed.key_names(), &["id"]);
    // Multi-key reference: Multi has keys=[window_id, menu_id] (above)
    assert_eq!(TopicKind::Multi.key_names(), &["window_id", "menu_id"]);
}
```

This will not compile until the macro grows the new arms.

- [ ] **Step 2: Run tests — expect compile error**

```bash
cargo test -p sola-bus topic::tests
```

Expected: macro can't match `#[persistent(namespace = "…")]` / can't find `TopicKind::namespace()` method.

- [ ] **Step 3: Extend the macro accumulator shape**

In `crates/sola-bus/src/topic.rs`, the inner-macro persistent payload accumulator is `[ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]`. Extend each occurrence to carry a namespace `tt`-fragment, emitting either an empty parenthesized group `()` for "no namespace" or `("..."`) for a declared namespace.

The full edit pattern is mechanical, replacing every `$pp:ident($ppt:ty) keys=[$($ppk:ident)*]` with `$pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt`, and every `$pp($ppt) keys=[$($ppk)*]` (in the rebuild) with `$pp($ppt) keys=[$($ppk)*] ns=$ppns`. Then existing arms thread `ns=()` for variants that don't declare a namespace.

Concrete worked example: the `#[persistent(keys = [...])]` payload arm becomes:

```rust
( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
  [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
  [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
  #[persistent(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
    $crate::_define_topics_inner!{
        [ $($eu)* ] [ $($ep($ept))* ]
        [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
        [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[$($k)*] ns=() ]
        $($rest)*
    }
};
```

…and add four **new** arms (with-namespace counterparts), e.g.:

```rust
// --- #[persistent(keys = [...], namespace = "...")] payload variants ---
( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
  [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
  [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
  #[persistent(keys = [ $($k:ident),+ $(,)? ], namespace = $ns:literal)]
  $name:ident ( $payload:ty ), $($rest:tt)* ) => {
    $crate::_define_topics_inner!{
        [ $($eu)* ] [ $($ep($ept))* ]
        [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
        [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[$($k)*] ns=($ns) ]
        $($rest)*
    }
};
// (terminal-form arm without `, $($rest:tt)*` — same body, no rest)

// --- #[persistent(namespace = "...")] payload variants (no keys) ---
( ... #[persistent(namespace = $ns:literal)] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
    ... ns=($ns) ...
};
// + terminal-form
```

Update the kept "no-namespace" arms to thread `ns=()` instead of nothing.

`#[persistent]` unit variants (no payload) do not gain namespace support — they lack key fields and their use is rare; out of scope.

Sticky variants stay untouched (namespace is a persistence concern only).

- [ ] **Step 4: Generate `TopicKind::namespace()` and `key_names()` in the terminal arm**

In the terminal arm (around line 304), add to the `impl TopicKind` block:

```rust
/// Path namespace declared via `#[persistent(namespace = "...")]`.
/// Returns `None` for unnamespaced and non-persistent kinds. The
/// returned string may contain `:keyname` placeholders that
/// interpolate from `keys_for()` at runtime.
#[allow(unused_variables)]
pub fn namespace(self) -> Option<&'static str> {
    match self {
        $( TopicKind::$pp => $crate::topic::__ns_or_none!($ppns), )*
        _ => None,
    }
}

/// Names of the key fields declared on this topic kind, in
/// declaration order. Empty slice for unkeyed kinds. Used together
/// with `keys_for()` to interpolate `:keyname` placeholders in a
/// namespace path.
pub fn key_names(self) -> &'static [&'static str] {
    match self {
        $( TopicKind::$sp => &[ $( stringify!($spk), )* ], )*
        $( TopicKind::$pp => &[ $( stringify!($ppk), )* ], )*
        _ => &[],
    }
}
```

Add the helper macro at the top-level of `topic.rs` (outside any `mod`):

```rust
#[macro_export]
#[doc(hidden)]
macro_rules! __ns_or_none {
    ( () ) => { None };
    ( ($ns:literal) ) => { Some($ns) };
}
```

- [ ] **Step 5: Update existing test fixtures to thread `ns=()` for old arms**

The existing test in `topic.rs` that uses `#[persistent(keys = [window_id, menu_id])] Multi(...)` needs no test change, but the macro arms it traverses are the unchanged accumulator. Verify by running the existing tests (see Step 7).

- [ ] **Step 6: Run new tests — expect them to pass**

```bash
cargo test -p sola-bus topic::tests
```

Expected: all topic tests pass.

- [ ] **Step 7: Run full workspace tests to confirm no regressions**

```bash
cargo test
```

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(sola-bus): add namespace attr to define_topics!

#[persistent(namespace = \"…\")] and #[persistent(keys = [...], namespace = \"…/:k\")]
declare a per-topic on-disk path (relative to ~/.config/sola/, sans .toml).
Generates TopicKind::namespace() and TopicKind::key_names() accessors.
Path resolution and storage routing land next."
```

---

## Task 4: Implement `Topic::path_for` runtime resolution

Walk the namespace string, substitute `:keyname` placeholders by zipping `key_names()` with `keys_for()`, validate each value, and return the final `PathBuf` under `sola_config_dir()`.

**Files:**
- Modify: `crates/sola-bus/src/topic.rs`
- Test: same file's `mod tests`

- [ ] **Step 1: Write failing tests**

Append to the topic.rs test module (the same one with the `define_topics!` test block):

```rust
#[test]
fn path_for_no_namespace_falls_back_to_state_toml() {
    let t = Topic::Singleton(StickyKeyed { id: "x".into(), other: 1 });
    let p = t.path_for();
    assert_eq!(p, sola_core::config::sola_config_dir().join("state.toml"));
}

#[test]
fn path_for_singleton_namespaced() {
    let t = Topic::NamespacedSingleton(StickyKeyed { id: "x".into(), other: 1 });
    let p = t.path_for();
    assert_eq!(p, sola_core::config::sola_config_dir().join("ns/single.toml"));
}

#[test]
fn path_for_keyed_namespaced_interpolates() {
    let t = Topic::NamespacedKeyed(StickyKeyed { id: "abc".into(), other: 1 });
    let p = t.path_for();
    assert_eq!(p, sola_core::config::sola_config_dir().join("ns/keyed/abc.toml"));
}

#[test]
fn path_for_rejects_unsafe_key_segments() {
    // path traversal
    let t = Topic::NamespacedKeyed(StickyKeyed { id: "../escape".into(), other: 1 });
    assert!(t.path_for_safe().is_err());
    // forward slash
    let t = Topic::NamespacedKeyed(StickyKeyed { id: "a/b".into(), other: 1 });
    assert!(t.path_for_safe().is_err());
    // empty
    let t = Topic::NamespacedKeyed(StickyKeyed { id: "".into(), other: 1 });
    assert!(t.path_for_safe().is_err());
}
```

- [ ] **Step 2: Run tests — expect compile error**

```bash
cargo test -p sola-bus topic::tests
```

Expected: `Topic::path_for` and `Topic::path_for_safe` undefined.

- [ ] **Step 3: Implement `path_for` and `path_for_safe`**

Add to `Topic` impl in the macro's terminal arm, right after `keys_for`:

```rust
/// Resolved on-disk path for this topic's persistence. For topics
/// without a `namespace` annotation, returns the shared `state.toml`
/// path (the legacy/default storage). For namespaced topics, returns
/// `<sola_config_dir>/<interpolated namespace>.toml`.
///
/// Panics on key-segment validation failures (slash, empty, `..`).
/// Use `path_for_safe` to receive a typed error instead.
pub fn path_for(&self) -> std::path::PathBuf {
    self.path_for_safe().expect("invalid key segment in namespace interpolation")
}

/// Same as `path_for`, but returns `Err` instead of panicking when
/// an interpolated key value is unsafe (contains `/`, `\0`, `..`,
/// or is empty).
pub fn path_for_safe(&self) -> Result<std::path::PathBuf, $crate::topic::PathError> {
    let kind = self.kind();
    let cfg = sola_core::config::sola_config_dir();
    match kind.namespace() {
        None => Ok(cfg.join("state.toml")),
        Some(template) => {
            let names = kind.key_names();
            let values = self.keys_for();
            let resolved = $crate::topic::interpolate_namespace(template, names, &values)?;
            Ok(cfg.join(format!("{resolved}.toml")))
        }
    }
}
```

Add the helper at module level (top of `topic.rs`, after `Behavior`):

```rust
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("namespace key '{0}' is empty")]
    Empty(String),
    #[error("namespace key '{0}' contains forbidden segment '{1}'")]
    Forbidden(String, String),
    #[error("namespace template references unknown key '{0}'")]
    UnknownKey(String),
}

/// Substitute `:keyname` placeholders in `template` with values from
/// `values`, matching by the same index in `names`. Each value is
/// validated: non-empty, no `/`, no `\0`, no `..` segment.
pub fn interpolate_namespace(
    template: &str,
    names: &[&'static str],
    values: &[String],
) -> Result<String, PathError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(idx) = rest.find(':') {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + 1..];
        // Read the longest matching key name at this position. We greedy-match
        // by trying each declared name; the first that's a prefix of `after`
        // wins. Names are short and there are few; trivial.
        let mut matched: Option<(&'static str, &str)> = None;
        for (i, name) in names.iter().enumerate() {
            if after.starts_with(name) {
                let value = values.get(i)
                    .ok_or_else(|| PathError::UnknownKey((*name).to_string()))?;
                if value.is_empty() {
                    return Err(PathError::Empty((*name).to_string()));
                }
                if value.contains('/') || value.contains('\0') {
                    return Err(PathError::Forbidden((*name).to_string(), value.clone()));
                }
                if value.split('/').any(|seg| seg == "..") || value == ".." {
                    return Err(PathError::Forbidden((*name).to_string(), value.clone()));
                }
                matched = Some((name, value));
                break;
            }
        }
        let (name, value) = matched.ok_or_else(|| {
            // No declared key matched at this position. Slice up to next path
            // delimiter for the error message.
            let end = after.find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(after.len());
            PathError::UnknownKey(after[..end].to_string())
        })?;
        out.push_str(value);
        rest = &after[name.len()..];
    }
    out.push_str(rest);
    Ok(out)
}
```

Add `thiserror = "2"` to `crates/sola-bus/Cargo.toml` `[dependencies]`. (Check first; if already present, skip.)

- [ ] **Step 4: Run tests**

```bash
cargo test -p sola-bus topic::tests
```

Expected: all path_for tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(sola-bus): Topic::path_for resolves namespace + key interpolation

path_for(&self) returns ~/.config/sola/<resolved-namespace>.toml for
namespaced persistent topics, falling back to state.toml otherwise.
path_for_safe surfaces interpolation errors (empty key, forward slash,
.. segment) instead of panicking. Slugification is the writer's
responsibility — sola-bus enforces a strict allowlist."
```

---

## Task 5: Add browser persistent topic types

Defines the structs and registers them with namespace annotations.

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`

- [ ] **Step 1: Write the structs**

Insert above the `define_topics!` invocation in `crates/sola-bus/src/topics.rs` (search for `define_topics! {` — there's only one in this file, around line 354):

```rust
/// One persisted browser tab. Keyed by `id` (UUIDv4 generated at tab
/// creation). `ordinal` orders the tab strip; gaps are fine, JS sorts
/// by ordinal. `session_state` is the base64-encoded WebKit page
/// session blob (back/forward stack, scroll position, form state) and
/// is `None` until the tab has been visited.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserTab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_state: Option<String>,
}

/// Browser-wide singleton config. Only one in flight; replaces on
/// every emit. Headroom for future fields (default search engine,
/// zoom default, etc.) without breaking the schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
}

/// One visited URL. Cap and MRU policy enforced by the browser before
/// emitting `BrowserHistory` (the singleton aggregate).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visits: u32,
}

/// Singleton browser history aggregate. The browser owns the cap
/// (1000) and MRU ordering. A future `sola-history` service can
/// take over this topic without breaking the schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserHistory {
    pub entries: Vec<HistoryEntry>,
}
```

- [ ] **Step 2: Register the topics in `define_topics!`**

Inside the `define_topics! { ... }` block in `topics.rs`, add three lines next to the existing `TerminalConfig` / `TerminalSession` registrations:

```rust
#[persistent(namespace = "browser")]
BrowserConfig(BrowserConfig),

#[persistent(namespace = "browser/history")]
BrowserHistory(BrowserHistory),

#[persistent(keys = [id], namespace = "browser/tabs/:id")]
BrowserTab(BrowserTab),
```

- [ ] **Step 3: Build to verify the macro accepts the new annotations**

```bash
cargo build -p sola-bus
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(sola-bus): add BrowserTab, BrowserConfig, BrowserHistory persistent topics

BrowserTab is keyed by [id] with namespace browser/tabs/:id (one file
per tab). BrowserConfig and BrowserHistory are singletons with their
own namespace files. Storage routing follows in the next commit."
```

---

## Task 6: Route bus state.rs to per-topic paths

Now that topic types exist, write the routing. Storage format for namespaced files: payload as the file's whole content (no `[Section]` header). Unnamespaced topics keep state.toml as today.

**Files:**
- Modify: `crates/sola-bus/src/state.rs`
- Test: same file

- [ ] **Step 1: Write failing tests** (same as the deferred Task 5 step 1, included here for completeness)

Append to `state.rs` test module:

```rust
#[test]
fn write_namespaced_singleton_no_section_header() {
    use crate::topics::BrowserConfig;
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("browser.toml");
    let t = Topic::BrowserConfig(BrowserConfig { active_tab_id: Some("abc".into()) });
    write_namespaced(&path, &t).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("[BrowserConfig]"));
    assert!(raw.contains("active_tab_id = \"abc\""));
}

#[test]
fn load_namespaced_singleton_yields_one_sticky() {
    use crate::topics::BrowserConfig;
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("browser.toml");
    fs::write(&path, "active_tab_id = \"abc\"\n").unwrap();
    let msgs = load_namespaced_singleton(&path, TopicKind::BrowserConfig);
    assert_eq!(msgs.len(), 1);
    match Topic::parse(&msgs[0]).unwrap() {
        Topic::BrowserConfig(c) => assert_eq!(c.active_tab_id.as_deref(), Some("abc")),
        _ => panic!("expected BrowserConfig"),
    }
    assert_eq!(msgs[0].source, BUS_SOURCE);
}

#[test]
fn load_namespaced_keyed_walks_directory() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("browser/tabs");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.toml"), "id = \"a\"\nurl = \"\"\ntitle = \"\"\nordinal = 0\n").unwrap();
    fs::write(dir.join("b.toml"), "id = \"b\"\nurl = \"\"\ntitle = \"\"\nordinal = 1\n").unwrap();
    let msgs = load_namespaced_keyed(&dir, TopicKind::BrowserTab);
    assert_eq!(msgs.len(), 2);
    let mut keys: Vec<String> = msgs.iter().flat_map(|m| m.keys.clone()).collect();
    keys.sort();
    assert_eq!(keys, vec!["a", "b"]);
}

#[test]
fn retract_namespaced_unlinks_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.toml");
    fs::write(&path, "x = 1\n").unwrap();
    retract_namespaced(&path).unwrap();
    assert!(!path.exists());
}
```

- [ ] **Step 2: Run tests — expect missing helpers**

```bash
cargo test -p sola-bus state::tests
```

Expected: undefined `write_namespaced`, `load_namespaced_singleton`, `load_namespaced_keyed`, `retract_namespaced`.

- [ ] **Step 3: Implement the four namespaced helpers in state.rs**

```rust
/// Write a namespaced topic's payload as the whole file's content
/// (no `[Section]` header — the topic kind is implied by the path).
/// For non-persistent topics or topics without a payload, no-op.
pub fn write_namespaced(path: &Path, topic: &Topic) -> io::Result<()> {
    let Some(value) = topic.to_toml_value() else {
        return Ok(());
    };
    let content = match value {
        toml::Value::Table(t) => toml::to_string_pretty(&t)
            .expect("topic payload table always serializes"),
        // Non-table payloads aren't expected for namespaced topics, but
        // emit a single-key wrapping so the file remains valid TOML.
        other => format!("value = {}\n", other),
    };
    atomic_write(path, content.as_bytes())
}

/// Load one sticky `Message` from a singleton namespaced file. Missing
/// file → empty vec. Parse errors logged and skipped.
pub fn load_namespaced_singleton(path: &Path, kind: TopicKind) -> Vec<Message> {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(path = %path.display(), %e, "namespaced singleton read failed");
            return Vec::new();
        }
    };
    let value: toml::Value = match raw.parse::<toml::Table>() {
        Ok(t) => toml::Value::Table(t),
        Err(e) => {
            warn!(path = %path.display(), %e, "namespaced singleton parse failed");
            return Vec::new();
        }
    };
    let Some(topic) = Topic::from_toml_section(kind, value) else {
        warn!(path = %path.display(), "namespaced singleton schema mismatch");
        return Vec::new();
    };
    let mut msg = topic.to_message();
    msg.sticky = true;
    msg.source = BUS_SOURCE.to_string();
    info!(path = %path.display(), "restored namespaced singleton");
    vec![msg]
}

/// Walk a directory and load every `*.toml` file as one keyed sticky
/// of the given topic kind. Missing dir → empty vec.
pub fn load_namespaced_keyed(dir: &Path, kind: TopicKind) -> Vec<Message> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(dir = %dir.display(), %e, "namespaced keyed dir read failed");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                warn!(path = %path.display(), %e, "namespaced keyed read failed");
                continue;
            }
        };
        let value: toml::Value = match raw.parse::<toml::Table>() {
            Ok(t) => toml::Value::Table(t),
            Err(e) => {
                warn!(path = %path.display(), %e, "namespaced keyed parse failed");
                continue;
            }
        };
        let Some(topic) = Topic::from_toml_section(kind, value) else {
            warn!(path = %path.display(), "namespaced keyed schema mismatch");
            continue;
        };
        let mut msg = topic.to_message();
        msg.sticky = true;
        msg.source = BUS_SOURCE.to_string();
        info!(path = %path.display(), keys = ?msg.keys, "restored namespaced keyed");
        out.push(msg);
    }
    out
}

/// Unlink a namespaced topic's file. Missing file is not an error.
pub fn retract_namespaced(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
```

- [ ] **Step 4: Run new helper tests — expect them to pass**

```bash
cargo test -p sola-bus state::tests::write_namespaced_singleton_no_section_header state::tests::load_namespaced_singleton_yields_one_sticky state::tests::load_namespaced_keyed_walks_directory state::tests::retract_namespaced_unlinks_file
```

Expected: 4 pass.

- [ ] **Step 5: Update the bus host call sites**

Now wire the four new helpers into the bus host's persistence path. Find every call to `write_section`, `retract_section`, and `load` (in `state.rs` and elsewhere — likely `host.rs` or wherever the host dispatches). For each persistent topic emit/retract:

```rust
let path = topic.path_for();   // resolves namespace or falls back to state.toml
let kind = topic.kind();
if kind.namespace().is_some() {
    if kind.has_keys() {
        write_namespaced(&path, &topic)?;
    } else {
        write_namespaced(&path, &topic)?;
    }
} else {
    write_section(&state_path(), &topic)?;
}
```

(For retract, the path is constructed from `Message::keys` if keyed; needs a small `Topic::reconstruct_path_from_keys(kind, keys)` helper or to walk through the existing topic if available.)

**Practical execution note for the agent:** identify the host code that calls `state::write_section` / `state::retract_section` and update it in-place to branch on `kind.namespace().is_some()`. The startup load path similarly grows a branch: for each persistent kind, if it has a namespace, walk its file/dir; otherwise read `state.toml` as today.

Run `grep -rn "state::write_section\|state::retract_section\|state::load" crates/sola-bus/src` to find call sites; expect 1–3 hits in the host module.

- [ ] **Step 6: Workspace tests**

```bash
cargo test
```

Expected: all pass. The Zones round-trip (already in `state.rs` tests) confirms unnamespaced flow still works; the new namespaced tests confirm the new flow.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(sola-bus): per-topic file routing for #[persistent(namespace=...)]

Namespaced topics persist to dedicated files (no [Section] wrapper:
the topic kind is implied by the path). Keyed namespaced topics get
one file per (interpolated) key. Retract unlinks the file. The
unnamespaced state.toml path is unchanged."
```

---

## Task 7: Replace browser persistence with bus topics + `realize_active`

The biggest application-level change. `JsonConfig` impls go away; tabs and history move to bus emit/retract; selection logic rewritten as `realize_active`.

**Files:**
- Modify: `crates/sola-browser/src/state.rs` — drop `JsonConfig` impls; keep `HistoryEntry::record_visit` and `search` as methods on `BrowserHistory` (re-exporting from `sola_bus::topics::BrowserHistory` if helpful, or a thin wrapper).
- Modify: `crates/sola-browser/src/app.rs` — handler rewrites, `realize_active`, persistence triggers.
- Modify: `crates/sola-browser/src/tabs.rs` — emission on URL/title change.

- [ ] **Step 1: Write a failing test for `realize_active`**

In `crates/sola-browser/src/app.rs` (or a new `realize.rs` if app.rs is too tangled), introduce a small pure helper that takes the inputs and returns the selection, testable without GTK:

```rust
/// Pure selection function: given the current tab set, the desired
/// active id, and the currently-realized id, returns the new selection.
/// `Some(id)` to switch; `None` to clear.
pub(crate) fn select_active(
    tabs_by_id: &std::collections::HashMap<String, sola_bus::topics::BrowserTab>,
    desired_active_id: Option<&str>,
    realized: Option<&str>,
) -> Option<Option<String>> {
    let target = desired_active_id
        .filter(|id| tabs_by_id.contains_key(*id))
        .map(str::to_string)
        .or_else(|| {
            tabs_by_id.values()
                .min_by_key(|t| t.ordinal)
                .map(|t| t.id.clone())
        });
    if target.as_deref() == realized {
        None  // no change
    } else {
        Some(target)
    }
}
```

Tests in the same file:

```rust
#[cfg(test)]
mod realize_tests {
    use super::*;
    use sola_bus::topics::BrowserTab;
    use std::collections::HashMap;

    fn tab(id: &str, ord: u32) -> BrowserTab {
        BrowserTab { id: id.into(), url: String::new(), title: String::new(), ordinal: ord, session_state: None }
    }

    #[test]
    fn no_change_when_target_matches_realized() {
        let tabs: HashMap<_, _> = [(String::from("a"), tab("a", 0))].into();
        let r = select_active(&tabs, Some("a"), Some("a"));
        assert_eq!(r, None);
    }

    #[test]
    fn switches_to_desired_when_present() {
        let tabs: HashMap<_, _> = [
            (String::from("a"), tab("a", 0)),
            (String::from("b"), tab("b", 1)),
        ].into();
        assert_eq!(select_active(&tabs, Some("b"), Some("a")), Some(Some("b".into())));
    }

    #[test]
    fn falls_back_to_lowest_ordinal_when_desired_missing() {
        let tabs: HashMap<_, _> = [
            (String::from("b"), tab("b", 5)),
            (String::from("c"), tab("c", 1)),
        ].into();
        assert_eq!(select_active(&tabs, Some("a"), None), Some(Some("c".into())));
    }

    #[test]
    fn clears_when_no_tabs() {
        let tabs: HashMap<String, BrowserTab> = HashMap::new();
        assert_eq!(select_active(&tabs, Some("a"), Some("a")), Some(None));
    }

    #[test]
    fn clears_when_no_desired_and_no_tabs() {
        let tabs: HashMap<String, BrowserTab> = HashMap::new();
        assert_eq!(select_active(&tabs, None, None), None);
    }
}
```

- [ ] **Step 2: Run tests — expect undefined `select_active`**

```bash
cargo test -p sola-browser realize_tests
```

Expected: undefined function.

- [ ] **Step 3: Implement `select_active` (helper above) and `realize_active` (calls it)**

Land the `select_active` helper from Step 1 verbatim in `app.rs`. Then add the impl method:

```rust
impl BrowserApp {
    fn realize_active(&mut self, ctx: &mut AppCtx) {
        if let Some(target) = select_active(
            &self.tabs_by_id,
            self.config.active_tab_id.as_deref(),
            self.realized_active_tab_id.as_deref(),
        ) {
            self.realized_active_tab_id = target.clone();
            // Apply the change to the visible WebView. Reuses the existing
            // switch/show logic; keep the body small and idempotent.
            if let Some(id) = target {
                self.show_tab(&id, ctx);
            } else {
                self.hide_all_tabs(ctx);
            }
        }
    }
}
```

`show_tab` and `hide_all_tabs` already exist in spirit — they're implemented inside the current `switch_tab` / similar paths. Adapt the existing code; do not duplicate it. If it lives across multiple methods today, factor a single `show_tab(&str)` helper.

- [ ] **Step 4: Replace `TabStore`/`BrowsingHistory` JsonConfig with bus state**

In `crates/sola-browser/src/state.rs`, remove the `impl JsonConfig for TabStore` and `impl JsonConfig for BrowsingHistory`. Drop the `TabStore` struct entirely (its role is replaced by `HashMap<String, BrowserTab>` plus `BrowserConfig`). Keep the `record_visit` and `search` logic; move them onto the topic struct via an extension trait, e.g.:

```rust
// crates/sola-browser/src/state.rs
use sola_bus::topics::{BrowserHistory, HistoryEntry};

const MAX_HISTORY_ENTRIES: usize = 1000;

pub trait HistoryOps {
    fn record_visit(&mut self, url: &str, title: &str);
    fn search(&self, query: &str, limit: usize) -> Vec<&HistoryEntry>;
}

impl HistoryOps for BrowserHistory {
    fn record_visit(&mut self, url: &str, title: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.url == url) {
            entry.title = title.to_string();
            entry.visits += 1;
        } else {
            self.entries.push(HistoryEntry { url: url.into(), title: title.into(), visits: 1 });
        }
        if let Some(pos) = self.entries.iter().position(|e| e.url == url) {
            let entry = self.entries.remove(pos);
            self.entries.insert(0, entry);
        }
        self.entries.truncate(MAX_HISTORY_ENTRIES);
    }
    fn search(&self, query: &str, limit: usize) -> Vec<&HistoryEntry> {
        let q = query.to_lowercase();
        let mut hits: Vec<&HistoryEntry> = self.entries.iter()
            .filter(|e| e.url.to_lowercase().contains(&q) || e.title.to_lowercase().contains(&q))
            .collect();
        hits.sort_by(|a, b| b.visits.cmp(&a.visits));
        hits.truncate(limit);
        hits
    }
}
```

The existing `history_record_and_search` and `history_caps_at_max` tests in `state.rs` keep working with the trait API.

- [ ] **Step 5: Rewrite the `BrowserApp` state and lifecycle**

In `app.rs`, replace `tabs: Vec<Tab>` with a parallel `tabs_by_id: HashMap<String, BrowserTab>` for selection logic, while preserving the existing `tabs: Vec<Tab>` for live `WebView` ownership. The two stay in sync: insert/remove from both on `on_browser_tab` arrivals and on UI-initiated actions.

Add fields:

```rust
pub(crate) tabs_by_id: std::collections::HashMap<String, sola_bus::topics::BrowserTab>,
pub(crate) config: sola_bus::topics::BrowserConfig,
pub(crate) realized_active_tab_id: Option<String>,
pub(crate) history: sola_bus::topics::BrowserHistory,
```

Drop:

```rust
pub(crate) tab_store: TabStore,            // gone — replaced by tabs_by_id + config
pub(crate) active_tab_id: Option<String>,  // gone — folded into config + realized_active_tab_id
```

In `BrowserApp::new`, replace `let tab_store = TabStore::load(); let history = BrowsingHistory::load();` with empty-init versions; the bus subscription in `register_bus` will receive sticky restoration shortly after.

In `register_bus`, add three handlers:

```rust
fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
    bus.on(TopicKind::MenuAction, Self::on_menu_action);
    bus.on(TopicKind::OpenUrl, Self::on_open_url);
    bus.on(TopicKind::BrowserTab, Self::on_browser_tab);
    bus.on(TopicKind::BrowserConfig, Self::on_browser_config);
    bus.on(TopicKind::BrowserHistory, Self::on_browser_history);
}
```

And the handlers themselves:

```rust
fn on_browser_tab(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
    let Topic::BrowserTab(tab) = delivery.topic else { return };
    if delivery.retracted {
        self.tabs_by_id.remove(&tab.id);
        self.destroy_webview(&tab.id, ctx);
    } else {
        // Idempotent upsert — covers create AND update.
        let was_present = self.tabs_by_id.contains_key(&tab.id);
        self.tabs_by_id.insert(tab.id.clone(), tab.clone());
        if was_present {
            self.update_webview(&tab, ctx);
        } else {
            self.create_webview_for_tab(&tab, ctx);
        }
    }
    self.realize_active(ctx);
}

fn on_browser_config(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
    let Topic::BrowserConfig(cfg) = delivery.topic else { return };
    self.config = cfg.clone();
    self.realize_active(ctx);
}

fn on_browser_history(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
    if delivery.source == Self::APP_ID { return; }
    let Topic::BrowserHistory(h) = delivery.topic else { return };
    self.history = h.clone();
}
```

Replace every `self.tab_store.save()` and `self.history.save()` with `ctx.emit(...)`:

```rust
// where the browser used to call self.persist_tabs():
ctx.emit(Topic::BrowserConfig(self.config.clone()));
// for the changed tab:
ctx.emit(Topic::BrowserTab(/* updated BrowserTab */));

// where the browser used to call self.persist_history():
ctx.emit(Topic::BrowserHistory(self.history.clone()));

// On tab close:
ctx.retract(Topic::BrowserTab(BrowserTab { id: tab_id.clone(), ..Default::default() }));
// Then remove from tabs_by_id and destroy WebView locally.
```

The existing `persist_tabs`, `persist_history`, `capture_session_state`, `capture_tab_session_state`, `create_tab`, `close_tab`, `switch_tab` methods need adaptation, not rewrite. Each call site that mutated `self.tab_store` should mutate the corresponding `BrowserTab` struct and emit it.

Map of removed/replaced methods:
- `persist_tabs` → emits `BrowserConfig` (active_tab_id) and one `BrowserTab` per affected tab.
- `persist_history` → emits `BrowserHistory(self.history.clone())`.
- `capture_session_state` → on shutdown, for each tab whose WebView has a session_state, build a fresh `BrowserTab` with that session_state and emit it.
- `capture_tab_session_state(tab_id)` → emit one updated `BrowserTab` for that id.
- Tab create → emit BrowserTab for the new tab; emit BrowserConfig if it changed active_tab_id.
- Tab close → retract BrowserTab(id); emit BrowserConfig if active_tab_id changed.
- Tab switch → emit BrowserConfig with new active_tab_id.
- Tab URL/title change (notify::uri / notify::title handlers in tabs.rs) → emit updated BrowserTab; record_visit on history then emit BrowserHistory.

There is no `Default` for `BrowserTab` because not all fields make sense as defaults — for retract, build a stub `BrowserTab { id, url: String::new(), title: String::new(), ordinal: 0, session_state: None }`. Only `id` is read by retract (via `keys_for`); other fields are ignored.

- [ ] **Step 6: Build**

```bash
cargo make build
```

Expected: clean. Inevitable surface-area-large edit; expect to chase 5–15 type/lifetime errors. Each is mechanical (renames, removed fields, new field shapes).

- [ ] **Step 7: Run tests**

```bash
cargo test
```

Expected: all pass. New `realize_tests` and `HistoryOps` carryovers green.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(sola-browser): persist tabs/config/history via bus topics

JsonConfig impls and JSON files retired. Per-tab BrowserTab is keyed
and per-file under browser/tabs/. BrowserConfig.active_tab_id and
BrowserHistory are singletons under their own namespace files.
realize_active is the single source of selection truth and is called
after every BrowserConfig or BrowserTab handler. on_browser_history
filters self-echoes via Delivery::source."
```

---

## Task 8: Legacy JSON migrator

One-shot migration of `browser-tabs.json` and `browser-history.json` into bus topics. Runs in `BrowserApp::new` before `register_bus` is called by the framework.

**Files:**
- Create: `crates/sola-browser/src/migrate.rs`
- Modify: `crates/sola-browser/src/app.rs` — call migrator from `new`
- Modify: `crates/sola-browser/src/main.rs` — `mod migrate;`
- Test: `crates/sola-browser/src/migrate.rs`

- [ ] **Step 1: Write failing test**

```rust
// crates/sola-browser/src/migrate.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn migrate_populates_topics_when_only_legacy_files_exist() {
        let tmp = TempDir::new().unwrap();
        let legacy_dir = tmp.path();
        fs::write(legacy_dir.join("browser-tabs.json"), r#"
{
  "tabs": [
    {"url": "https://example.com/", "title": "Example"},
    {"url": "https://github.com/", "title": "GitHub", "session_state": "abc"}
  ],
  "active_tab_id": "ignored-by-design"
}
"#).unwrap();
        fs::write(legacy_dir.join("browser-history.json"), r#"
{
  "entries": [
    {"url": "https://example.com/", "title": "Example", "visits": 3}
  ]
}
"#).unwrap();

        let plan = compute_migration(legacy_dir).unwrap();
        assert_eq!(plan.tabs.len(), 2);
        assert_eq!(plan.tabs[0].url, "https://example.com/");
        assert_eq!(plan.tabs[0].ordinal, 0);
        assert_eq!(plan.tabs[1].session_state.as_deref(), Some("abc"));
        // active_tab_id: legacy schema has no usable identity; left empty.
        assert!(plan.config.active_tab_id.is_none());
        assert_eq!(plan.history.entries.len(), 1);
    }

    #[test]
    fn migrate_returns_none_when_new_namespace_exists() {
        let tmp = TempDir::new().unwrap();
        let legacy = tmp.path();
        fs::write(legacy.join("browser-tabs.json"), "{}").unwrap();
        fs::create_dir_all(legacy.join("browser")).unwrap();
        assert!(compute_migration(legacy).is_none());
    }

    #[test]
    fn migrate_returns_none_when_no_legacy_files() {
        let tmp = TempDir::new().unwrap();
        assert!(compute_migration(tmp.path()).is_none());
    }
}
```

- [ ] **Step 2: Run tests — expect undefined `compute_migration`**

```bash
cargo test -p sola-browser migrate::tests
```

- [ ] **Step 3: Implement the migrator**

```rust
// crates/sola-browser/src/migrate.rs
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sola_bus::topics::{BrowserConfig, BrowserHistory, BrowserTab, HistoryEntry};

/// Snapshot of legacy on-disk state, ready to emit via the bus.
#[derive(Debug)]
pub struct MigrationPlan {
    pub tabs: Vec<BrowserTab>,
    pub config: BrowserConfig,
    pub history: BrowserHistory,
}

#[derive(Deserialize)]
struct LegacyTab {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    session_state: Option<String>,
}

#[derive(Deserialize)]
struct LegacyTabStore {
    #[serde(default)]
    tabs: Vec<LegacyTab>,
}

#[derive(Deserialize, Serialize)]
struct LegacyHistory {
    #[serde(default)]
    entries: Vec<HistoryEntry>,
}

/// Compute a migration plan from `dir` (typically `~/.config/sola/`).
/// Returns `None` if there's nothing to migrate (either the new
/// namespace already exists, or no legacy files are present).
pub fn compute_migration(dir: &Path) -> Option<MigrationPlan> {
    // If the new namespace root exists, assume migration already happened.
    if dir.join("browser").exists() {
        return None;
    }
    let tabs_path = dir.join("browser-tabs.json");
    let history_path = dir.join("browser-history.json");
    if !tabs_path.exists() && !history_path.exists() {
        return None;
    }

    let tabs: Vec<BrowserTab> = std::fs::read_to_string(&tabs_path).ok()
        .and_then(|raw| serde_json::from_str::<LegacyTabStore>(&raw).ok())
        .map(|store| store.tabs.into_iter().enumerate().map(|(i, t)| BrowserTab {
            id: uuid::Uuid::new_v4().to_string(),
            url: t.url,
            title: t.title,
            ordinal: i as u32,
            session_state: t.session_state,
        }).collect())
        .unwrap_or_default();

    let history = std::fs::read_to_string(&history_path).ok()
        .and_then(|raw| serde_json::from_str::<LegacyHistory>(&raw).ok())
        .map(|h| BrowserHistory { entries: h.entries })
        .unwrap_or_default();

    Some(MigrationPlan {
        tabs,
        // Legacy schema can't reliably identify which tab was active —
        // see plan-of-record. realize_active falls back to lowest-ordinal
        // (i.e., the first tab), reproducing legacy behavior.
        config: BrowserConfig { active_tab_id: None },
        history,
    })
}

/// Renames the legacy files to `.migrated` so they're preserved for
/// one cycle but not re-migrated on subsequent runs.
pub fn mark_migrated(dir: &Path) {
    for name in &["browser-tabs.json", "browser-history.json"] {
        let path = dir.join(name);
        if path.exists() {
            let dest = path.with_extension("json.migrated");
            if let Err(e) = std::fs::rename(&path, &dest) {
                tracing::warn!(path = %path.display(), %e, "failed to rename legacy file");
            }
        }
    }
}
```

- [ ] **Step 4: Wire migrator into `BrowserApp::new`**

In `app.rs`, near the top of `BrowserApp::new`:

```rust
fn new(ctx: &mut AppCtx) -> Self {
    if let Some(plan) = crate::migrate::compute_migration(&sola_core::config::sola_config_dir()) {
        for tab in plan.tabs {
            ctx.emit(Topic::BrowserTab(tab));
        }
        ctx.emit(Topic::BrowserConfig(plan.config));
        ctx.emit(Topic::BrowserHistory(plan.history));
        crate::migrate::mark_migrated(&sola_core::config::sola_config_dir());
    }
    // ... rest of existing new() body ...
}
```

The bus stickies these emits and persists them to the new namespace files synchronously. The subscription replay in `register_bus` then delivers them back to populate `tabs_by_id`/`config`/`history`.

- [ ] **Step 5: Add `mod migrate;` to main.rs**

```rust
// crates/sola-browser/src/main.rs
mod app;
mod chrome;
mod migrate;
mod state;
mod tabs;
```

- [ ] **Step 6: Add `uuid` to crate deps if it isn't already**

Check `crates/sola-browser/Cargo.toml`. The legacy version had `uuid = { version = "1", features = ["v4"] }`. After Task 1 it should still be there.

- [ ] **Step 7: Run tests**

```bash
cargo test -p sola-browser
```

Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(sola-browser): one-shot legacy JSON migrator

On startup, if browser/ namespace dir doesn't exist and legacy
browser-tabs.json/browser-history.json are present, parse them and
emit corresponding BrowserTab/BrowserConfig/BrowserHistory topics.
Legacy files are renamed to .migrated. compute_migration is a pure
function with full unit-test coverage."
```

---

## Task 9: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the workspace structure block**

In the "Workspace Structure" section:

- Add `  sola-browser/        # WebKit browser` to the `crates/` listing (alphabetical position: between `sola-bus/` and `sola-core/` if going strictly alphabetical, or near `sola-shell/`).
- Remove the `  browser/             # WebKit browser (not in workspace yet)` line from the `apps/` listing.

- [ ] **Step 2: Verify**

```bash
grep -n "sola-browser\|browser/" CLAUDE.md
```

Expected: one match for `sola-browser` (in crates/), zero for `browser/` under apps/.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: move browser to crates/ in CLAUDE.md workspace listing"
```

---

## Task 10: Final build + clean tree

**Files:** none.

- [ ] **Step 1: Final workspace build**

```bash
cargo make build
```

Expected: clean.

- [ ] **Step 2: Final workspace test**

```bash
cargo test
```

Expected: clean.

- [ ] **Step 3: Confirm clean tree**

```bash
git status
```

Expected: clean working tree on branch `port/browser-to-crates`.

- [ ] **Step 4: Print branch summary**

```bash
git log --oneline master..HEAD
```

Eyes-on review of the commit list. Each commit is a self-contained step.

---

## Out-of-band notes for the executor

- **No installs.** `cargo make install` is forbidden without express user permission per `CLAUDE.md`. Verify with `cargo make build` and unit tests only. Manual smoke is the user's job after merge.
- **No merges to master.** Stay on `port/browser-to-crates`. The user merges.
- **Keep frontend untouched.** Every `web/` file in `crates/sola-browser/web/` survives the move. No edits unless something breaks at build/test time.
- **If `realize_active` integration trips up:** the Step 3 of Task 7 expects `show_tab` and `hide_all_tabs` helpers. The current code expresses this via `switch_tab(&id)`. Adapt the existing path; do not duplicate logic. When in doubt, keep the smallest possible diff to the visible-tab management code — just route it through `realize_active` instead of the old direct-call path.
- **State.toml during transition:** if you accidentally leave a partial migration where some browser topics still write to `state.toml`, no harm done — just verify the namespace path resolution at the writer. The bus shouldn't care; both paths work.
