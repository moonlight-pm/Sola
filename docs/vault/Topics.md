# Topics

Typed bus messages defined via the `define_topics!` macro in `crates/sola-bus/src/topics.rs`. Each variant of the `Topic` enum represents a message type.

## Sending

```rust
bus.emit(Topic::GrabInput("sola-switcher".into()))?;
bus.emit(Topic::Shutdown)?;
```

## Receiving

```rust
let Some(topic) = Topic::parse(&msg) else { continue };
match topic {
    Topic::GrabInput(target) => { ... }
    Topic::Shutdown => { ... }
    _ => {}
}
```

## Current Topics

### Input Routing

| Topic | Payload | Purpose |
|---|---|---|
| `Key` | `KeyEvent` | Super+key event forwarded from compositor |
| `GrabInput` | `String` (app_id) | Request exclusive input for a surface |
| `ReleaseInput` | none | Release exclusive input, restore normal focus |

### App Management

| Topic | Payload | Purpose |
|---|---|---|
| `ListApps` | none | Request the current app list |
| `Apps` | `Vec<App>` | The current app list (MRU ordered) |
| `RaiseApp` | `String` (app_id) | Raise all windows of an app |
| `FocusChanged` | `String` (app_id) | Focused app changed |
| `LaunchApp` | `String` (app_id or path) | Launch an app |

### Lifecycle

| Topic | Payload | Purpose |
|---|---|---|
| `Shutdown` | none | Request full desktop shutdown |

## Payload Types

```rust
pub struct App {
    pub app_id: String,
    pub name: String,
    pub icon: String,
    pub window_count: u32,
}

pub struct KeyEvent {
    pub code: u32,
    pub pressed: bool,
    pub super_held: bool,
    pub shift_held: bool,
}
```

## Adding Topics

Add a variant to `define_topics!` in `topics.rs`. Unit variant for no payload, tuple variant for typed payload:

```rust
define_topics! {
    Shutdown,              // unit — no payload
    GrabInput(String),     // payload — String
    Apps(Vec<App>),        // payload — Vec<App>
}
```

Topic string is derived from the variant name automatically.
