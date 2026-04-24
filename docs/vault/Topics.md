# Topics

Typed bus messages defined via the `define_topics!` macro in
`crates/sola-bus/src/topics.rs`. Each `Topic` variant carries a
payload type; `TopicKind` is a plain C-like enum of the same names
used for subscription and dispatch.

## Sending

```rust
bus.emit(Topic::Focus(FocusTarget { window_id: 42 }))?;
bus.emit(Topic::Shutdown)?;
```

`BusClient::emit` is the only emit method. The bus inspects
`TopicKind::behavior()` to decide whether to stamp the wire
`Message` as sticky and whether to persist it — there is no
per-emit flag.

## Receiving

```rust
let Some(topic) = Topic::parse(&msg) else { continue };
match topic {
    Topic::Windows(windows) => { /* ... */ }
    Topic::Zones(map)       => { /* restored from disk or live edit */ }
    Topic::Shutdown         => { /* ... */ }
    _ => {}
}
```

## Behavior

Three delivery behaviors, set per variant in the macro:

| Behavior       | Annotation        | Retained in memory | Restored from disk |
|----------------|-------------------|--------------------|--------------------|
| Ephemeral      | (none, default)   | no                 | no                 |
| Sticky         | `#[sticky]`       | yes                | no                 |
| Persistent     | `#[persistent]`   | yes (implies)      | yes                |

Sticky retention is keyed by `(topic_kind, source)` — different
apps can hold independent stickies for the same kind. When a client
subscribes, the bus replays every matching sticky.

Persistent topics survive bus restart. On startup the bus reads
`~/.config/sola/state.toml`, parses one section per kind, and
pre-populates the sticky map tagged `source = "sola-bus"`. The first
live emit replaces the bootstrap entry (see
[[sola-bus#Persistence]]). New subscribers see exactly one value per
persistent kind.

## Current Topics

### Persistent

| Topic   | Payload                          | Emitter      | Purpose                                  |
|---------|----------------------------------|--------------|------------------------------------------|
| `Zones` | `HashMap<String, Zone>`          | sola-shell   | Zone assignment per app_id               |

### Sticky (in-memory only)

| Topic              | Payload                        | Emitter             | Purpose                              |
|--------------------|--------------------------------|---------------------|--------------------------------------|
| `Windows`          | `Vec<Window>`                  | sola-river          | Current compositor window list       |
| `OutputGeometry`   | `OutputGeometry`               | sola-river          | Screen resolution (startup/hotplug)  |
| `RegisteredChords` | `Vec<RegisteredChord>`         | sola-shell          | Chords the shell wants routed        |
| `SetAppMenu`       | `AppMenuPayload`               | each app            | App-specific menubar definition      |

### Ephemeral

**Window management**

| Topic            | Payload                 | Purpose                         |
|------------------|-------------------------|---------------------------------|
| `LaunchApp`      | `LaunchAppPayload`      | Request to launch an app        |
| `LaunchResult`   | `LaunchResultPayload`   | Outcome of a launch attempt     |
| `UserAppExited`  | `UserAppExitedPayload`  | A user app process exited       |
| `CloseApp`       | `String` (app_id)       | Request to close an app         |

**Composition (shell → sola-river)**

| Topic         | Payload                  | Purpose                           |
|---------------|--------------------------|-----------------------------------|
| `Composition` | `Vec<CompositionEntry>`  | Z-order (bottom to top)           |
| `Frame`       | `FrameUpdate`            | Per-window position and size      |
| `Focus`       | `FocusTarget`            | Which window gets keyboard focus  |

**Mouse (sola-river → shell)**

| Topic          | Payload                 | Purpose                      |
|----------------|-------------------------|------------------------------|
| `MouseEntered` | `MouseEnteredPayload`   | Pointer entered a window     |
| `MouseLeft`    | —                       | Pointer left all windows     |
| `MouseClicked` | `MouseClickedPayload`   | Click on a window            |

**Keyboard (shell ↔ sola-river)**

| Topic           | Payload       | Purpose                                    |
|-----------------|---------------|--------------------------------------------|
| `Chord`         | `ChordEvent`  | A registered chord was pressed             |
| `ChordReleased` | `ChordEvent`  | A registered chord was released            |

**Menus**

| Topic         | Payload              | Purpose                      |
|---------------|----------------------|------------------------------|
| `MenuAction`  | `MenuActionPayload`  | A menu item was activated    |

**Clipboard**

| Topic    | Payload        | Purpose                    |
|----------|----------------|----------------------------|
| `Copy`   | `EditRequest`  | Global copy chord fired    |
| `Paste`  | `EditRequest`  | Global paste chord fired   |

**Other**

| Topic                 | Payload              | Purpose                            |
|-----------------------|----------------------|------------------------------------|
| `OpenUrl`             | `OpenUrlRequest`     | Open a URL in the browser          |
| `ClientConnected`     | `String` (app_id)    | A bus client identified itself     |
| `ClientDisconnected`  | `String` (app_id)    | A bus client disconnected          |
| `Shutdown`            | —                    | Request full desktop shutdown      |

## Key Payload Types

```rust
pub struct Window {
    pub window_id: u32,
    pub app_id: String,
    pub title: String,
    pub pid: Option<u32>,
}

pub enum Zone {
    Left, Right, Top, Bottom,
    TopMiddle, BottomMiddle, FullMiddle, Fullscreen,
}

pub struct RegisteredChord {
    pub keysym: u32,
    pub modifiers: u32,
}
```

## Adding a Topic

Add a variant to `define_topics!` in `topics.rs`. Annotate for
sticky or persistent retention if needed:

```rust
define_topics! {
    // Ephemeral (default)
    Shutdown,
    CloseApp(String),

    // Sticky — latest value replayed to new subscribers
    #[sticky]
    Windows(Vec<Window>),

    // Persistent — survives bus restart via state.toml
    #[persistent]
    Zones(HashMap<String, Zone>),
}
```

Topic names on the wire and in `state.toml` come from the variant
identifier — no string duplication. Persistent payloads must be TOML-
round-trippable; use `serde` attributes (e.g. `#[serde(tag = "type")]`
for data-carrying enums) if the default shape won't serialize cleanly.

## Adding a Persistent Topic

Any TOML-shapeable payload qualifies. Things to watch:

- **Use unit enums for plain choices** (like `Zone`) — serialize as
  strings, read nicely in state.toml
- **Use `#[serde(tag = "type")]` on data-carrying enums** — array-of-
  tables layout wants tagged variants
- **For sensitive fields, wrap with `sola_core::Encrypted<T>`** —
  encrypts on TOML (disk), pass-through on postcard (wire). See
  [[sola-bus#Encrypted payloads]].
