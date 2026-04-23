# sola-bus

**Crate:** `crates/sola-bus/`
**Binary:** `sola-bus`
**Role:** IPC bus host process + client library + protocol definitions.

## Dual Purpose

This crate is both:
1. **The bus host binary** — creates the Unix socket, accepts connections, broadcasts messages
2. **The client library** — other crates depend on `sola-bus` for `BusClient`, `Message`, `Topic`, transport helpers

## Bus Host

- Creates socket at `$XDG_RUNTIME_DIR/sola-bus` (or `$SOLA_BUS_PATH`)
- Star topology: every message broadcast to every connected client
- Tracks client roster, replays sticky messages to new connections
- One reader thread per client connection

## Client Library

### Message

The [[Wire Format]] struct — what flows over the socket:

```rust
pub struct Message {
    pub id: Uuid,                 // UUIDv7 (monotonic, embeds timestamp)
    pub topic: String,            // e.g. "Windows", "Shutdown"
    pub payload: Option<Vec<u8>>, // postcard-encoded typed data
    pub sticky: bool,             // bus retains and replays to new clients
    pub source: String,           // emitting app_id (set by BusClient)
}
```

Sticky messages are keyed by `(topic, source)` — each app can have its own sticky on the same topic.

### Topic

Typed topics defined via `define_topics!` macro. See [[Topics]].

Config-related types (`ConfigValue`, `MutateOp`, `MutateConfigPayload`) are defined in `sola-core::config` and re-exported by `sola-bus::topics`.

### BusClient

```rust
let mut bus = BusClient::new();
bus.set_app_id("my-app");
bus.connect()?;
bus.subscribe(&[TopicKind::Windows, TopicKind::Config])?;
bus.emit(Topic::CloseApp("firefox".into()))?;
bus.emit_sticky(Topic::Config(snapshot))?;

// Blocking connect (retries until bus is up):
bus.connect_blocking(Duration::from_secs(1));
```

### BusRegistry

Generic topic handler dispatch:

```rust
let mut reg: BusRegistry<MyApp, MyCtx> = BusRegistry::new();
reg.on(TopicKind::Windows, MyApp::on_windows);
// Later, in event loop:
registry.dispatch(&topic, &mut app, &mut ctx);
```

## Source Files

| File | Purpose |
|---|---|
| `src/main.rs` | Bus host binary — accept, broadcast, sticky replay |
| `src/lib.rs` | Library root — re-exports |
| `src/message.rs` | `Message` struct, constructors, timestamp extraction |
| `src/client.rs` | `BusClient` — connect, emit, recv, subscribe, connect_blocking |
| `src/topic.rs` | `define_topics!` macro, `decode_payload`, `encode_payload` |
| `src/topics.rs` | Topic definitions, payload types, re-exports from sola-core |
| `src/registry.rs` | `BusRegistry` — generic topic handler dispatch |
| `src/transport.rs` | Length-prefixed postcard framing over streams |
