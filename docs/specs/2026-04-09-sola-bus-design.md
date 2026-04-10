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

- **`sola`** is a pure process manager. It launches `sola-bus`, `sola-compositor`, and all shell apps. It restarts any that crash. It has no desktop logic, no bus logic, no Wayland logic. It almost never needs to change.
- **`sola-bus`** is a separate process that hosts the bus. Creates the Unix socket, accepts connections, broadcasts all messages to all clients. Stateless — if it restarts, clients reconnect.
- **`sola-compositor`** is a separate process. Wayland compositor + bus client.
- **Shell apps** (switcher, launcher, panel, etc.) are separate processes. Wayland clients + bus clients. Sola apps are resilient to compositor and bus restarts.

No launch ordering is required. All Sola apps handle missing bus or compositor connections gracefully and reconnect when available.

### Workspace

```
crates/
  sola/              # process manager
  sola-bus/          # bus host process + protocol definitions
  sola-compositor/   # Wayland compositor (bus client)
```

## Principles

- **Everything is an event.** There is one message type. No registration, no discovery, no RPC. Apps connect, listen for topics they care about, and emit events.
- **The bus is loose.** Any client can put whatever it wants on the bus. Good convention (not enforcement) keeps things orderly.
- **No built-in correlation.** If an event needs to reference another event, that goes in the payload. The bus itself doesn't know or care about request/response patterns.
- **Wire format is the contract, not Rust types.** Apps can be rebuilt independently. `sola-protocol` provides convenience types but isn't a hard compile-time coupling.

## Wire Format

Every message on the bus:

```
id:      UUIDv7          // unique, monotonic, binary (16 bytes)
                         // embeds ms-precision timestamp; extract for logging
topic:   String          // e.g. "shell:show-switcher", "shell:apps"
payload: Option<Bytes>   // arbitrary binary, deserialized by consumer
```

Three fields. That's the entire protocol.

**Transport:** `postcard` (serde-compatible, compact binary). Auto-serializes to JSON for human-readable logs.

**Socket:** `$XDG_RUNTIME_DIR/sola-bus` (Unix stream, star topology — sola-bus broadcasts every message to all connected clients).

## Topic Convention

Topics use the format `category:event-name`:

- `shell` — core shell concerns (app list, raise, focus, switcher, launcher)
- Future apps use their own categories

Examples: `shell:show-switcher`, `shell:apps`, `shell:raise-app`

## Component Responsibilities

### sola (crate / process)

- Launches `sola-bus`, `sola-compositor`, and all shell apps as child processes
- Watches for crashes, restarts processes
- No bus logic, no desktop logic — pure process management

### sola-bus (crate / process)

- Creates the bus socket at `$XDG_RUNTIME_DIR/sola-bus`
- Accepts client connections
- Broadcasts all messages to all connected clients
- Stateless — no client tracking, no filtering, no routing

### sola-bus (crate / process)

Also contains the protocol definitions:

- Bus message struct with serde derives (postcard for wire, JSON for logs)
- UUIDv7 generation and timestamp extraction
- Transport helpers: framed read/write over Unix socket
- Convenience types for common payloads — optional, not required

Clients depend on `sola-bus` as a library for the message types and transport helpers, while the bus itself runs as a separate binary.

## Bus Behavior

- **Star topology:** all apps connect to `sola-bus`. Every message is broadcast to every connected client. No filtering, no routing, no subscriptions.
- **Stateless:** the bus doesn't track client identity or state. If it restarts, clients reconnect — no state to recover.
- **Client lifecycle:** apps connect, emit events, receive events, disconnect. If a client crashes and restarts, it reconnects. No special handling needed.
- **Resilience:** all Sola apps handle bus disconnection gracefully. They retry connection and resume normal operation when the bus is available.
