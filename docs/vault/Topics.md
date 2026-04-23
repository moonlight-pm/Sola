# Topics

Typed bus messages defined via the `define_topics!` macro in `crates/sola-bus/src/topics.rs`. Each variant of the `Topic` enum represents a message type. Payload types and config-related types (`ConfigValue`, `MutateOp`, `MutateConfigPayload`) live in `sola-core` and are re-exported by `sola-bus::topics`.

## Sending

```rust
bus.emit(Topic::Focus(FocusTarget { window_id: 42 }))?;
bus.emit(Topic::Shutdown)?;
bus.emit_sticky(Topic::Config(snapshot))?;
```

## Receiving

```rust
let Some(topic) = Topic::parse(&msg) else { continue };
match topic {
    Topic::Windows(windows) => { ... }
    Topic::Shutdown => { ... }
    _ => {}
}
```

## Current Topics

### Config

Centralized config managed by sola-session. Persisted as `~/.config/sola/sola.toml`.

| Topic | Payload | Sticky | Direction |
|---|---|---|---|
| `Config` | `Vec<(String, ConfigValue)>` | yes | session → all |
| `MutateConfig` | `MutateConfigPayload` | no | any → session |

`Config` is the full flattened config snapshot (dotted keys → typed values). `MutateConfig` carries an op (`Set`, `Delete`, `Append`, `Insert`, `Remove`, `Replace`) and a dotted key path. Session validates, persists, and re-emits `Config` on success.

### Window Management

| Topic | Payload | Sticky | Purpose |
|---|---|---|---|
| `Windows` | `Vec<Window>` | yes | All compositor windows (from sola-river) |
| `LaunchApp` | `LaunchAppPayload` | no | Request to launch an app |
| `LaunchResult` | `LaunchResultPayload` | no | Outcome of a launch attempt |
| `UserAppExited` | `UserAppExitedPayload` | no | A user app process exited |
| `CloseApp` | `String` (app_id) | no | Request to close an app |

### Composition (shell → sola-river)

| Topic | Payload | Purpose |
|---|---|---|
| `Composition` | `Vec<CompositionEntry>` | Z-order (bottom to top) |
| `Frame` | `FrameUpdate` | Per-window position and size |
| `Focus` | `FocusTarget` | Which window gets keyboard focus |

### Output

| Topic | Payload | Purpose |
|---|---|---|
| `OutputGeometry` | `OutputGeometry` | Screen resolution (startup + hotplug) |

### Mouse Events (sola-river → shell)

| Topic | Payload | Purpose |
|---|---|---|
| `MouseEntered` | `MouseEnteredPayload` | Pointer entered a window |
| `MouseLeft` | none | Pointer left all windows |
| `MouseClicked` | `MouseClickedPayload` | Click on a window |

### Keyboard (shell ↔ sola-river)

| Topic | Payload | Purpose |
|---|---|---|
| `RegisteredChords` | `Vec<RegisteredChord>` | Chords the shell wants routed |
| `Chord` | `ChordEvent` | A registered chord was pressed |
| `ChordReleased` | `ChordEvent` | A registered chord was released |

### Menus

| Topic | Payload | Purpose |
|---|---|---|
| `SetAppMenu` | `AppMenuPayload` | App registers its menu bar |
| `MenuAction` | `MenuActionPayload` | A menu item was activated |

### Clipboard

| Topic | Payload | Purpose |
|---|---|---|
| `Copy` | `EditRequest` | Global copy chord fired |
| `Paste` | `EditRequest` | Global paste chord fired |

### Other

| Topic | Payload | Purpose |
|---|---|---|
| `OpenUrl` | `OpenUrlRequest` | Open a URL in the browser |
| `ClientConnected` | `String` (app_id) | A bus client connected |
| `ClientDisconnected` | `String` (app_id) | A bus client disconnected |
| `Shutdown` | none | Request full desktop shutdown |

## Key Payload Types

```rust
pub struct Window {
    pub window_id: u32,
    pub app_id: String,
    pub title: String,
    pub pid: Option<u32>,
}

pub struct MutateConfigPayload {
    pub key: String,      // dotted path, e.g. "mail.imap_port"
    pub op: MutateOp,
}

pub enum MutateOp {
    Set(ConfigValue),
    Delete,
    Append(ConfigValue),
    Insert { index: u32, value: ConfigValue },
    Remove { index: u32 },
    Replace { index: u32, value: ConfigValue },
}

pub enum ConfigValue {
    String(String), Int(i64), Float(f64), Bool(bool),
    Array(Vec<ConfigValue>), Table(Vec<(String, ConfigValue)>),
}
```

## Adding Topics

Add a variant to `define_topics!` in `topics.rs`. Unit variant for no payload, tuple variant for typed payload:

```rust
define_topics! {
    Shutdown,                       // unit — no payload
    CloseApp(String),               // payload — String
    Windows(Vec<Window>),           // payload — Vec<Window>
}
```

Topic string is derived from the variant name automatically.
