# sola-mail kit Implementation Plan

> **For agentic workers:** Implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `crates/sola-mail` — kit-native iced mail client with feature parity to `apocrypha/apps/mail`.

**Architecture:** Single crate, three layers: `protocol/` (IMAP/SMTP/IDLE/rules/wicket/html→text), `worker/` (typed cmd/event thread), `ui/` (iced panes). Config from sticky `Topic::MailConfig`. No WebView.

**Tech Stack:** Rust, sola-kit/iced 0.14, sola-bus, sola-core, `imap` + rustls-connector, lettre, mail-parser, ureq, html2text.

**Spec:** `docs/specs/2026-07-27-sola-mail-kit-design.md`

## Global Constraints

- Work on branch `sola-mail` in this workspace (already feature branch).
- `cargo make build mail` (or `sola-mail`) to verify — **never** `cargo make install` without user permission.
- Lift protocol from `apocrypha/apps/mail/src/`; rebuild UI on kit.
- Config: bus `MailConfig` only — no `mail.toml` loader.
- APP_ID / binary: `sola-mail`.
- Password: `MailConfig.password.0` (Encrypted inner String) at worker boundary.
- Do not rewrite storybook pages unless asked.

## File Map

| Path | Responsibility |
|------|----------------|
| `Cargo.toml` (workspace) | Reintroduce mail workspace deps |
| `crates/sola-mail/Cargo.toml` | Crate manifest |
| `crates/sola-mail/src/main.rs` | startup, BusSetup, iced app entry |
| `crates/sola-mail/src/bridge.rs` | cmd_tx / event channel |
| `crates/sola-mail/src/protocol/mod.rs` | module exports |
| `crates/sola-mail/src/protocol/types.rs` | Folder, MessageSummary, MessageBody |
| `crates/sola-mail/src/protocol/rules.rs` | rule_matches + tests |
| `crates/sola-mail/src/protocol/imap.rs` | ImapClient (lift) |
| `crates/sola-mail/src/protocol/idle.rs` | IDLE watcher (lift) |
| `crates/sola-mail/src/protocol/sender.rs` | SMTP (lift) |
| `crates/sola-mail/src/protocol/wicket.rs` | from-address API (lift) |
| `crates/sola-mail/src/protocol/html_text.rs` | HTML → plain text |
| `crates/sola-mail/src/protocol/account.rs` | thin adapter over bus MailConfig (password plain) |
| `crates/sola-mail/src/worker/mod.rs` | thread + loop |
| `crates/sola-mail/src/worker/cmds.rs` | MailCmd / MailEvent |
| `crates/sola-mail/src/ui/mod.rs` | App state, update, view, subscription |
| `crates/sola-mail/src/ui/*.rs` | folder_list, message_list, message_view, compose, toast |

---

### Task 1: Scaffold crate + kit shell

**Files:** workspace `Cargo.toml`, `crates/sola-mail/**`

- [ ] Add workspace deps: `imap`, `rustls-connector`, `lettre`, `mail-parser`, `html2text` (ureq/base64/rustls already present).
- [ ] Create crate with `main.rs`: `startup` → `BusSetup` → empty three-pane placeholder UI, menu Quit, theme bus.
- [ ] `cargo make build mail` succeeds.

### Task 2: Protocol types + rules + html_text

- [ ] Port types and `rule_matches` tests (use bus `MailRule` / `MailRuleCondition`).
- [ ] `html_text::to_plain` with unit test fixtures.
- [ ] `Account` helper from `MailConfig` (plain password access).

### Task 3: Lift imap / idle / sender / wicket

- [ ] Mechanical port; switch config type to `Account` / bus fields.
- [ ] No tokio required: keepalive via std thread sleep in worker.

### Task 4: Worker + bridge

- [ ] `MailCmd` / `MailEvent` enums; worker thread handles connect, list, search, body, mark, move, empty, send, apply_rules, reconfigure, shutdown.
- [ ] IDLE → `MailEvent::NewMail`.
- [ ] Bridge channels + iced subscription (poll or stream::channel pattern like kit bus_subscription).

### Task 5: UI — connect + folders + list + body

- [ ] On `MailConfig` bus: reconfigure/connect.
- [ ] Folder sidebar, message list, open body, mark read, loading/error empty states.

### Task 6: UI — compose, move, undo, search, bulk, empty

- [ ] Compose mode swap; send; shortcuts j/i/a/d/u/w/s; search; bulk archive/trash; empty folder.

### Task 7: Smart mailboxes, IDLE refresh, OpenUrl, polish

- [ ] Smart folder listing; NewMail refresh; link open via `Topic::OpenUrl`; disconnected “configure in Settings”.
- [ ] Full smoke checklist from design.

---

## Verification

- `cargo make build mail`
- `cargo test -p sola-mail`
- Manual smoke per design § Testing (user installs when ready)
