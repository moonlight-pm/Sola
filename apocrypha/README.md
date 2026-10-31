# Apocrypha

Reference-only material that is **not part of the active Sola workspace**.

Nothing under this tree is a `[workspace]` member. `cargo make build` /
`cargo make install` do not discover or build it. It is kept for
history and as a rewrite aid — not as a second stack to extend.

## Contents

| Path | What it was | Status |
|------|-------------|--------|
| `sola-app/` | GTK4 + WebKit6 WebView app framework | Frozen host. Superseded by `crates/sola-kit` (iced). |
| `apps/agent/` | WebView coding-agent prototype (`claude` CLI) | Retired. Kit ACP GUI (`crates/sola-agent`) also retired 2026-08-28; daily agent work is Projects. |
| `apps/mail/` | Full IMAP/SMTP/IDLE mail client (WebView UI) | **Ported** to `crates/sola-mail` (kit UI; engine lifted from here; config over `Topic::MailConfig` from Settings). Kept as history only. |

## Do not

- Depend on these packages from anything under `crates/`.
- Add new features here.
- Reintroduce them to the workspace without an explicit rewrite plan.

## Building (optional, unsupported)

Path deps point at live `crates/` (bus, core, assets) so a curious
reader *can* try:

```bash
cargo build --manifest-path apocrypha/sola-app/Cargo.toml
cargo build --manifest-path apocrypha/apps/mail/Cargo.toml
```

These are not CI-gated and may bitrot. Prefer reading the sources over
building.

Nothing here is left to port: every app is ported or retired.
