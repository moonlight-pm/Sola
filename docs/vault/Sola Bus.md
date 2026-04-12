# Sola Bus

The general-purpose IPC bus that all Sola components use to communicate. Hosted by [[sola-bus]] as a separate process.

## Principles

- **The bus is loose.** Any client can put whatever it wants on the bus. Good convention, not enforcement.
- **No built-in correlation.** Request/response linking goes in the payload if needed.
- **[[Wire Format]] is the contract, not Rust types.** Apps can be rebuilt independently. Unknown topics are ignored.

## Behavior

- **Star topology** — every message broadcast to every client
- **Stateless** — no client tracking, no subscriptions, no filtering
- **Resilient** — all apps handle disconnection and reconnect

## Recovery Patterns

1. **Request pattern** — emit a query topic (e.g., `ListApps`), owners respond with current state
2. **Focus-driven refresh** — compositor emits `FocusChanged` on every focus change; apps that need fresh state listen for this and re-request

See also: [[Topics]], [[Input Routing]], [[Wire Format]]
