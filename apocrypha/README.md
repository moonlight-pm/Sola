# Apocrypha

Reference-only material that is **not part of the active Sola workspace**.

Nothing under this tree is a `[workspace]` member. `cargo make build` /
`cargo make install` do not discover or build it. It is kept for
history and as a rewrite aid — not as a second stack to extend.

## Contents

| Path | What it was | Status |
|------|-------------|--------|
| `sola-app/` | GTK4 + WebKit6 WebView app framework | Frozen host. Superseded by `crates/sola-kit` (iced). |
| `apps/agent/` | WebView coding-agent prototype (`claude` CLI) | Retired. Kit ACP GUI (`crates/sola-agent`) also retired 2026-08-28; daily agent work is Workspaces. |
| `apps/mail/` | Full IMAP/SMTP/IDLE mail client (WebView UI) | **Reference for a future `crates/sola-mail`.** Settings already has iced mail *config* (`Topic::MailConfig`); the client UI/engine has not been ported. |

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

## Future mail rewrite

When starting `crates/sola-mail` on sola-kit:

1. Lift IMAP/SMTP/IDLE/rules/sender logic from `apps/mail/src/` (Rust).
2. Reimplement UI in iced against kit components — do not port the TS/WebView.
3. Wire config through existing `Topic::MailConfig` / `sola-settings` mail panel.
4. Delete or further archive this tree once parity is good enough.
