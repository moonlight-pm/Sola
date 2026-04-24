# sola-bus

**Crate:** `crates/sola-bus/`
**Binary:** `sola-bus`
**Role:** IPC bus host process + client library + protocol definitions.

## Dual Purpose

1. **The bus host binary** — creates the Unix socket, accepts
   connections, broadcasts messages, retains stickies, persists the
   persistent ones to disk.
2. **The client library** — other crates depend on `sola-bus` for
   `BusClient`, `Message`, `Topic`, `TopicKind`, and transport
   helpers.

## Bus Host

- Socket: `$XDG_RUNTIME_DIR/sola-bus` (overridable via `$SOLA_BUS_PATH`)
- Star topology; every message routed to every subscriber of that
  kind
- Per-client writer thread with a bounded mpsc queue so a slow client
  can't stall broadcasts
- Sticky map keyed by `(topic, source)` — replayed on subscription
- Persistent-sticky storage in `~/.config/sola/state.toml` — see
  [[#Persistence]] below

## Message

The [[Wire Format]] struct — what flows over the socket:

```rust
pub struct Message {
    pub id: Uuid,                 // UUIDv7 (monotonic, embeds timestamp)
    pub topic: String,            // variant name, e.g. "Windows"
    pub payload: Option<Vec<u8>>, // postcard-encoded typed data
    pub sticky: bool,             // set by BusClient from TopicKind::behavior()
    pub source: String,           // emitting app_id
}
```

## Topic

Typed topics defined via `define_topics!`. See [[Topics]] for the
full list, annotations, and payload types.

## BusClient

```rust
let mut bus = BusClient::new();
bus.set_app_id("my-app");
bus.connect_blocking(Duration::from_secs(1));
bus.subscribe(&[TopicKind::Windows, TopicKind::Zones])?;

// The bus decides sticky-ness from TopicKind::behavior();
// there's no separate `emit_sticky`.
bus.emit(Topic::Focus(FocusTarget { window_id: 42 }))?;
bus.emit(Topic::Zones(zone_map))?;        // persisted to disk automatically
```

### BusRegistry

Generic topic handler dispatch used by sola-app and sola-shell:

```rust
let mut reg: BusRegistry<MyApp, MyCtx> = BusRegistry::new();
reg.on(TopicKind::Windows, MyApp::on_windows);
reg.on(TopicKind::Zones,   MyApp::on_zones);
// Later, in the event loop:
registry.dispatch(&topic, &mut app, &mut ctx);
```

## Persistence

Persistent topics (annotated `#[persistent]` in `define_topics!`)
are stored in a single file: `~/.config/sola/state.toml`. One
`[Section]` per topic kind, section name equal to the variant
identifier.

```toml
[Zones]
"sola-browser" = "Left"
"sola-terminal" = "Right"
```

### Startup

1. Read state.toml (missing file → empty state, no error)
2. For each section, look up `TopicKind::from_str(section_name)`.
   Unknown sections are logged and skipped
3. Reject if `kind.behavior() != Persistent` (guards against
   accidental hand-edits adding non-persistent sections)
4. `Topic::from_toml_section(kind, value)` → `Option<Topic>`;
   schema mismatches are logged and skipped
5. Convert to `Message`, set `sticky = true`, `source = "sola-bus"`,
   insert into the sticky map

### On emit

After the bus stores a sticky and broadcasts it, if the kind is
persistent:
- Evict any `(kind, "sola-bus")` bootstrap entry that this live emit
  supersedes
- Load state.toml → replace the section → atomic temp+rename write

Holding the bus lock during the disk write is acceptable because
persistent topics carry config, not traffic; debouncing remains a
follow-up optimization.

## Encrypted payloads

`sola_core::Encrypted<T>` is a serde newtype that encrypts only
when the serializer is human-readable:

- **TOML (disk):** inner value is JSON-encoded, age-encrypted,
  base64'd, and prefixed with `age1enc:`
- **postcard (wire):** passes through in the clear (local socket,
  same-user processes can already read it)

Key is auto-generated at `~/.config/sola/key` (mode 0600) on first
use of `Encrypted<T>`. Lost key → `Encrypted<T>` fields fail to
deserialize; the bus logs, the app treats the field as unset and
re-prompts. Threat model is grep-safety and accidental-cloud-sync-
safety, not defense against a local attacker.

## Source Files

| File               | Purpose                                                         |
|--------------------|-----------------------------------------------------------------|
| `src/main.rs`      | Bus host binary — accept, broadcast, sticky, persistence        |
| `src/lib.rs`       | Library root — re-exports                                       |
| `src/message.rs`   | `Message` struct, UUIDv7 IDs, timestamps                        |
| `src/client.rs`    | `BusClient` — connect, identify, subscribe, emit, recv          |
| `src/topic.rs`     | `define_topics!` macro, `Behavior`, payload helpers             |
| `src/topics.rs`    | Topic definitions                                               |
| `src/registry.rs`  | `BusRegistry` — typed handler dispatch                          |
| `src/state.rs`     | state.toml read/write, atomic writes, BUS_SOURCE tag            |
| `src/transport.rs` | Length-prefixed postcard framing over streams                   |
