# Keyed Stickies + Retract — Design

**Date:** 2026-04-27
**Status:** Proposed
**Branch:** `feature/keyed-stickies`

## Summary

Today the bus stores at most one sticky per `(topic, source)` pair. A topic that wants to represent a *collection* (e.g. the live tab list of `sola-terminal`) has to ship the whole collection in one payload, and every emit overwrites the entire collection. That makes per-element correctness fragile — any code path emitting a stale view of the collection clobbers everything.

This change lets a topic declare key fields. The bus then stores stickies keyed by `(topic, source, keys)`, so a single source can have many concurrent stickies of the same topic kind, addressed by their key values. Removing one is done with a new `retract` operation that takes the same typed `Topic` value `emit` does.

The terminal port (`feature/terminal-port`) is the first consumer: `TerminalSession { id, … }` replaces `TerminalSessions { tabs: Vec<…> }`. After that, `sola-mail` drafts, per-window menu state in `sola-shell`, and similar collections become natural to model.

## Goals

- Allow a `#[persistent]` (and `#[sticky]`) topic variant to declare one or more key fields via a macro attribute.
- Bus stores stickies keyed by `(topic, source, keys)` and persists each independently.
- New client API: `ctx.retract(topic)` removes a keyed sticky. Symmetric with `ctx.emit(topic)`.
- Subscribers receive both live emits and retractions through their existing `bus.on(TopicKind, …)` registration.
- No replay barrier: subscribers handle stickies as they arrive, in any order.
- No backward compatibility: this is a coordinated change across the workspace; old wire format is retired.

## Non-goals

- Replay-completion / "all stickies delivered" signals.
- Generic event sourcing or message-history replay (the bus still stores only the *latest* sticky per key, not a log).
- Per-message ACL or scoping.
- Schema migration tooling for `state.toml` (we erase and start fresh).

## Architecture

### `Message` extension

```rust
pub struct Message {
    pub id: Uuid,
    pub topic: String,
    pub payload: Option<Vec<u8>>,
    pub sticky: bool,
    pub source: String,
    pub keys: Vec<String>,    // empty for unkeyed topics
    pub tombstone: bool,      // when sticky+tombstone, evicts (topic, source, keys)
}
```

- `keys` carries the stringified key field values, in declaration order.
- `tombstone` is meaningful only when `sticky == true`; `sticky=false && tombstone=true` is malformed and rejected.
- No `Option`, no `serde(default)` — fresh wire format.

### `define_topics!` macro extension

Existing form (still works for unkeyed topics):

```rust
#[persistent]
Zones(HashMap<String, Zone>),
```

New form for keyed stickies:

```rust
#[persistent(keys = ["id"])]
TerminalSession(TerminalSession),
```

Multiple keys allowed:

```rust
#[persistent(keys = ["window_id", "menu_id"])]
WindowMenu(WindowMenu),
```

The macro generates, per keyed variant:

- A `keys_for` extractor that, given the typed payload, returns `Vec<String>` by stringifying each named field via `Display`. The named fields must implement `Display` and exist on the payload struct.
- Wiring into `Topic::to_message` so emitted messages carry `keys` populated.
- Wiring into the eviction path (`retract`) so the same extraction runs.
- Compile-time error if the named field doesn't exist or doesn't implement `Display`.

For non-string key types (e.g. `u32`), `Display` is sufficient. We do not require `serde::Serialize`-to-string here; `Display` keeps the encoding uniform and human-readable on disk.

### Bus host changes

Sticky map type:

```rust
sticky: HashMap<(String, String, Vec<String>), Message>,
```

On a sticky emit (`sticky=true && tombstone=false`):

1. Insert into the map under `(topic, source, keys)`, replacing any prior value for that key.
2. Broadcast to subscribers (sender skipped, as today).
3. If persistent, write to disk (see Disk persistence below).

On a retract (`sticky=true && tombstone=true`):

1. Remove the entry under `(topic, source, keys)` from the in-memory map.
2. Broadcast the tombstone message to subscribers (sender skipped). Subscribers see the typed payload alongside the tombstone bit and use it to remove their local copy.
3. If persistent, remove the corresponding disk entry.

A retract for a key that doesn't exist is a no-op in storage, but still broadcast so subscribers can reconcile (idempotent).

Replay on subscribe is unchanged in spirit: iterate `sticky.values()`, send each to the new subscriber. Tombstones are never stored, so they never replay.

### Client API

```rust
impl AppCtx {
    pub fn emit(&self, topic: Topic);
    pub fn retract(&self, topic: Topic);
}
```

Both take a typed `Topic`. `retract` is rejected at runtime with a tracing warning if the topic kind has no declared keys (single-slot topics use overwrite semantics, not retract).

For consumers that have a process-local cache of the live records, `retract(topic)` reads naturally — they already hold the typed value. For consumers that only have keys, they construct a `T::default()` and overwrite the key fields; the bus only inspects the macro-extracted keys, so default values elsewhere are safe.

### Subscriber callback shape

The framework's `BusRegistry::on` callback signature changes from `(&mut Self, &Topic, &mut AppCtx)` to `(&mut Self, &Delivery, &mut AppCtx)`, where:

```rust
pub struct Delivery<'a> {
    pub topic: &'a Topic,
    pub retracted: bool,
}
```

Existing unkeyed handlers migrate with a one-line destructure (`let Delivery { topic, .. } = delivery;`); they never see `retracted == true` because the runtime rejects retract for unkeyed kinds. Keyed-aware handlers branch on `retracted`:

