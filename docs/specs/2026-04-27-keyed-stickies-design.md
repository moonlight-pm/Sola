# Keyed Stickies + Retract — Design

**Date:** 2026-04-27
**Status:** Proposed
**Branch:** `feature/keyed-stickies`

## Summary

Today the bus stores at most one sticky per `(topic, source)` pair. A topic that wants to represent a *collection* (e.g. the live tab list of `sola-terminal`) has to ship the whole collection in one payload, and every emit overwrites the entire collection. That makes per-element correctness fragile — any code path emitting a stale view of the collection clobbers everything. The implicit `source` dimension also adds an invisible second key that nobody asked for: persistent topics today need a "bootstrap eviction" dance because the bus restores stickies under `source = "sola-bus"` and a real client emit lands under a different key.

This change lets a topic declare key fields. The bus then stores stickies keyed by `(topic, keys)` — `source` becomes pure provenance metadata, not part of identity. A single emitter can have many concurrent stickies of the same topic kind, addressed by their key values; multiple emitters that genuinely want per-app records declare `app_id` (or similar) as one of their keys. Removing a sticky is done with a new `retract` operation that takes the same typed `Topic` value `emit` does and travels on the wire as `sticky=false` (the topic kind's macro declaration is the source of truth for whether a `sticky=false` arrival is a transient event or a retraction).

The terminal port (`feature/terminal-port`) is the first consumer: `TerminalSession { id, … }` replaces `TerminalSessions { tabs: Vec<…> }`. After that, `sola-mail` drafts, per-window menu state in `sola-shell`, and similar collections become natural to model.

## Goals

- Allow a `#[persistent]` or `#[sticky]` topic variant to declare zero or more key fields via a macro attribute.
- Bus stores stickies keyed by `(topic, keys)` and persists each independently. `source` is no longer part of sticky identity.
- New client API: `ctx.retract(topic)` removes a sticky. Symmetric with `ctx.emit(topic)`.
- Subscribers receive both live emits and retractions through their existing `bus.on(TopicKind, …)` registration.
- No replay barrier: subscribers handle stickies as they arrive, in any order.
- No backward compatibility: this is a coordinated change across the workspace; old wire format is retired.

## Non-goals

- Replay-completion / "all stickies delivered" signals.
- Generic event sourcing or message-history replay (the bus still stores only the *latest* sticky per key, not a log).
- Per-message ACL or scoping.

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
}
```

- `keys` carries the stringified key field values, in declaration order. Empty for topic kinds that didn't declare any.
- `sticky` does double duty: for a topic kind whose macro declaration is `#[sticky]` or `#[persistent]`, `sticky=true` means "upsert this entry into the sticky map" and `sticky=false` means "retract the entry at `(topic, keys)`". For ephemeral topic kinds, `sticky=false` is just a transient broadcast (today's behavior). The topic kind's declaration is what disambiguates — the wire bit is identical in both cases.
- `source` remains on every message as provenance metadata (logs, audit, debugging) but is no longer part of the sticky map key.
- No `Option`, no `serde(default)` — fresh wire format.

### `define_topics!` macro extension

Existing form (still works for single-record topics):

```rust
#[persistent]
Zones(HashMap<String, Zone>),
```

New form for keyed stickies on a persistent topic:

```rust
#[persistent(keys = ["id"])]
TerminalSession(TerminalSession),
```

Same form on an in-memory sticky topic:

```rust
#[sticky(keys = ["app_id"])]
Windows(WindowList),
```

Multiple keys allowed (works on both `#[sticky]` and `#[persistent]`):

```rust
#[persistent(keys = ["window_id", "menu_id"])]
WindowMenu(WindowMenu),
```

The macro generates, per keyed variant:

- A `keys_for` extractor that, given the typed payload, returns `Vec<String>` by stringifying each named field via `Display`. The named fields must implement `Display` and exist on the payload struct.
- Wiring into `Topic::to_message` so emitted messages carry `keys` populated.
- Wiring into the retract path so the same extraction runs.
- Compile-time error if the named field doesn't exist or doesn't implement `Display`.

