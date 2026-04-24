# Wire Format

Every message on the [[sola-bus|bus]]:

```rust
pub struct Message {
    pub id: Uuid,                 // UUIDv7, 16 bytes, embeds ms-precision timestamp
    pub topic: String,            // variant name, e.g. "Zones", "Shutdown"
    pub payload: Option<Vec<u8>>, // postcard-encoded typed data
    pub sticky: bool,             // set by BusClient from TopicKind::behavior()
    pub source: String,           // emitting app_id; "sola-bus" for restored stickies
}
```

Five fields. Five bytes of framing. That's the whole protocol.

## Naming

- **Message** — the wire struct. What flows over the socket.
- **Topic** — a typed Rust enum variant that serializes itself into a
  Message (`topic.to_message()`). Topic names on the wire are the
  variant identifiers, so duplicates can't happen inside one enum.

## Transport

- **Serialization:** `postcard` (serde-compatible, compact binary)
- **Framing:** 4-byte little-endian length prefix + postcard bytes
- **Socket:** `$XDG_RUNTIME_DIR/sola-bus` (Unix stream)
- Fast path: `BufRead` for reads, blocking writes on a dedicated
  per-client writer thread with a bounded mpsc queue

## Sticky and Source

`sticky` is set automatically by `BusClient::emit` from
`TopicKind::behavior()`:

- Ephemeral → `sticky = false`
- Sticky → `sticky = true`
- Persistent → `sticky = true` *and* written to state.toml by the bus

`source` is the emitting client's `app_id` (set via
`BusClient::set_app_id`). For persistent stickies restored from
disk, the bus tags them `source = "sola-bus"`; the first live emit
supersedes that entry. See [[Topics#Behavior]].

## Wire vs. Disk

Topics move in two forms:

- **Wire (postcard):** compact, binary, always used on the socket.
  Non-human-readable serializer — `Encrypted<T>` passes values
  through unchanged.
- **Disk (TOML):** `~/.config/sola/state.toml`, one section per
  persistent topic kind. Human-readable serializer —
  `Encrypted<T>` ciphers here.

The `define_topics!` macro generates `Topic::to_toml_value()` and
`Topic::from_toml_section(kind, value)` so the bus host can move
persistent topics between the two formats without running app code.
