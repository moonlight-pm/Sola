# sola-mail — kit-native design

**Date:** 2026-07-27  
**Branch:** master (merged from sola-mail)  
**Status:** implemented (partial) — 2026-10-19 rework: coordinator worker + per-account read / write / IDLE sessions (3 IMAP logins, was 4 + keepalive); config edits reconnect only accounts whose IMAP login changed; failed accounts retry with backoff (and **Retry Now**); list rows carry account + mailbox + UID (search hits open / move in their own mailbox); headers RFC 2047-decoded via mail-parser; dates are unix seconds formatted locally; cursor paging (no re-fetch from the top); shortcuts ignore keys a text field used; Esc / row click ask before discarding an edited draft; Forward (with files); Reply-To + References + `<…>` In-Reply-To; Message-ID on sent mail; Gmail Sent not appended twice; Drafts save / reopen; mark read/unread + flag; erase confirm; `UID EXPUNGE` when UIDPLUS; compose Tab order, address suggestions, per-address check on Send; a recipient the server refuses is named and worker failures are logged. Earlier: dest-UID undo, 5s toasts, move rules on connect + IDLE, attachments, send identities, multi-IMAP canonical boxes, Gmail Archive create, snapshot boot + connect card; focusing the window re-lists the open folder (ring beside the count while a first page is out)
**Supersedes:** `docs/specs/2026-04-20-sola-mail-design.md` (WebView / `sola-app` era)  
**Reference:** `apocrypha/apps/mail/` (logic + UX parity source)  
**Gaps:** no HTML engine (converted letter; CID images are files, not inline); no drag-drop onto compose; no full offline store (last-session list snapshot only); IDLE watches each account INBOX only; undo dest-UID if COPYUID and Message-ID both missing; move rules on connect scan the newest 500 INBOX only; search is Enter-only over Inbox/Sent/Archive/Drafts (+ open Junk/Trash), 200 hits; Edit → Paste lands in the compose body only; quiet refresh covers the newest 150 rows (older remote deletes show on reopen)

**Compose:** From / To / Cc / Subject sit on one line with the label (table), caption-size type, full reader-pane width (not the letter 640px measure). Body is the remaining well at kit body 13px. Action toasts auto-dismiss after 5 seconds.

## Goal

Ship `crates/sola-mail`: a **sola-kit** (iced) desktop mail client with **feature parity** to the apocrypha WebView mail app — IMAP list/fetch/move/search, SMTP send, IDLE push, move rules, smart mailboxes, wicket from-addresses, keyboard shortcuts, and link open via the bus — as a single-process, single-window kit app.

## Decisions (locked)

| Topic | Choice |
|---|---|
| v1 scope | Feature parity with apocrypha mail |
| UI stack | sola-kit / iced (no WebView) |
| Message bodies | Prefer HTML when present (what mail apps render); convert with `html2text` rich + kit `prose`. Tracking hrefs show as a short label. No embedded HTML engine |
| Compose | Plain text only (parity). Same window, mode swap (compose replaces message pane) |
| Process model | In-process background worker (agent-style), not a bus daemon |
| Architecture | Layered single crate **B** — `protocol/` + `worker/` + `ui/` |
| Config | Consume sticky `Topic::MailConfig` from the bus (edited by sola-settings). No in-app account editor |
| Accounts | Every `imap_enabled` account is connected. The sidebar is the six canonical boxes (Inbox, Sent, Drafts, Archive, Junk, Trash) **combined** across accounts. Remote names (Gmail `[Gmail]/Trash`, `\Sent`, …) map in; All Mail / Starred / Important / extra labels stay off the list. Gmail with no Archive mailbox gets `CREATE Archive` (a user label) on connect. Each message carries `account` + UID so Trash/Archive/undo hit **that** account's mapped mailbox. Extra accounts may still be SMTP-only (`imap_enabled` off). |
| Location | `crates/sola-mail` |

## Non-goals (v1)

