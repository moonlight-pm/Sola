# sola-mail — kit-native design

**Date:** 2026-07-27  
**Branch:** `sola-mail`  
**Status:** approved for implementation  
**Supersedes:** `docs/specs/2026-04-20-sola-mail-design.md` (WebView / `sola-app` era)  
**Reference:** `apocrypha/apps/mail/` (logic + UX parity source)

## Goal

Ship `crates/sola-mail`: a **sola-kit** (iced) desktop mail client with **feature parity** to the apocrypha WebView mail app — IMAP list/fetch/move/search, SMTP send, IDLE push, move rules, smart mailboxes, wicket from-addresses, keyboard shortcuts, and link open via the bus — as a single-process, single-window kit app.

## Decisions (locked)

| Topic | Choice |
|---|---|
| v1 scope | Feature parity with apocrypha mail |
| UI stack | sola-kit / iced (no WebView) |
| Message bodies | Prefer `text/plain`; if only HTML, strip to readable plain text (links as text + URL). No embedded HTML engine |
| Compose | Plain text only (parity). Same window, mode swap (compose replaces message pane) |
| Process model | In-process background worker (agent-style), not a bus daemon |
| Architecture | Layered single crate **B** — `protocol/` + `worker/` + `ui/` |
| Config | Consume sticky `Topic::MailConfig` from the bus (edited by sola-settings). No in-app account editor |
| Accounts | Single account (matches bus `MailConfig`) |
| Location | `crates/sola-mail` |

## Non-goals (v1)

- Multi-account
- Attachments (send or receive) beyond whatever the protocol lift already surfaces in summaries
- Rich HTML compose or full HTML render
- Separate `sola-mail-d` service / multi-client daemon
- In-app settings UI (settings already owns account + rules)
- Offline local store / full cache (live IMAP like the reference app)
- Calendar / contacts integration

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  sola-mail (single process)                                 │
│                                                             │
│  iced UI thread                    mail worker thread       │
│  · MailApp state                   · typed MailCmd loop     │
│  · folder / list / message /       · IMAP session           │
│    compose views                   · SMTP send              │
│  · keyboard shortcuts              · IDLE + keepalive       │
│  · bus: Theme, MailConfig,         · rules apply on IDLE    │
│    MenuAction, OpenUrl             · wicket from-addresses  │
│         │                                │                  │
│         │  MailCmd (mpsc)                │                  │
│         └───────────────────────────────►│                  │
│         │  MailEvent (mpsc → iced)       │                  │
│         ◄───────────────────────────────┘                  │
└─────────────────────────────────────────────────────────────┘
         │ bus (Unix socket)
         ▼
  sola-settings  ──► Topic::MailConfig (sticky)
  sola-browser   ◄── Topic::OpenUrl
  sola-shell     ◄── Topic::SetAppMenu / MenuAction
```

### Process model

- **`startup(APP_ID)`** → **`BusSetup`** → **`iced::application`** (same chassis as settings / agent).
- On boot, spawn a named OS thread (`sola-mail-worker`) that owns:
  - blocking IMAP (`imap` + rustls connector),
  - SMTP (`lettre`),
  - IDLE watcher task / reconnect,
  - optional wicket HTTP fetch (`ureq`).
- UI never blocks on network. All protocol work is `MailCmd` → worker → `MailEvent`.
- App exit: UI sends `MailCmd::Shutdown`; worker drops IDLE/keepalive and exits.

Rationale: matches sola-agent’s proven pattern; keeps restartability as one unit; no new bus topic surface for v1.

### Module layout

```
crates/sola-mail/
  Cargo.toml
  src/
    main.rs              — startup, BusSetup, iced::application, menu
    bridge.rs            — global cmd_tx / event_rx (or channels installed at boot)
    protocol/
      mod.rs
      types.rs           — Folder, MessageSummary, MessageBody, rule types used by UI+worker
      rules.rs           — matching (port apocrypha rules + former TS smart-mailbox fns)
      imap.rs            — ImapClient (lift apocrypha)
      idle.rs            — IDLE watcher (lift)
      sender.rs          — SMTP (lift)
      wicket.rs          — from-address fetcher (lift)
      html_text.rs       — HTML → plain text helper
    worker/
      mod.rs             — thread entry, cmd loop
      cmds.rs            — MailCmd / MailEvent enums + handlers
    ui/
      mod.rs             — MailApp state, update, view, subscription
      folder_list.rs
      message_list.rs
      message_view.rs
      compose.rs
      toast.rs           — inline error / status (kit components)
```

Single crate, many small files. No `sola-mail-core` split (YAGNI).

### Typed worker API (not JSON)

Replace apocrypha’s string command bag with Rust enums. Shape (illustrative):

```rust
pub enum MailCmd {
    Connect,                         // use latest MailConfig known to worker
    Reconfigure(MailConfig),         // bus pushed new credentials/rules
    ListFolders,
    ListMessages { folder, offset, limit },
    Search { folder, query },
    FetchBody { folder, uid },
    MarkRead { folder, uid },
    Move { folder, uid, dest },
    EmptyFolder { folder },
    Send { from, to, cc, subject, body, in_reply_to },
    ApplyRules,
    Shutdown,
}

