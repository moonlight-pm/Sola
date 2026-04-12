# Wire Format

Every message on the [[Sola Bus]]:

```
id:      UUIDv7          // 16 bytes, monotonic, embeds ms-precision timestamp
topic:   String          // e.g. "GrabInput", "Shutdown"
payload: Option<Bytes>   // arbitrary binary, deserialized by consumer
```

Three fields. That's the entire protocol.

## Naming

- **Message** — the wire struct. What flows over the socket.
- **Topic** — a typed Rust enum variant that packs itself into a Message. Names derived from variant names, so duplicates are impossible.

## Transport

- **Serialization:** `postcard` (serde-compatible, compact binary)
- **Framing:** 4-byte little-endian length prefix + postcard bytes
- **Socket:** `$XDG_RUNTIME_DIR/sola-bus` (Unix stream)
- **Logging:** auto-serializes to JSON for human-readable logs

## Topic Strings

Topic names are the Rust enum variant names directly:
- `"GrabInput"`, `"ReleaseInput"`, `"Shutdown"`, `"Key"`, etc.

No namespacing on the wire. Uniqueness enforced by Rust's type system (can't have two enum variants with the same name).
