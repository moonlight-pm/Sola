# Sola Bus Design

## Overview

The Sola Bus is a general-purpose IPC bus for communication between all Sola components — compositor, shell apps, and future extensions. It is the foundation for all shell functionality.

## Architecture

### System Topology

```
┌───────────────────────────────┐
│       sola (process manager)  │
│       Launches & restarts all │
└───┬───────┬───────┬───────┬───┘
    │       │       │       │
    ▼       ▼       ▼       ▼
┌───────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐
│sola-  │ │sola-     │ │sola-     │ │future    │
│bus    │ │compositor│ │switcher  │ │apps      │
│       │ │          │ │          │ │          │
└───┬───┘ └────┬─────┘ └────┬─────┘ └────┬─────┘
    │          │             │             │
    │    Unix socket: $XDG_RUNTIME_DIR/sola-bus
    └──────────┴─────────────┴─────────────┘
```

- **`sola`** is a pure process manager. It launches `sola-bus`, `sola-compositor`, and all shell apps. It restarts any that crash. If a managed process exits with code 0, sola treats it as intentional shutdown — kills all children and exits. Any other exit code triggers a restart.
- **`sola-bus`** is a separate process that hosts the bus. Creates the Unix socket, accepts connections, broadcasts all messages to all clients. Stateless — if it restarts, clients reconnect.
- **`sola-compositor`** is a separate process. Wayland compositor + bus client.
- **Shell apps** (switcher, launcher, menubar, etc.) are separate processes. Wayland clients + bus clients. Sola apps are resilient to compositor and bus restarts.

No launch ordering is required. All Sola apps handle missing bus or compositor connections gracefully and reconnect when available.

### Workspace

```
crates/
  sola/              # process manager
  sola-bus/          # bus host process + protocol definitions
  sola-compositor/   # Wayland compositor (bus client)
apps/
  switcher/          # app switcher (WebView, bus client)
```

## Principles

- **The bus is loose.** Any client can put whatever it wants on the bus. Good convention (not enforcement) keeps things orderly.
- **No built-in correlation.** If a message needs to reference another message, that goes in the payload. The bus itself doesn't know or care about request/response patterns.
- **Wire format is the contract, not Rust types.** Apps can be rebuilt independently. The `sola-bus` crate provides convenience types but isn't a hard compile-time coupling. Unknown topic strings are ignored.

## Wire Format

Every message on the bus:

```
id:      UUIDv7          // unique, monotonic, binary (16 bytes)
                         // embeds ms-precision timestamp; extract for logging
topic:   String          // e.g. "shell::ShowSwitcher", "shell::Apps"
payload: Option<Bytes>   // arbitrary binary, deserialized by consumer
```

Three fields. That's the entire protocol.

### Naming

- **Message** — the wire struct (id + topic + payload). What flows over the socket.
- **Topic** — a typed Rust struct that knows how to pack itself into a Message. Defined via the `define_topics!` macro. Topic names are derived from the struct name, so duplicates are impossible (Rust enforces unique struct names per module).

### Typed Topics

Topics are defined with `define_topics!` and used via `bus.emit()`:

```rust
define_topics! {
    shell {
        GrabInput(String),     // topic string: "shell::GrabInput"
        ReleaseInput,          // topic string: "shell::ReleaseInput"
        RaiseApp(String),
    }
}

// Sending:
bus.emit(shell::GrabInput("sola-switcher".into()))?;

// Receiving:
match msg.topic.as_str() {
    shell::GrabInput::TOPIC => { let target = shell::GrabInput::decode(&msg)?; }
    _ => { /* unknown topic, ignore */ }
}
```

**Transport:** `postcard` (serde-compatible, compact binary). Auto-serializes to JSON for human-readable logs.

**Socket:** `$XDG_RUNTIME_DIR/sola-bus` (Unix stream, star topology — sola-bus broadcasts every message to all connected clients).

## Input Routing

The compositor owns all input (libinput, Wayland seat). One rule governs how keys are dispatched:

- **Super key held** → key event goes to the bus. Not forwarded to any Wayland client. Shell apps listen on the bus for their key combos and respond.
- **No Super key** → key event goes to the focused Wayland client via normal Wayland protocol. Non-Sola apps work normally.

This means all global shortcuts use Super as a modifier. The compositor doesn't know what any shortcut means — it just routes based on whether Super is held.

### Input Grab

Shell apps that need exclusive input (e.g., the switcher while it's active) use a grab/release pattern:

- **`shell::GrabInput { target }`** — the compositor shows the target's surface above everything and routes all input (keyboard + pointer) to it exclusively. Other clients stop receiving input.
- **`shell::ReleaseInput`** — the compositor hides the surface and restores normal focus behavior.

The compositor identifies surfaces by matching the target name to a Wayland client identity announced over the bus.

## Recovery Patterns

The bus is stateless and messages are fire-and-forget. Two patterns handle missed messages:

1. **Request pattern.** An app emits a request topic (e.g., `shell::ListApps`). Apps that own that data respond by emitting their current state. Used for on-demand queries.

2. **Focus-driven refresh.** The compositor emits `shell::FocusChanged` on every focus change. Apps that need to stay in sync (e.g., the menubar) listen for this and request fresh data. If the menubar crashes and restarts, the next focus change brings it up to date. The normal flow IS the recovery.

## Component Responsibilities

### sola (process manager)

- Launches `sola-bus`, `sola-compositor`, and all shell apps as child processes
- Watches for crashes, restarts processes
- Exit code 0 from any child = intentional shutdown, exit everything
- Watches all managed binaries for changes, restarts on update
- Watches own binary, execv's self on update
- No bus logic, no desktop logic — pure process management

### sola-bus (crate / process)

The crate serves dual purpose — it's both the bus host binary and the client library:

**Bus host (binary):**
- Creates the bus socket at `$XDG_RUNTIME_DIR/sola-bus`
- Accepts client connections
- Broadcasts all messages to all connected clients
- Stateless — no client tracking, no filtering, no routing

**Client library:**
- `Message` struct with serde derives (postcard for wire, JSON for logs)
- `Topic` trait and `define_topics!` macro for typed topics
- `BusClient` — connect, emit, send, recv
- UUIDv7 generation and timestamp extraction
- Transport helpers: framed read/write over Unix socket
- Shell topic definitions and payload types

## Bus Behavior

- **Star topology:** all apps connect to `sola-bus`. Every message is broadcast to every connected client. No filtering, no routing, no subscriptions.
- **Stateless:** the bus doesn't track client identity or state. If it restarts, clients reconnect — no state to recover.
- **Client lifecycle:** apps connect, emit topics, receive messages, disconnect. If a client crashes and restarts, it reconnects. No special handling needed.
- **Resilience:** all Sola apps handle bus disconnection gracefully. They retry connection and resume normal operation when the bus is available.