pub enum MailEvent {
    Connected { folders, smart_counts, from_addresses, rules },
    Folders { folders, smart_counts },
    Messages { folder, messages, total, offset },
    SearchResults { messages, total },
    Body(MessageBody),
    Sent,
    Moved { folder, uid, dest },
    Emptied { folder },
    RulesApplied { moved },
    NewMail,                         // IDLE nudge → UI refreshes
    Error { context, message },
    Disconnected { reason },
}
```

Worker holds current `MailConfig` (seeded by `Reconfigure` / first `Connect`). UI holds display state only.

## Config & bus

### Source of truth

- **sola-settings** edits account + rules and emits sticky `Topic::MailConfig`.
- **sola-mail** subscribes and on each delivery:
  1. stores config in UI state for display needs (from address default, rules for smart mailboxes),
  2. sends `MailCmd::Reconfigure(cfg)` (or `Connect` if first time).
- Missing / empty credentials → disconnected UI with a clear “configure mail in Settings” message — no local `mail.toml` loader.

Password field is `Encrypted<String>` on the bus wire form; worker receives the decrypted in-process value the same way settings does when saving.

### Other bus topics

| Topic | Role |
|---|---|
| `Topic::Theme` | kit theme + fonts via `apply_theme_update` |
| `Topic::SetAppMenu` | publish Mail menu at boot |
| `Topic::MenuAction` | quit |
| `Topic::OpenUrl` | emit when user activates a link in a message body |
| `Topic::MailConfig` | account + rules (sticky) |

No new topics for v1.

### Menu (v1)

Parity with apocrypha: one app menu **Mail** with **Quit Mail** (`Meta+Q`). Message actions stay keyboard/UI buttons; menu expansion is a follow-up.

## UI

### Layout

Single iced window, three columns (kit `split` / sidebar + panes):

```
┌──────────┬────────────────────┬──────────────────────────┐
│ Folders  │ Message list       │ Message view  OR compose │
│ + smart  │ search, bulk ops   │ headers, body, actions   │
│ mailboxes│ infinite scroll    │                          │
└──────────┴────────────────────┴──────────────────────────┘
```

- **Folder list:** real IMAP folders + smart mailboxes derived from rules (`action == "smart_mailbox"`). Unread/total badges.
- **Message list:** summaries for selected folder; search; load-more; archive-all / trash-all when applicable; empty-folder for Trash/Junk-style folders (parity).
- **Message view:** subject/from/to/cc/date; plain-text body (or HTML→text); Reply / Reply All / Archive / Trash / Compose entry points.
- **Compose mode:** replaces the right pane (or right two panes if density requires — default: replace message pane only). Fields: From (pick from wicket list), To, Cc, Subject, body. Send / Cancel.

Loading / fatal / toast states mirror the reference app without WebView chrome.

### Smart mailboxes

Parity behavior:

- Rules with `action: "smart_mailbox"` appear as virtual folders (`smart:<name>` or equivalent internal id).
- Listing a smart mailbox filters INBOX (or cached inbox window) by rule match — same semantics as apocrypha TS `matchesRule` / `matchesAnySmartMailbox`, implemented in Rust `protocol::rules`.
- Moves from a smart mailbox operate on the real source folder (INBOX).

### Message body rendering

1. If `MessageBody.text` is non-empty after fetch → show it.
2. Else if `html` present → `html_text::to_plain(html)` (prefer a maintained crate such as `html2text`, or a small sanitizing stripper if dependency weight is undesirable — pick during implementation; unit-test a few fixtures).
3. Link activation: detect URLs in the displayed text (or structured link spans if the converter yields them) and emit `Topic::OpenUrl { url, activate: true }`.

No HTML widget tree in v1.

### Keyboard shortcuts

When not composing and focus is not a text input:

| Key | Action |
|---|---|
| `j` | Move selected → Junk, advance |
| `i` | Move selected → INBOX, advance |
| `a` | Move selected → Archive, advance |
| `d` | Move selected → Trash, advance |
| `u` | Undo last move |
| `w` | Previous message |
| `s` | Next message |

### Theme / kit components

- Use sola-kit: `sidebar`, `split`, `button`, `field` / `text_input`, `text`, `badge`, `card` as needed, `toolbar` where it fits.
- Graphite DS tokens via bus theme; no app-local palette snowflakes.
- Density matches agent/settings (HIG-ish tooling density).

## Protocol layer (lift)

Lift from `apocrypha/apps/mail/src/` with light cleanup:

| Apocrypha | Kit crate |
|---|---|
| `imap.rs` | `protocol/imap.rs` |
| `idle.rs` | `protocol/idle.rs` |
| `sender.rs` | `protocol/sender.rs` |
| `wicket.rs` | `protocol/wicket.rs` |
| `rules.rs` + TS matchers | `protocol/rules.rs` + `types.rs` |
| `handler.rs` commands | `worker/cmds.rs` (typed) |
| `config.rs` file load | **drop** — bus `MailConfig` only |
| `state.rs` | worker-owned connection state |

### Config type mapping

Use bus `sola_bus::topics::MailConfig` / `MailRule` / `MailRuleCondition` as the wire and app config type. Adapt protocol code that previously used a local `MailConfig` (plain password string) via a small internal view or field mapping at the worker boundary (`password.0` / decrypt semantics as elsewhere in sola).

### Dependencies (mail-specific)

Approximate (pin in crate or workspace as existing practice dictates):

- `imap` (TLS via rustls connector path used in apocrypha)
- `rustls` + `rustls-connector`
- `lettre` (SMTP, rustls)
- `mail-parser`
- `ureq` (wicket)
- `base64`
- `html2text` or equivalent for HTML→text
- `tokio` only if idle/worker needs it; prefer matching apocrypha’s blocking + thread model unless idle already depends on tokio — keep lift mechanical where possible

Install rustls crypto provider once at process start (same as apocrypha).

### IDLE & refresh

- On connect: start IDLE + keepalive as in apocrypha.
- On IDLE new mail: apply move rules; emit `MailEvent::NewMail`.
- UI on `NewMail` (and optional focus/time-based refresh): re-list folders + current folder messages; do not spam toasts on refresh failure.

## Data flow (happy paths)

### Connect

1. Bus delivers `MailConfig` (or app starts and waits for sticky replay).
2. UI → `MailCmd::Connect` / `Reconfigure`.
3. Worker connects IMAP, lists folders, fetches wicket addresses (fallback `[email]`), starts IDLE, returns `Connected`.
4. UI selects INBOX, requests first page of messages.

### Open message

1. UI → `FetchBody`.
2. Worker returns body; UI shows text/HTML→text.
3. If unseen → `MarkRead` + local unread decrement.

### Send

1. Compose mode → `Send`.
2. Worker SMTP; on success `Sent` + UI exits compose and may refresh Sent/INBOX as parity requires.

### Move / undo

1. `Move` + local list update + `last_move` stash.
2. `u` issues reverse `Move`.

## Error handling

- Worker maps failures to `MailEvent::Error { context, message }` (e.g. `context: "connect" | "fetch" | "send" | ...`).
- UI: toast/banner for user-initiated ops; silent/log for background IDLE refresh failures.
- Disconnect: clear connection-dependent state; show reconnect path (auto-retry on next `Reconfigure` / explicit reconnect when config present).
- Never lose errors only to TTY — `tracing` to sola log path via kit startup.

## Testing

### Automated

- Port / rewrite rule-matching unit tests from apocrypha `rules.rs` (+ former TS cases for domain/address/contains/equals on from/to/subject).
- `html_text` fixtures: simple markup, links, nested tags → stable plain text.
- Worker command handlers with a mock/fake IMAP are optional for v1; not required if mechanical lift is covered by manual smoke.

### Manual smoke (parity checklist)

1. Launch with valid MailConfig from settings → folders + INBOX.
2. Open message → body; auto mark-read; unread counts update.
3. Compose → send → appears in Sent.
4. `d` → Trash + advance; `u` restores.
5. IDLE new mail → list/counts refresh.
6. Search → filter; clear → restore.
7. Smart mailbox → rule-matched only.
8. Link in body → sola-browser via `OpenUrl`.
9. Meta+Q → clean exit.
10. Empty / invalid config → clear “configure in Settings” state (no panic).

## Build / install / runtime

- Workspace member via `crates/*` (new directory auto-members).
- Binary name: `sola-mail`, `APP_ID = "sola-mail"`.
- `cargo make build mail` / `cargo make build sola-mail` — confirm sola-make short-name resolution during scaffold; register if needed.
- **Do not** `cargo make install` without explicit user permission.
- Self-watch binary re-exec via kit `startup` (same as other apps).
- Desktop launch: user/session launches like other sola apps; no special session manager change required if apps are discovered by binary name / existing conventions — verify against how agent/settings are registered during implementation.

## Historical note

The April 2026 design/plan targeted `sola-app` + Arrow.js under `apps/mail`. That stack is retired under `apocrypha/`. Protocol and UX intent remain valid; host, UI, and config path are fully replaced by this document.

## Implementation milestones (high level)

1. Scaffold crate + kit shell (empty panes, menu, theme bus).
2. Bridge + worker stub (Connect/Error events).
3. Lift `protocol` (types, rules+tests, imap, idle, sender, wicket, html_text).
4. Wire `MailConfig` bus → Reconfigure/Connect.
5. Folder list + message list + open body + mark read.
6. Compose/send, move/undo, search, bulk, empty folder.
7. IDLE NewMail refresh, smart mailboxes, shortcuts, OpenUrl.
8. Polish loading/error/empty states; full smoke checklist.

Detailed task breakdown lives in the companion implementation plan.