- Showing provider-specific labels / `[Gmail]/All Mail` as extra sidebar rows
- Rich HTML compose or full HTML render (CID images list as files)
- Drag-drop onto compose
- Separate `sola-mail-d` service / multi-client daemon
- In-app settings UI (settings already owns account + rules)
- Offline local store / full cache (live IMAP like the reference app)
- Calendar / contacts integration

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  sola-mail (single process)                                      │
│                                                                  │
│  iced UI thread            coordinator thread (sola-mail-worker) │
│  · App state (list,        · MailConfig + shared rules           │
│    reader, compose, moves) · account slots (connect / retry)     │
│  · keyboard / menus        · fan-out jobs → merge replies        │
│  · bus: Theme, MailConfig, · SMTP send on its own thread         │
│    MenuAction, CloseApp,         │                               │
│    Windows, WindowFloating       ├─► account A: read · write · IDLE│
│         │  MailCmd               └─► account B: read · write · IDLE│
│         └────────────────►  (Input channel; sessions reply here) │
│         ◄──────────────── MailEvent (sessions emit UI events)    │
└──────────────────────────────────────────────────────────────────┘
         │ bus (Unix socket)
         ▼
  sola-settings  ──► Topic::MailConfig (sticky)
  sola-shell     ◄── Topic::SetAppMenu / MenuAction, Topic::MailStatus
```

### Process model

- **`startup(APP_ID)`** → panic filter → **`BusSetup`** → **`iced::application`**.
- The **coordinator** owns the config and the account set. Each connected
  account is an `AccountHandle` with three IMAP sessions on their own
  threads: **read** (lists, counts, search, bodies, flags; keeps only the
  newest body request in a burst; NOOP when idle), **write** (MOVE,
  delete, erase, Sent / Drafts append, move rules), **IDLE** (INBOX).
- List / count / search / erase are **jobs**: one request per account,
  merged when every account answered (an account that drops out is
  removed from the job). Bodies, flags and moves go straight to that
  message's account.
- `Reconfigure` diffs the account set: unchanged IMAP logins keep their
  sessions; a rules-only edit just swaps the shared rules. Failed connects
  retry at 15 s → 5 min; **Get New Mail** retries now.
- UI never blocks on network. Exit: `MailCmd::Shutdown` stops every
  session (IDLE shuts its socket so it stops at once).

### Module layout

```
crates/sola-mail/src/
  main.rs            — startup, panic filter, BusSetup (narrow topics), menus
  bridge.rs          — UI ↔ worker channels
  snapshot.rs        — last-session list (versioned)
  protocol/
    types.rs         — FolderRef, MailId (account+mailbox+uid), summaries, bodies
    parse.rs         — FETCH → summary / body (mail-parser headers, reading blocks)
    imap.rs          — ImapClient (reconnect-once, cursor paging, MOVE, erase)
    dest_uid.rs      — COPYUID + Message-ID fallback for undo
    idle.rs          — IDLE watcher (socket-shutdown stop)
    sender.rs        — Outgoing build + SMTP (STARTTLS / 465)
    rules.rs         — move / smart rule matching + IMAP SEARCH criteria
    boxes.rs, account.rs, from.rs, date.rs, html_text.rs, attachments.rs
  worker/
    mod.rs           — coordinator, jobs, account slots
    account.rs       — AccountHandle (connect, map boxes, spawn sessions)
    read.rs, write.rs, send.rs, merge.rs, cmds.rs
  ui/
    mod.rs           — App, Msg, update, view
    list.rs, list_sync.rs, reader.rs, compose.rs, moves.rs,
    sidebar.rs, chrome.rs, keys.rs, format.rs, style.rs, widgets.rs
```

### Typed worker API

```rust
pub enum MailCmd {
    Reconfigure(MailConfig), Retry, ListFolders,
    List { epoch, folder: FolderRef, page: Page, cursors, limit },
    Search { epoch, query, also },
    FetchBody(MailId), SetFlag { id, flag, on }, Move { id, dest },
    Empty { folder }, Send(Outgoing), SaveDraft(Outgoing), Shutdown,
}

