# sola-browser

**Status:** partial dogfood — iced chrome + CEF; Profiles + Bitwarden unlock /
fill / **Create login** / passkey **get** (Google and Gemini Exchange 2FA)
and **create**.
Downloads auto-save to `~/Downloads`. Page ⌘C / ⌘V and triple-click select
work on form fields and body text.

## What it is

Sola’s product web browser: **iced chrome** (tabs, omnibox, session, profiles,
Bitwarden vault when built with `bitwarden`) plus a **CEF** CPU OSR engine in
one binary, `sola-browser`.

Launch from the shell launcher (**Browser**), or
`/opt/sola/bin/sola-browser`.

## Default URL handler

Sola routes http(s) opens to **sola-browser**:

| Path | Behavior |
|------|----------|
| Terminal / mail / arcade link click | `sola_core::open_url` → `chrome.sock` if chrome is up, else spawn |
| `solactl open <url>` | same |
| Bus `Topic::OpenUrl` | live chrome opens a tab; shell only spawns if chrome is down |
| `xdg-open` / MIME defaults | `sola-browser.desktop`; a second process hands off and exits |

Only **one** iced chrome runs. A second `sola-browser` (or `solactl open`)
hands the URL to `~/.local/share/sola/browser/chrome.sock` and exits.

Install re-registers MIME defaults from `~/.local/share/applications/sola-*.desktop`.
Override the binary with `SOLA_BROWSER`. There is **no** alternate browser
fallback — if the binary is missing, open fails.

## Profiles

A **profile** is a separate web identity + tab workspace (D8).

| Piece | Location |
|-------|----------|
| Registry | `~/.local/share/sola/browser/profiles.json` |
| Tabs / session | `~/.local/share/sola/browser/profiles/<uuid>/session.json` |
| CEF cookies / storage | `~/.local/share/sola/browser/profiles/<uuid>/cef/` |
| Discardable cache | `~/.cache/sola/browser/profiles/<uuid>/` |
| Vault prefs (shared) | `~/.config/sola/browser/vault.json` |
| Downloads index (shared) | `~/.local/share/sola/browser/shared/downloads.json` |

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

The left strip is the tab list (`⌘T` for a new blank). **Drag a row** to
reorder; a click (no drag) still selects. Titles fill the column and
ellipsize at the edge (they grow if you widen the strip). Close removes
the row immediately — no flash back. Closing the tab you are looking at
selects the neighbor to the right (or the left if it was last). The last
tab is replaced by a blank rather than closing the window. Order is
saved in that profile’s `session.json`.

**Groups** are named folders at the **top** of the strip. Loose tabs stay
in one run underneath. Right-click a tab for **New group**, **Add to…**,
or **Ungroup**. Right-click a group header to **Rename** or **Ungroup**
(members go loose). Click the header to collapse — the page stays if you
were on a tab inside. Drag a loose tab into a group to join it; drag a
member into the loose run to leave. `⌘T` always makes a loose tab.
Empty groups disappear. There are no colors, nested groups, or spaces
yet.

## Omnibox

Type a URL or a search and press Enter. Search text goes to Kagi.

- The field **unfocuses on submit** so the caret is gone while the page
  loads. The text swaps from what you typed to the resolved URL, then
  to the page’s canonical URL — it does not flash empty in between.
- While a real page is loading, a **thin accent line** grows along the
  bottom of the field. Reload becomes **Stop**; back / forward follow
  the engine. Escape also stops the load.

## Downloads

Toolbar **download** icon (right of vault / cards) is always there.

- A download **auto-saves** to `~/Downloads`. If `report.pdf` already exists
  the next file is `report (1).pdf`. There is no Save dialog.
- While a file is coming in, the icon goes accent and a thin progress line
  grows on the button. The panel does not open by itself.
- Click the icon for the list (flat rows). Long hash names shorten in the
  middle. In-progress rows show percent and **Cancel**. Finished rows
  open the file with the default app (`xdg-open`). **×** removes the
  row from the list only — the file stays on disk.
- After a download finishes, the icon stays accent until you open the panel.
- Completed and failed items survive quit. In-progress ones do not (the
  helper dies with the window).

No “show in folder” (Sola has no file manager yet). No delete-from-disk.

## Bitwarden vault

Toolbar **key** opens logins. Toolbar **card** opens cards. They are
separate panels (only one at a time). Unlock is shared. While locked
both icons sit muted (key is a lock). After unlock both come up to
full chrome color; the open panel’s icon is the accent wash.

- **Unlock** with Bitwarden email + master password (and 2FA when required).
  The key button then opens the **fill login** list for the active page
  (unless a passkey ceremony is already waiting). The card button unlocks
  the same way, then opens **fill card**.
- **Fill login** lists URI-matching items (tall list; items with a passkey show
  a **passkey** badge). Click to fill username / password into the page.
- **Fill card** lists every card in the vault (cards rarely have URIs). Each
  row shows the item name, brand, last digits, and expiry. Click fills
  number, name, expiry, and CVC on the page (standard `cc-*` autocomplete
  plus common checkout names). The panel does not show the full number.
- **Create login** is always on the unlocked card (primary when this site has
  no matches). Username is the last one you used, selected so typing replaces
  it. Password is a fresh 16-character generated value (visible; **Regenerate**
  if you want another). URL is the page’s apex domain (`google.com`, no
  `https://`). **Create** or Enter writes the item to Bitwarden **first**, then
  fills every username and password field on the page (including confirm).
  If the page has no fields yet, the item is still saved.
- **Passkeys (get):** when a site calls WebAuthn `navigator.credentials.get`,
  the vault panel opens (unlock first if needed) with a **list of matching
  passkeys** — pick one to complete sign-in. The intercept is injected in
  **every frame** (Google sign-in iframes, Gemini Exchange 2FA, etc.).
  Duplicate or retry `get()` calls for the same site stay one picker —
  they do not fail the page before you pick. Chromium’s own passkey
  window is not used.
- **Passkeys (create):** when a site calls `navigator.credentials.create`,
  the vault panel opens (unlock first if needed) with **Save a passkey**.
  Confirm creates a new Bitwarden login for the site (name is the apex
  domain, username from the request). Matching logins for the page are
  listed so you can attach the passkey to one of those instead. Chromium’s
  own passkey window is not used.

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
- Save-as / custom download folder  

See capability row **browser** in [`docs/capabilities.md`](../capabilities.md)
and freeze [`docs/specs/2026-08-10-sola-browser-profiles-design.md`](../specs/2026-08-10-sola-browser-profiles-design.md).
