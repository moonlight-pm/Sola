# sola-browser

**Status:** partial dogfood — iced chrome + CEF; Profiles + Bitwarden unlock /
fill / passkey **get** dogfooded.

## What it is

Sola’s product web browser: **iced chrome** (tabs, omnibox, session, profiles,
Bitwarden vault when built with `bitwarden`) plus a **CEF** CPU OSR engine in
one binary, `sola-browser`.

Launch from the shell launcher (**Browser**), or
`/opt/sola/bin/sola-browser`.

## Profiles

A **profile** is a separate web identity + tab workspace (D8).

| Piece | Location |
|-------|----------|
| Registry | `~/.local/share/sola/browser/profiles.json` |
| Tabs / session | `~/.local/share/sola/browser/profiles/<uuid>/session.json` |
| CEF cookies / storage | `~/.local/share/sola/browser/profiles/<uuid>/cef/` |
| Discardable cache | `~/.cache/sola/browser/profiles/<uuid>/` |
| Vault prefs (shared) | `~/.config/sola/browser/vault.json` |

### Menubar → Profiles

1. **Profile list** — click a name to switch. The active profile shows a
   checkmark (requires shell that draws `MenuItem.checked`).
2. **New Profile…** — name dialog; creates dirs and switches to it in the
   same window.
3. **Rename Profile…** — renames the active profile.
4. **Delete Profile…** — confirms deletion of the **active** profile (blocked
   if it is the only one). Data dirs are removed; the window reloads the
   next profile’s tabs.

Switching or creating a profile **keeps the window open**. The previous
profile’s workspace is **parked** (CEF tabs stay warm in memory so form
state and SPA work are not reloaded) and also written to `session.json`.
Returning to a parked profile resumes those same tabs. Parks follow a
shared eviction policy (idle timeout, max parked profiles, total tab
budget) — same rules whether the workspace was left via Profiles or
would be under memory pressure later.

**Cold start** with an empty session opens a single **blank** tab.

## Bitwarden vault

Toolbar lock / key icon opens the vault panel.

- **Unlock** with Bitwarden email + master password (and 2FA when required).
  After unlock, the panel opens the **fill login** list for the active page
  (unless a passkey ceremony is already waiting).
- **Fill login** lists URI-matching items (tall list; items with a passkey show
  a **passkey** badge). Click to fill username / password into the page.
- **Passkeys (get):** when a site calls WebAuthn `navigator.credentials.get`,
  the vault panel opens (unlock first if needed) with a **list of matching
  passkeys** — pick one to complete sign-in. Dogfooded on Google accounts.
  **Registration** (`credentials.create`) is not supported yet.

### Unlock speed

Bitwarden’s master-password KDF is expensive (~600k PBKDF2). Prefer:

```bash
cargo make install browser --release
```

Debug installs also compile crypto crates at opt-level 3 (faster than plain
debug, still slower than full release).

Vault prefs (remembered email) live at `~/.config/sola/browser/vault.json`
(shared across profiles).

## Not in this manual yet

- Full keyboard chrome reference  
- Engine quirks (loading flags, OSR input/scroll)  
- Passkey **registration**  

See capability row **browser** in [`docs/capabilities.md`](../capabilities.md)
and freeze [`docs/specs/2026-08-10-sola-browser-profiles-design.md`](../specs/2026-08-10-sola-browser-profiles-design.md).