pub enum MailEvent {
    AccountLink { account, email, state }, Changed, NotConfigured,
    Folders { folders, smart },
    Messages { epoch, folder, page, rows, total, failed },
    SearchResults { epoch, rows, capped },
    Body(Box<MessageBody>), Moved { id, dest, dest_uid }, MoveFailed { id, message },
    Emptied { folder, failed }, Sent, DraftSaved, NewMail,
    Failed { op: Op, message },
}
```

`epoch` lets the UI drop pages for a list it already left. `Page::More`
continues below each account's oldest UID on screen (sequence windows, no
SEARCH). `failed` names accounts whose part failed so their rows stay.

## Config & bus

### Source of truth

- **sola-settings** edits accounts + rules and emits sticky `Topic::MailConfig`.
- Inbox credentials stay on the top-level fields (old configs load). Inbox **Send from** is the host’s address list (`GET /api/auth/me`); the operator checks which ones Mail offers. Catch-alls (`*@…`) are omitted. Extra accounts still type aliases (`from_hidden` for those). `primary_from` is the default From. Compose does not dump the host’s full alias list.
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

### Menus

Mail (Get New Mail, Quit), Edit, Mailbox (Erase Junk / Deleted Items — both confirm), Message (New, Save Draft ⌘S, Attach, Reply, Reply All, Forward ⌘⇧F, Mark Read/Unread ⌘⇧U, Flag ⌘⇧L, moves, Undo), View (next / previous).

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

- **Folder list:** real IMAP folders + smart mailboxes derived from rules (`action == "smart_mailbox"`). Unread-only badges (hidden when 0).
- **Message list:** summaries for selected folder; search; load-more; archive-all / trash-all when applicable; empty-folder for Trash/Junk-style folders (parity).
- **Message view:** letter header (subject, person + address, date with local time under it); kit `prose` body (paragraphs, quotes, inline links); file list (Open / Save) when the MIME has attachments; Reply / Reply All / Archive / Trash / Copy.
- **Compose mode:** replaces the right pane (or right two panes if density requires — default: replace message pane only). Fields: From (kit select of settings identities: Default From first, then A–Z; unchecked aliases and catch-alls like `*@domain` are omitted; reply preselects the identity in the original To/Cc), To, Cc, Subject, body. Attach (toolbar paperclip, Message → Attach Files…, or Attach next to Send) via kit FilePicker. Send / Cancel. SMTP follows the From identity (extra account vs inbox).
- **List:** paperclip on rows whose IMAP `BODYSTRUCTURE` has a user-facing file.

Loading / fatal / toast states mirror the reference app without WebView chrome.

### Smart mailboxes

Parity behavior:

- Rules with `action: "smart_mailbox"` appear as virtual folders (`smart:<name>` or equivalent internal id).
- Listing a smart mailbox filters INBOX (or cached inbox window) by rule match — same semantics as apocrypha TS `matchesRule` / `matchesAnySmartMailbox`, implemented in Rust `protocol::rules`.
- Moves from a smart mailbox operate on the real source folder (INBOX).

### Message body rendering

1. Prefer the HTML part whenever it is present (what mail apps render).
   Convert with `html2text` rich + raw tables, no link footnotes.
2. Render through kit `prose`: paragraphs, `>` quotes, inline links.
   Tracking / click-wrapper hrefs show as a short label (“Link”), never
   the raw `upn=` URL. First-party destinations (magic links) keep
   host+path; a lone long URL is not dropped as tracking.
   Inbox unread is published as sticky `Topic::MailStatus` for the
   shell menubar (chip hidden when mail is not mapped).
3. Link activation: `on_link` → `Topic::OpenUrl` / `open_url_logged`.
4. Copy / reply use `flatten` of those blocks (toolbar Copy Message).
   In-body drag-select is kit `prose` (visible text; Edit Copy / Select All).
5. Menus: Mail, Edit, Mailbox, Message, View — not one stuffed app menu.

No HTML widget tree in v1.

### Keyboard shortcuts

When not composing and focus is not a text input:

| Key | Action |
|---|---|
| `x` | Move selected → Junk, advance (Message menu) |
| `i` | Move selected → INBOX, advance (Message menu) |
| `a` | Move selected → Archive, advance (Message menu) |
| `d` | Move selected → Trash, advance (Message menu) |
| `u` | Undo last move (Message menu) |
| `w` / ↑ | Previous message (View menu) |
| `s` / ↓ | Next message (View menu) |

### Theme / kit components

- Use sola-kit: `sidebar`, `split`, `button`, `field` / `text_input`, `text`, `prose`, `readable`, `badge`, `card` as needed, `toolbar` where it fits.
- Graphite DS tokens via bus theme; no app-local palette snowflakes.
- Density matches agent/settings (HIG-ish tooling density).

## Protocol layer (lift)

Lift from `apocrypha/apps/mail/src/` with light cleanup:

| Apocrypha | Kit crate |
|---|---|
| `imap.rs` | `protocol/imap.rs` |
| `idle.rs` | `protocol/idle.rs` |
| `sender.rs` | `protocol/sender.rs` |
| `wicket.rs` | **dropped** (Settings owns the Wicket From checklist) |
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
- `html2text` or equivalent for HTML→text
- `tokio` only if idle/worker needs it; prefer matching apocrypha’s blocking + thread model unless idle already depends on tokio — keep lift mechanical where possible

Install rustls crypto provider once at process start (same as apocrypha).

### IDLE & refresh

- On connect: the write session applies `action == "move"` rules to the newest 500 INBOX rows (UI refresh only if something moved); IDLE starts on its own connection. After an IDLE reconnect the sweep runs again.
- On IDLE new mail: the write session applies move rules to the newest `max(20, arrivals)`, then `MailEvent::NewMail`. No move rules → `NewMail` straight away.
- From/To `equals` (and `address`) match the envelope address inside `Name <addr>`, not only the raw header string.
- UI on `NewMail` (and optional focus/time-based refresh): re-list folders + current folder messages; do not spam toasts on refresh failure.

## Data flow (happy paths)

### Connect

1. Bus delivers `MailConfig` (or app starts and waits for sticky replay).
2. UI → `MailCmd::Connect` / `Reconfigure`.
3. Coordinator connects each new account on its own thread; each one up emits `AccountLink Ready` + `Changed`.
4. UI refreshes folders and the shown list (snapshot rows merge; empty list replaces).

### Open message

1. UI → `FetchBody`.
2. Worker returns body; UI shows text/HTML→text.
3. If unseen → `MarkRead` + local unread decrement.

### Send

1. Compose mode → `Send` (plain, or `multipart/mixed` when files are attached).
2. Worker SMTP; on success `Sent` + UI exits compose and may refresh Sent/INBOX as parity requires.

### Move / undo

1. `Move` + local list update + `last_move` stash (source UID + dest folder).
2. Worker returns the **destination UID** (IMAP UIDs are per-mailbox). `UID MOVE` COPYUID when untagged; otherwise scan recent dest envelopes by Message-ID.
3. `u` restores the row locally, then reverse-`Move`s **that dest UID** back. It never uses the source UID against Trash/Junk (that number is a different message). If dest UID is not in yet, undo waits for `Moved`.

## Error handling

- Worker maps failures to `MailEvent::Failed { op, message }` (`Op::Open(id)` shows in the reading pane with Try Again; list failures stay silent; send / draft failures re-enable compose).
- UI: toast/banner for user-initiated ops (auto-dismiss after 5s); silent/log for background IDLE refresh failures.
- Connect failure: the account card shows the error; retries back off 15 s → 5 min; **Retry Now** / Get New Mail retries at once. A dropped session reconnects once per command.
- Never lose errors only to TTY — `tracing` to sola log path via kit startup.

## Testing

### Automated

- Port / rewrite rule-matching unit tests from apocrypha `rules.rs` (+ former TS cases for domain/address/contains/equals on from/to/subject).
- `html_text` fixtures: simple markup, links, nested tags → stable plain text.
- Attachment parse fixtures (mixed PDF, nested rfc822 flatten) + SMTP `multipart/mixed` round-trip.
- Worker command handlers with a mock/fake IMAP are optional for v1; not required if mechanical lift is covered by manual smoke.

### Manual smoke (parity checklist)

1. Launch with valid MailConfig from settings → folders + INBOX.
2. Open message → body; auto mark-read; unread counts update.
3. Compose → send → appears in Sent.
3a. Attach a file in compose (paperclip / Attach Files…) → send → recipient (or Sent) shows the file.
3b. Open a message with a file → Open (paint for images, browser else) / Save (kit picker, default Downloads).
4. `d` → Trash + advance; `u` restores **that** message (destination UID, not the source UID in Trash).
5. IDLE new mail → list/counts refresh.
6. Search → filter; clear → restore.
7. Smart mailbox → rule-matched only.
8. Link in body → sola-browser via `OpenUrl`.
9. Meta+Q → clean exit.
11. Type `dad` in Search with a letter selected → nothing moves.
12. Search hit from Sent / Archive → opens that letter; `d` trashes that one.
13. Esc in a typed draft → Discard? bar (Keep Editing / Save Draft / Discard). Save Draft → Drafts; open it → Edit draft → send replaces it.
14. Forward → files come along; Reply to a list with Reply-To → goes to the list.
15. Mark unread / Flag from the toolbar; flag shows on the row.
16. Compose: Tab / Shift+Tab walk To → Cc → Subject → body; typing `an` in To suggests known people; Send with `half typed@` marks that address and does not send.
17. Send to an address the server has no mailbox for → the toast names that address and the server's reason; the draft stays open; `app-sola-mail.log` has a `send failed` warn line.
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