For non-string key types (e.g. `u32`), `Display` is sufficient. We do not require `serde::Serialize`-to-string here; `Display` keeps the encoding uniform and human-readable on disk.

### Bus host changes

Sticky map type:

```rust
sticky: HashMap<(String, Vec<String>), Message>,
```

The key is `(topic_name, keys)`. `source` is no longer part of identity, so the bootstrap-eviction logic (`persist_if_needed` removing the `BUS_SOURCE`-keyed entry) goes away — restored persistent stickies live under their real key from the start, and a client emit naturally replaces them.

On a sticky emit (`sticky=true`):

1. Insert into the map under `(topic, keys)`, replacing any prior value for that key.
2. Broadcast to subscribers (sender skipped, as today).
3. If persistent, write to disk (see Disk persistence below).

On a retract (`sticky=false` for a topic kind whose declaration is `#[sticky]` or `#[persistent]`):

1. Remove the entry under `(topic, keys)` from the in-memory map.
2. Broadcast the message to subscribers (sender skipped). Subscribers see the typed payload alongside `retracted = true` and use it to remove their local copy.
3. If persistent, remove the corresponding disk entry.

A retract for a key that doesn't exist is a no-op in storage, but still broadcast so subscribers can reconcile (idempotent).

For ephemeral topic kinds, `sticky=false` keeps today's meaning — a transient broadcast, no map mutation, no disk involvement.

Replay on subscribe is unchanged in spirit: iterate `sticky.values()`, send each to the new subscriber. Retract messages are never stored, so they never replay.

### Client API

```rust
impl AppCtx {
    pub fn emit(&self, topic: Topic);
    pub fn retract(&self, topic: Topic);
}
```

Both take a typed `Topic`. `emit` produces a wire message with `sticky` set from the topic kind's declaration (true for `#[sticky]`/`#[persistent]`, false for ephemeral). `retract` produces a wire message with `sticky=false` and is rejected at runtime with a tracing warning if the topic kind isn't `#[sticky]` or `#[persistent]` (ephemeral topics have nothing to retract).

For consumers that have a process-local cache of the live records, `retract(topic)` reads naturally — they already hold the typed value. For consumers that only have keys, they construct a `T::default()` and overwrite the key fields; the bus only inspects the macro-extracted keys, so default values elsewhere are safe.

### Subscriber callback shape

The framework's `BusRegistry::on` callback signature changes from `(&mut Self, &Topic, &mut AppCtx)` to `(&mut Self, &Delivery, &mut AppCtx)`, where:

```rust
pub struct Delivery<'a> {
    pub topic: &'a Topic,
    pub retracted: bool,
}
```

`retracted` is computed by the framework on receipt: `topic_kind.is_sticky() && !msg.sticky`. Existing handlers for ephemeral topics migrate with a one-line destructure (`let Delivery { topic, .. } = delivery;`); they never see `retracted == true` because their topic kind isn't sticky. Keyed-aware handlers branch on `retracted`:

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

For unit variants (no payload), retract isn't meaningful — the bus rejects `retract(Topic::SomeUnitVariant)` at the client at runtime.

### Disk persistence

Today `state.toml` has one section per persistent topic kind:

```toml
[Zones]
"sola-browser" = "Left"
```

Unkeyed (no `keys = …`) persistent topics keep this exact shape — single `[Section]` with the payload inlined. The first emit overwrites the section; only one record exists per topic.

Keyed persistent topics serialize as a TOML array of tables, one entry per record, with no separate index — serde writes the payload directly:

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

