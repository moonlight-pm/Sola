# sola-browser

**Status:** partial dogfood — iced chrome + CEF; Profiles menubar shipped in
code; engine polish still open.

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
2. **New Profile…** — name dialog; creates dirs, makes the profile active,
   restarts the browser process.
3. **Rename Profile…** — renames the active profile (no restart).
4. **Delete Profile…** — confirms deletion of the **active** profile (blocked
   if it is the only one). Data dirs are removed; browser restarts on another
   profile.

Switching or creating a profile **re-execs** the process so CEF and tabs load
under the new id. Open tabs for the previous profile stay in that profile’s
`session.json`.

**Note:** Moving CEF user data under the profile path means the first run after
that cutover may look like a fresh login for sites (no migration of the old
global CEF runtime tree).

## Bitwarden vault

Toolbar lock icon opens the vault panel.

- **Unlock** with Bitwarden email + master password (and 2FA when required).
- **Fill login** lists URI-matching items for the active page (tall list;
  items with a passkey show a **passkey** badge). Click to fill username /
  password into the page.
- **Passkeys:** with the vault unlocked, WebAuthn `navigator.credentials.get`
  is intercepted and signed from Bitwarden FIDO2 credentials (auto-picks the
  first matching passkey). **Registration** (`credentials.create`) is not
  supported yet.

Vault prefs (remembered email) live at `~/.config/sola/browser/vault.json`
(shared across profiles).

## Not in this manual yet

- Full keyboard chrome reference  
- Engine quirks (loading flags, OSR input/scroll)  
- Passkey registration / multi-credential picker UI  

See capability row **browser** in [`docs/capabilities.md`](../capabilities.md)
and freeze [`docs/specs/2026-08-10-sola-browser-profiles-design.md`](../specs/2026-08-10-sola-browser-profiles-design.md).
