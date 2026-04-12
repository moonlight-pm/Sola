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
- Stateless — no client tracking, no filtering, no routing
- One reader thread per client connection

## Client Library

### Message

The [[Wire Format]] struct — what flows over the socket:

```rust
pub struct Message {
    pub id: Uuid,           // UUIDv7 (monotonic, embeds timestamp)
    pub topic: String,      // e.g. "GrabInput", "Shutdown"
    pub payload: Option<Vec<u8>>,
}
```

### Topic

Typed topics defined via `define_topics!` macro. See [[Topics]].

### BusClient

```rust
let mut bus = BusClient::connect()?;
bus.emit(Topic::GrabInput("sola-switcher".into()))?;
let msg: Option<Message> = bus.try_recv();
```

## Source Files

| File | Purpose |
|---|---|
| `src/main.rs` | Bus host binary — accept, broadcast |
| `src/lib.rs` | Library root — re-exports |
| `src/message.rs` | `Message` struct, constructors, timestamp extraction |
| `src/client.rs` | `BusClient` — connect, emit, send, recv |
| `src/topic.rs` | `Topic` trait, `define_topics!` macro, `decode_payload` |
| `src/topics.rs` | Shell topic definitions and payload types |
| `src/transport.rs` | Length-prefixed postcard framing over streams |