The bus chooses the shape per-topic based on whether the topic kind declared keys: empty `keys` → `[Section]` (today's shape), non-empty `keys` → `[[Section]]`.

On startup the bus restores by iterating the appropriate shape and inserting each entry into the sticky map under `(topic, keys)`. On retract, the bus removes the matching array entry by key match and rewrites the section atomically (existing temp+rename flow). On the final retract for a keyed topic, the section is removed rather than left as an empty array.

Loaded entries on the wire keep `source = "sola-bus"` (still useful as provenance for logs), but the sticky map key is `(topic, keys)`, so a client emit naturally replaces the restored entry without a special eviction step.

### Wire format / postcard

`postcard` is order-sensitive. The new `keys` field goes at the end, after `source`:

```
id, topic, payload, sticky, source, keys
```

`keys: Vec<String>` gets postcard's default vec encoding. Existing `Message::new` and `with_payload` constructors initialize it to `vec![]`.

### Replay barrier — explicitly absent

The bus does not signal "all stickies delivered." Subscribers process each delivery on arrival:

- A sticky for an unknown key → add it locally.
- A sticky for a known key → replace the local copy.
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
bus.on(TopicKind::TerminalSession, |delivery, ctx| {
    let Topic::TerminalSession(session) = delivery.topic else { return };
    if delivery.retracted {
        self.remove(&session.id);
    } else {
        self.upsert(session);
    }
});
```

The bus replay on startup delivers each persisted `TerminalSession` once. The terminal upserts each into its local map, sorts by `(ordinal, id)`, and pushes the assembled list to JS. No replay barrier: tabs flicker in over a frame or two, then settle.

## Testing strategy

### Bus + macro

- Roundtrip via postcard for a keyed topic, including the `keys` vec.
- Roundtrip via TOML — write a section, restart, observe replay.
- Multi-key extraction: payload with two `keys = […]` fields produces a 2-element `keys` vec in the right order.
- Sticky map keying: emitting two records with different keys yields two map entries, both replayed to a new subscriber.
- Two emitters (different `source`) for the same `(topic, keys)` overwrite each other in the sticky map (no implicit per-source slot).
- Retract removes from in-memory map and from disk; new subscriber sees neither.
- Retract on missing key is a no-op storage-wise, still broadcasts.
- Compile-fail tests for: missing key field, key field type that isn't `Display`.
- Subscriber receives a `Delivery` with `retracted = true` and the typed payload after a `retract`.

### Disk

- Single-record persistent topic (no `keys`) serializes as `[Section]` with payload inlined — same as today.
- Keyed persistent topic with no live entries serializes as an absent section, not an empty array.
- Mixed file with one keyed and one unkeyed persistent topic round-trips intact.
- Hand-edited section with a stray array entry whose schema is wrong — load logs and skips that entry but keeps siblings.
- Final retract for a keyed topic removes the section entirely (no empty `[[Topic]]` array left behind).

### Failure modes

- `retract` on an ephemeral topic kind logs a warning at the client and is a no-op on the wire.
- `retract` on a unit (payload-less) sticky variant logs a warning at the client (no payload → no keys to extract).

## Migration / compatibility

- The wire format changes (new `keys` field on `Message`, `sticky` bit reused for retract on sticky/persistent kinds). Old `BusClient` and old `sola-bus` host are incompatible. The whole stack is rebuilt and reinstalled at once. Sola is not yet shipped externally, so we don't carry version skew.
- All currently-declared persistent topics on master are single-record (`Zones`, `MailConfig`, etc.) — their `define_topics!` entries don't add `keys`, the macro generates an empty extractor, `keys` stays empty, and the on-disk shape is identical to today's `[Section]`-with-payload layout. The natural overwrite-on-write behavior of `state.toml` means no explicit migration step: existing files continue to load as-is.
- Removing `source` from sticky-map identity is a behavioral change for any topic where two distinct apps emitted the same single-record sticky and expected per-app slots. Today's persistent topics each have one designated writer, so no in-tree persistent topic regresses. Sticky (in-memory only) topics declared today should be audited during the implementation step: any kind with multiple emitters (e.g. a per-app `Windows` list) must be redeclared with an explicit `keys = ["app_id"]` to preserve current behavior. Future per-app records must always declare the discriminator as a key.
- Keyed persistent topics serialize as TOML arrays of tables (`[[TerminalSession]]`). This shape only appears once a topic is declared with `keys = …`. The first such topic in master is `TerminalSession`, introduced by the dependent terminal-port refactor.
- Sticky (in-memory only) topics work the same way: keyed or unkeyed based on declaration, but no disk involvement.

## Open questions

None at design time. The remaining unknowns are implementation-level (exact macro shape, exact `Delivery` field naming, exact runtime check for "is this topic kind sticky?") and will be resolved when the plan is written.
