# Sola call plane

**Date:** 2026-08-13  
**Status:** **Frozen** — infrastructure + compositor/session + kit helper; observer + monitor inspector on master (`monitor-polish` merged 2026-08-21)  
**Related:** [bus freeze](2026-04-09-sola-bus-design.md); Workspaces freeze (other worktree, evidence only); [architecture](../architecture.md); **D3** confirm-policy in [open-questions](../open-questions.md)

## Intent

Sola has two as-built planes: the **bus** (fan-out facts, no correlation) and **Wayland** (pixels / input). Agents and `solactl` need **call semantics**: do this, then tell *me* if it worked.

Do **not** put request/response on the bus. Do **not** grow per-app sockets (`sat` is evidence of one app’s methods, not the home).

## Locked

| Topic | Decision |
|-------|----------|
| Spine | Third plane: `sola-call`, sibling of `sola-bus` |
| App down | **Fail.** Host does not launch Wayland windows. |
| First client | `solactl` (MCP is a later adapter, not the Sola contract) |
| Face | Real CLI: `solactl <owner> <command> …`. Not `solactl call 'sig'` |
| Unknown apps | Anyone may **advertise**. Builtins get a compiled clap tree (`compositor`, `session`, `workspaces`). Other live owners appear as `solactl <app-id>` from the registry |
| River on the CLI | **`compositor`**, not `river` |
| Trust | Local-user Unix socket `0600`, same as the bus |
| Confirm / ACL | **Not in v1.** See **D3** (cousin of D1). Do not invent |
| `sat` | **Declined.** Face is `solactl workspaces …` only. |
| `eval` | Remove. WebView / `Topic::Evaluate` is dead |

## Planes

```text
                    ┌──────────┐
                    │   sola   │
                    └────┬─────┘
           ┌─────────────┼─────────────┐
           ▼             ▼             ▼
      sola-bus      sola-call      river / shell
      fan-out        registry       session / apps
      facts          + replies
```

- **Bus** stays announcements and stickies (`Windows`, `Focus`, `LaunchApp`, theme, menus).
- **Call** is request id, timeout, error to the caller, live method registry.
- Shell Super+Shift / selection screenshot is a **call** (`compositor.screenshot`). Launcher `LaunchApp` stays a bus announcement.

## Process

- Binary / crate: `sola-call` (host + client library, like `sola-bus`)
- Socket: `$XDG_RUNTIME_DIR/sola-call` (override `SOLA_CALL_PATH`)
- Supervisor `MANAGED`: after `sola-bus`, before `sola-river`
- Independently restartable. Providers reconnect and re-advertise.

## Protocol

Length-prefixed JSON (u32 LE + UTF-8). Not MCP. Not postcard.

1. **Hello** — `role: caller | provider | observer`, `app_id`, provider also sends `owner` (CLI noun).
2. **Advertise** — provider: list of methods (name, summary, args).
3. **Invoke** — caller: `owner`, `method`, `params` (JSON object), optional `timeout_ms`. Host forwards to the provider (owner omitted).
4. **Reply** — `ok`, `error?`, `data?`, same `id`.
5. **List / Catalog** — live owners and their advertised methods.
6. **Observer** — hello as `observer`; host pushes `Catalog` on connect and on provider change, plus `Trace` copies of invoke/reply/timeout/advertise/unregister. Same socket privilege as a caller. Not RPC on the bus.

Owner not connected → immediate error (`not running`). No launch.

`session.launch` is a call **on session** (session is up). It may start another process. That is not a call-host side effect.

## `solactl` (v1)

```text
solactl compositor screenshot [--app] [--window] [-o PATH]
solactl compositor windows
solactl compositor input click|move|scroll|key …
solactl session launch <app_id> [--command]
solactl session close  <app_id>
solactl emit | logs | open | media     # not calls
solactl <app-id>                       # live extra owner: list advertised commands
solactl <app-id> <method> [json|--flags]
```

`--help` at every compiled level works with the owner down. **Invoke** fails if the owner is not connected. Call host down → fail (do not fall back to bus RPC).

Removed: `eval`, top-level `apps` / `screenshot` / `click` / `move` / `scroll` / `key`.

`open` is launch-shaped (scheme handler): spawns **sola-browser** with the
URL. No alternate browser. Not a fail-if-down call (fails if the binary is
missing).

## Providers in this slice

| CLI owner | Process | Methods |
|-----------|---------|---------|
| `compositor` | `sola-river` | `screenshot` (`format=png` default, or `rgba` packed RGBA8 freeze), `sample`, `windows`, `input.click`, `input.move`, `input.scroll`, `input.key` |
| `session` | `sola-session` | `launch`, `close` |

Kit apps do **not** grow commands here. Agent-terminal will advertise when that worktree resumes.

## Later (do not invent policy; park here)

These are real follow-ups. Not v1 blockers.

| Item | Why later |
|------|-----------|
| **MCP adapter** | Translator in front of the Sola protocol so Grok attaches as it already knows. Not the internal contract. |
| **D3 confirm / ACL** | Which methods need a human in the loop; who owns the prompt. Cousin of D1. |
| **Catalog sticky on the bus** | `sola-call` could emit live owners as a fact so monitor can audit without speaking RPC. |
| **`sat` alias** | **Declined (2026-08-14).** Workspaces face is `solactl workspaces …`. |
| **`LaunchResult`** | Leftover reply on the bus. `LaunchApp` stays an announcement (launcher → session). Session already replies on the call for `session.launch`. Shell “Opening…” toast still listens to `LaunchResult`. Move that toast to a call or keep the fact. |
| **`CloseApp` as call** | Meta+Q is still a bus poke. Could be `session.close` with a real error. |
| **`media.*` methods** | Today the shell execs `solactl media`. Same verbs could register if agents need them. |
| **`open` single-instance** | **Done on browser-polish (2026-08-15):** `chrome.sock` handoff; live chrome gets the URL; shell only spawns if chrome is down. |
| **Host built-ins** | `ping`, richer `list` filters, cancel in-flight. |
| **Monitor UI** | **Done (merged 2026-08-21):** observer role + traces; live owners/methods in sola-monitor. Catalog sticky on the bus still later. |
| **Dogfood / install** | Supervisor will not start `sola-call` until this worktree is installed. |
| **Workspaces methods** | First kit consumer of `CallSetup` / `BusSetup::calls`. Owner `ws`; CLI freeze [`2026-08-18-workspaces-cli-design.md`](2026-08-18-workspaces-cli-design.md). Desk smoke pending. |

**Dropped in this slice (were fake request/reply on the bus):** `Evaluate` / `Evaluation`, `CaptureScreen` / `Screenshot`, `SimulatePointer` / `SimulateKey`. Payload types `CaptureScreenPayload`, `CaptureTarget`, `PointerAction`, `PointerButton` remain for the call path.

## Implementation

**Code:** `crates/sola-call`; supervisor + install order; river/session providers; `solactl` tree; kit [`CallSetup`] / `BusSetup::calls` / `call_subscription`; `Role::Observer` + `Wire::Trace` + kit `install_observer` / `observe_subscription`; sola-monitor Bus/Call inspector; shell screenshot via call; `compositor.sample` (RGBA patch around the pointer, for sola-scope). `MethodSpec.timeout_ms` is an optional advertised deadline (`solactl` live owners use it).  
**Dogfood:** `cargo make install call monitor` ran 2026-08-21 (debug); desk smoke pending.  
**Gaps:** see Later; D3; monitor desk smoke pending; catalog sticky still not on the bus.