```rust
bus.on(TopicKind::TerminalSession, |delivery, ctx| {
    let Topic::TerminalSession(session) = delivery.topic else { return };
    if delivery.retracted {
        self.remove_tab(&session.id);
    } else {
        self.upsert_tab(session);
    }
});
```

For unit variants (no payload), `retracted` is always `false` — the bus rejects emits/retracts that combine `tombstone=true` with a payload-less topic kind.

### Disk persistence

Today `state.toml` has one section per persistent topic kind:

```toml
[Zones]
"sola-browser" = "Left"
```

For keyed stickies, the section becomes a TOML array of tables, one entry per keyed sticky, with no separate index — serde writes the payload directly:

```toml
[[TerminalSession]]
id = "abc-uuid"
tmux_session = "sola-abc-uuid"
ordinal = 0

[[TerminalSession]]
id = "def-uuid"
tmux_session = "sola-def-uuid"
cwd = "/tmp"
ordinal = 1
```

The bus restores by iterating the array and re-emitting each entry as a sticky on startup, with `keys` extracted from the payload via the same macro-generated extractor. On retract, the bus removes the matching array entry by key match and rewrites the section atomically (existing temp+rename flow).

Mixed topics (some persistent unkeyed, some persistent keyed) coexist — each topic decides its on-disk shape based on whether `keys` is empty.

### Wire format / postcard

`postcard` is order-sensitive. The new `Message` fields go at the end, after `source`:

```
id, topic, payload, sticky, source, keys, tombstone
```

`keys: Vec<String>` and `tombstone: bool` get postcard's default vec/bool encoding. Existing `Message::new` and `with_payload` constructors initialize them to `vec![]` and `false`.

### Replay barrier — explicitly absent

The bus does not signal "all stickies delivered." Subscribers process each delivery on arrival:

- A keyed sticky for an unknown key → add it locally.
- A keyed sticky for a known key → replace the local copy.
- A retract for a known key → remove the local copy.
- A retract for an unknown key → no-op (safe).

Ordering across keys is undefined; each key's *own* events are FIFO from the bus's perspective. Consumers that need a stable display order must put an ordering hint in the payload (e.g. `ordinal: u32`) and sort locally.

## Worked example — terminal sessions

```rust
// in sola-bus/src/topics.rs
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct TerminalSession {
    pub id: String,
    pub tmux_session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub ordinal: u32,
}

define_topics! {
    // ...
    #[persistent(keys = ["id"])]
    TerminalSession(TerminalSession),
}
```

```rust
// sola-terminal cmd_spawn_pty:
ctx.emit(Topic::TerminalSession(TerminalSession {
    id: pty_id.clone(),
    tmux_session: tmux_session.clone(),
    cwd: None,
    ordinal: next_ordinal,
}));

// sola-terminal cmd_close_pty:
let session = self.local.get(&pty_id).cloned()?;
ctx.retract(Topic::TerminalSession(session));

// sola-terminal register_bus:
bus.on(TopicKind::TerminalSession, |delivery, ctx| match delivery {
    Delivery::Live(s) => self.upsert(s),
    Delivery::Retracted(s) => self.remove(&s.id),
});
```

The bus replay on startup delivers each persisted `TerminalSession` once. The terminal upserts each into its local map, sorts by `(ordinal, id)`, and pushes the assembled list to JS. No replay barrier: tabs flicker in over a frame or two, then settle.

## Testing strategy

### Bus + macro

- Roundtrip via postcard for a keyed topic, including the `keys` vec.
- Roundtrip via TOML — write a section, restart, observe replay.
- Multi-key extraction: payload with two `keys = […]` fields produces a 2-element `keys` vec in the right order.
- Sticky map keying: emitting two records with different keys yields two map entries, both replayed to a new subscriber.
- Retract removes from in-memory map and from disk; new subscriber sees neither.
- Retract on missing key is a no-op storage-wise, still broadcasts.
- Compile-fail tests for: missing key field, key field type that isn't `Display`.
- Subscriber receives `Delivery::Retracted` with the typed payload after a `retract`.

### Disk

- Topic with no live entries serializes as an absent section, not an empty array.
- Mixed file with one keyed and one unkeyed persistent topic round-trips intact.
- Hand-edited section with a stray array entry whose schema is wrong — load logs and skips that entry but keeps siblings.

### Failure modes

- `retract` on an unkeyed topic kind logs a warning and is a no-op on the wire.
- Malformed inbound message (`sticky=false && tombstone=true`) is dropped at the bus host with a tracing warning.

## Migration / compatibility

- The wire format changes (new `keys` and `tombstone` fields on `Message`). Old `BusClient` and old `sola-bus` host are incompatible. The whole stack is rebuilt and reinstalled at once. Sola is not yet shipped externally, so we don't carry version skew.
- All currently-declared persistent topics on master are unkeyed (`Zones`, `MailConfig`, etc.). Their `define_topics!` entries don't add `keys`, the macro generates an empty extractor, `keys` stays empty, and the on-disk shape is identical to today's `[Section]`-with-payload layout. No `state.toml` migration is needed for master.
- Keyed persistent topics serialize as TOML arrays of tables (`[[TerminalSession]]`). This shape only appears once a topic is declared with `#[persistent(keys = …)]`. The first such topic in master is `TerminalSession`, introduced by the dependent terminal-port refactor.
- Sticky (in-memory only) topics work the same way: keyed or unkeyed based on declaration, but no disk involvement.

## Open questions

None at design time. The remaining unknowns are implementation-level (exact macro shape, exact `Delivery` enum naming) and will be resolved when the plan is written.
