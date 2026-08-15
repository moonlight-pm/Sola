# sola-browser

**Status:** partial dogfood — iced chrome + CEF; Profiles + Bitwarden unlock /
fill / **Create login** / passkey **get**. Page ⌘C / ⌘V and triple-click
select work on form fields and body text.

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

Site logins (cookies) live under that profile CEF dir. The engine uses
Chromium’s **basic** password store so cookie encryption works without a
desktop keyring (Sola runs from a TTY). After updating the browser binary,
sign in once more if an older session does not restore — cookies written
under a failed keyring backend cannot be re-read.

### Menubar → Profiles

1. **Profile list** — click a name to switch. The active profile shows a
   checkmark (requires shell that draws `MenuItem.checked`).
2. **New Profile…** — name dialog; creates dirs and switches to it in the
   same window.
3. **Rename Profile…** — renames the active profile.
4. **Delete Profile…** — confirms deletion of the **active** profile (blocked
   if it is the only one). Data dirs are removed; the window reloads the
   next profile’s tabs.

Switching profiles is **instant** in the **same window**. The shell
still shows one **Browser** app. Each profile keeps its own CEF cookie
store in a headless engine process (no extra switcher entries). The
tab strip and location bar update immediately; the page area blanks
until the new profile’s tab paints (it does not keep showing the
previous identity). A tab you have already opened this session
appears instantly; one that has not painted yet goes blank until it
does. Switching back restores the parked helper’s live tabs — they
do not reload. Closing the window quits the browser and
those helpers. Tabs restore from `session.json`.

The **profile name** lives in the left of the full-width top bar
(aligned with the tab column). Each profile has a small enamel mark;
the menu hangs under the name as a darker card. Click to switch.
Manage (new / rename / delete) stays under **Menubar → Profiles**.

**Cold start** with an empty session opens a single **blank** tab.

## Tabs

The left strip is the tab list (`⌘T` for a new blank). Close removes the
row immediately — no flash back. Closing the tab you are looking at
selects the neighbor to the right (or the left if it was last). The last
tab is replaced by a blank rather than closing the window.

## Omnibox

Type a URL or a search and press Enter. Search text goes to Kagi.

- The field **unfocuses on submit** so the caret is gone while the page
  loads. The text swaps from what you typed to the resolved URL, then
  to the page’s canonical URL — it does not flash empty in between.
- While a real page is loading, a **thin accent line** grows along the
  bottom of the field. Reload becomes **Stop**; back / forward follow
  the engine. Escape also stops the load.

## Bitwarden vault

Toolbar lock / key icon opens the vault panel.

- **Unlock** with Bitwarden email + master password (and 2FA when required).
  After unlock, the panel opens the **fill login** list for the active page
  (unless a passkey ceremony is already waiting).
- **Fill login** lists URI-matching items (tall list; items with a passkey show
  a **passkey** badge). Click to fill username / password into the page.
- **Create login** is always on the unlocked card (primary when this site has
  no matches). Username is the last one you used, selected so typing replaces
  it. Password is a fresh 16-character generated value (visible; **Regenerate**
  if you want another). URL is the page’s apex domain (`google.com`, no
  `https://`). **Create** or Enter writes the item to Bitwarden **first**, then
  fills every username and password field on the page (including confirm).
  If the page has no fields yet, the item is still saved.
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

## Copy, paste, and click

⌘C / ⌘V on the page go through chrome (River steals the chords). Copy
extracts the selection in the engine helper and writes the system
clipboard; paste inserts into the focused field without emptying the
clipboard. Triple-click selects a line / field the way Chromium expects.

Clicking the page focuses the engine (caret / IME). Shift+wheel scrolls
sideways. Composition (dead keys, CJK) is forwarded to Chromium when the
page owns keys; the candidate window sits on the last caret box.

`<select>` dropdowns paint as an overlay on the page (Chromium’s OSR
popup buffer).

## Not in this manual yet

- Full keyboard chrome reference  
- Passkey **registration** (deferred)  

See capability row **browser** in [`docs/capabilities.md`](../capabilities.md)
and freeze [`docs/specs/2026-08-10-sola-browser-profiles-design.md`](../specs/2026-08-10-sola-browser-profiles-design.md).
