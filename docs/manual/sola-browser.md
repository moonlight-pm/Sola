# Browser

Browser is Sola's web browser. It is one binary, `sola-browser`: Sola's
own chrome, built with iced (tabs, location bar, session, profiles, and a
Bitwarden vault when built with the `bitwarden` feature), around a
Chromium (CEF) engine that renders off-screen on the CPU.

## Launch

Open **Browser** from the shell launcher, or run
`/opt/sola/bin/sola-browser`.

The same window is also a call-plane owner: `solactl browser` lists tabs,
opens and moves them, and snapshots and clicks the page (YAML with refs).
See [solactl](solactl.md). If the browser chrome or `sola-call` is not
running, `solactl browser` fails; it does not launch a window.

## Default URL handler

Sola routes web opens to Browser. There is no Helium (or other browser)
fallback.

| Path | Behavior |
|------|----------|
| Link click in Terminal, Mail, or Arcade | `sola_core::open_url`: hands the URL to `chrome.sock` if the browser is running, else starts it |
| `solactl open <url>` | Same |
| Bus `Topic::OpenUrl` | A running browser opens a tab; the shell starts the browser only if it is not running |
| Terminal `open` / `xdg-open` | `xdg-open`; MIME via `sola-browser.desktop`; a second process hands off and exits |

`sola-browser.desktop` `Exec` is `/opt/sola/bin/sola-browser %u` (the
same `chrome.sock` handoff as `solactl open`). It claims
`x-scheme-handler/http`, `https`, `about`, and `unknown`, plus
`text/html`, `application/xhtml+xml`, and `application/pdf`, so GIO and
`xdg-open` (terminal `open`) do not pick Chrome or another handler for
those types. An HTML or PDF file path (absolute or relative to the
calling process) is opened as `file://`, not as `https://apocrypha/…`.

If a Browser window is already open, an outside open **raises it** to the
top (the same as a click) and focuses the new tab.

Only **one** browser chrome runs. A second `sola-browser` (or
`solactl open`) hands the URL to
`~/.local/share/sola/browser/chrome.sock` and exits.

Installing Sola copies the desktop file to `~/.local/share/applications/`
and re-registers MIME defaults from every `sola-*.desktop` `MimeType=`
line. Override the binary with `SOLA_BROWSER`. If the binary is missing,
the open fails.

## Profiles

A **profile** is a separate web identity and tab workspace.

| Piece | Location |
|-------|----------|
| Registry | `~/.local/share/sola/browser/profiles.json` |
| Tabs / session | `~/.local/share/sola/browser/profiles/<uuid>/session.json` |
| CEF cookies / storage | `~/.local/share/sola/browser/profiles/<uuid>/cef/` |
| Discardable cache | `~/.cache/sola/browser/profiles/<uuid>/` |
| Vault prefs (shared) | `~/.config/sola/browser/vault.json` |
| Vault session (shared) | `~/.local/share/sola/browser/vault-session.json` (age-encrypted tokens and keys; **Log out** deletes it) |
| Downloads index (shared) | `~/.local/share/sola/browser/shared/downloads.json` |
| Visit history (location bar) | `~/.local/share/sola/browser/shared/history.json` |
| Notification permission | `~/.local/share/sola/browser/profiles/<uuid>/notifications.json` |
| Microphone / camera | `~/.local/share/sola/browser/profiles/<uuid>/media.json` |
| HTTP Basic / Digest auth | `~/.local/share/sola/browser/profiles/<uuid>/http-auth.json` (age-encrypted; kit **Sign in** prompt) |

Site logins (cookies) live under that profile's CEF directory. The engine
uses Chromium's **basic** password store, so cookie encryption works
without a desktop keyring (Sola runs from a TTY). Updating the browser
binary asks each helper to **Quit** (which flushes cookies) before it is
replaced, so GitHub and Google sessions are not dropped by a hard kill.
After updating, sign in once more if an older session still does not
restore (cookies written under a failed keyring backend cannot be read
again). Closed tabs stay closed across a restart; `⌘⇧T` reopens them.

### Menubar → Profiles

1. **Profile list**: click a name to switch. The active profile shows a
   checkmark.
2. **New Profile…**: a name dialog; creates the directories and switches
   to the new profile in the same window.
3. **Rename Profile…**: renames the active profile.
4. **Delete Profile…**: confirms deletion of the **active** profile
   (blocked if it is the only one). Its data directories are removed, and
   the window loads the next profile's tabs.

### Switching profiles

Switching profiles is **instant**. The shell still shows one **Browser**
app. Each profile keeps its own CEF cookie store in a headless engine
process (no extra switcher entries). The location bar updates
immediately; the page area blanks until the new
profile's tab paints (it does not keep showing the previous identity). A
tab you have already opened this session appears instantly; one that has
not painted yet stays blank until it does. Switching back restores the
parked helper's live tabs; they do not reload. Closing the last page
window quits the browser and those helpers. Tabs restore from
`session.json`.

Switch, create, rename, and delete profiles under **Menubar → Profiles**.

**Cold start** with an empty session opens a single **blank** tab.

## Tabs

Each tab is a **page with its own window**: a row in the shell's tab bar
like any other window. The browser has no tab strip of its own and no tab
groups. The page window you are in is the active page and carries the
whole top bar (location bar, find, menus, DevTools, vault, downloads).
Another page window on screen shows its page live under a read-only
address bar; click into it to make it the active page. Pages stacked
behind the shown one in a zone rest until shown.

`⌘T` opens a blank page window in the focused zone. `⌘W` closes the page
and its window; closing the last page window quits the browser.

`⌘⇧T` (Browser → **Reopen Closed Tab**) reopens the most recently closed
page in a new window; press it again for the one before that. The stack is
per profile, up to 25, and is saved in `session.json`. The open pages are
saved there too, each with a stable id, so every page window keeps its
place in the tab bar across restarts.

Links from other apps (`xdg-open`, `solactl open`) open a new page window.
`⌘`-click opens a page window behind the current one. A page
`window.open` (database consoles, SSH terminals, and similar) or a
`target=_blank` link opens a page window in front, focused.

## Location bar

The location bar (omnibox) is **plain text** on the chrome until you click
it (or press `⌘L`); then it is an edit field. While idle, `https://` and
the query string are omitted (the path stays).

Type a URL or a search and press Enter. Search text goes to Kagi.
Scheme-less hosts get `https://`; **localhost and loopback** (`127.0.0.1`,
`[::1]`, `*.localhost`) get `http://`. An explicit scheme is left as
typed.

Typing searches **visit history**: the top five matches appear as a list
under the chrome. The field is not autocompleted and **keeps focus**. The
first row is not selected for you. ↓ / `Ctrl+N` and ↑ / `Ctrl+P`, or a
click, highlight a row. Further typing refines the list.

- **Enter** loads the highlighted row, or the URL you typed in this tab,
  or Kagi's first result for a search.
- **Shift+Enter** always loads the Kagi results page.

A new blank tab that names a site already open switches to that tab and
closes the blank. The exact URL of another open tab is focused instead of
loaded again.

- The field **unfocuses on submit**, so the caret is gone while the page
  loads. The text changes from what you typed to the resolved URL, then
  to the page's canonical URL; it does not flash empty in between.
- While a real page is loading, a **thin accent line** grows along the
  bottom of the field. Reload becomes **Stop**; back and forward follow
  the engine. Escape also stops the load. `⌘⇧R` still hard-reloads
  (ignores the cache even if a load is in flight).
- A **copy** button sits just left of the field. It puts the current page
  URL on the clipboard (not a draft you are still typing). Empty and
  `about:blank` tabs disable it. The glyph flashes a check for a moment
  after a successful copy.

## Find and reload

`⌘F` finds on the page; `⌘G` / `⌘⇧G` is next / previous.

`⌘R` reloads. `⌘⇧R` (Browser → **Hard Reload**) bypasses the HTTP cache
for this navigation. Shift-click on the reload button does the same. It
does not empty the profile disk cache.

## Media

Pages may autoplay media without a click. Typical MP4 (H.264 + AAC),
including Steam store trailers, plays on a CEF built with those codecs
(`scripts/cef-codecs/`). The public `install-cef` tarball does not
include those codecs.

## JavaScript dialogs

`alert()`, `confirm()`, and `prompt()` show as a kit dialog over the page
(OK / Cancel; `prompt` has a field). Leave-page confirms use Leave / Stay.
Escape cancels (OK on `alert`). A second dialog waits until the first is
answered.

## Developer Tools

Developer Tools docks Chromium's inspector **under the page** in the same
tab (not a new tab or a separate window). Drag the hairline to resize.
Choose the action again to close it. Switching tabs hides the panel until
you return to the inspected tab.

Right-click the page:

- **Use as Desktop Background** (on a picture): the browser fetches the
  largest version it can find — the link the picture sits in, Wikimedia's
  original behind a thumbnail, or the picture — and, when its shape is not
  the screen's, shows it with a frame in the screen's shape: drag the frame
  to choose what fills the screen, scroll for less or more, Return to use
  it, Esc to leave. The part is saved in `~/Pictures/Backgrounds/` and
  becomes the desktop's background.
- **Open Developer Tools**: console, docked.
- **Inspect Element**: elements, with the node under the pointer selected.

**Menubar → Browser → Developer Tools** (`⌘⌥I`) opens the console. There
is no F12 binding (media-key keyboards).

## Downloads

The toolbar **download** icon (right of the vault) is always there.

- A download **auto-saves** to `~/Downloads`. If `report.pdf` already
  exists, or a leftover `report.pdf.crdownload` is still there, the next
  file is `report (1).pdf`. There is no Save dialog. A failed row names
  the reason (no space, timed out, incomplete).
- While a file is coming in, the icon turns accent and a thin progress
  line grows on the button. The panel does not open by itself.
- Click the icon for the list (flat rows). Long hash names shorten in the
  middle. In-progress rows show percent and **Cancel**. Finished rows open
  the file with the default app (`xdg-open`). **×** removes the row from
  the list only; the file stays on disk.
- After a download finishes, the icon stays accent until you open the
  panel.
- Completed and failed items survive quit. In-progress ones do not (the
  helper dies with the window).

There is no "show in folder" (Sola has no file manager yet) and no delete
from disk.

## Notifications

A page that calls `Notification.requestPermission()` gets a chrome dialog
(**Allow** / **Block**). The choice is stored per profile in
`notifications.json`. After **Allow**, `new Notification(title, { body })`
shows a Sola desk card at the **screen** top-right under the menubar (the
same place as Projects' done cards), not a banner over the page. Click
the card to raise Browser and that tab. Missed cards collect behind the
menubar bell.

Sites you have not allowed cannot notify. There is no sound and there are
no action buttons yet.

## Microphone and camera

A page that calls `getUserMedia` (WebRTC, huddles, camera) gets the same
Allow / Block overlay as notifications. Grants live in
`profiles/<uuid>/media.json` and in Chromium's camera and microphone site
settings for that origin. Chromium has no permission bubble in this
off-screen path; without Allow, the request is denied. Screen share uses
the same dialog when the page asks for desktop capture.

## HTTP sign-in (Basic / Digest)

A site that asks for HTTP Basic or Digest auth gets a kit **Sign in**
dialog (username and password), not Chromium's native prompt. **Sign in**
saves the credentials for that host and realm in this profile
(`http-auth.json`, age-encrypted). The next visit, including after a
browser restart, uses them and does not ask again. If the server rejects
them, the dialog comes back with the username prefilled. **Cancel** aborts
the load.

## Bitwarden vault

The vault is present when Browser is built with the `bitwarden` feature.
The toolbar **vault** is one control (not separate login, authenticator,
and card buttons). Locked, it is a lock; unlocked, a key. It becomes a
shield when this page has a TOTP login, and a fingerprint during a
passkey ceremony. The open panel's icon is the accent wash.

- **Unlock** with your Bitwarden email and master password (and 2FA when
  required). The same panel then shows the vault (unless a passkey
  ceremony is already waiting). After a successful unlock the session
  stays live across browser restarts (no password again until you
  **Log out**). **Log out** on the unlocked card wipes tokens, keys, and
  the session file. The remembered email stays, so the next unlock is
  prefilled.
- **Browse** searches every item you can decrypt (personal plus every
  organization after unlock and sync): logins, cards, identities, notes,
  SSH keys, and the rest. Type chips filter. Autofill suggestions are
  URI-matching **logins** for the current page; **Fill** on that row
  injects the username and password and closes the panel. Click a row to
  open the **record**.
- **Record** shows the whole item: usernames, passwords (reveal and copy),
  websites, card number and CVC, identity fields, private notes, custom
  fields, SSH keys, and a live authenticator code with countdown. **Fill**
  on a login, card, or identity fills the page. Authenticator **Fill**
  copies the code and tries to fill an OTP field. Copy does not close the
  panel.
- **Create login** is the **+** on the unlocked card. Username is the last
  one you used, selected so typing replaces it. Password is a fresh
  16-character generated value (visible; **Regenerate** if you want
  another). URL is the page's apex domain (`google.com`, no `https://`).
  **Create** or Enter writes the item to your **personal** Bitwarden vault
  **first**, then fills every username and password field on the page
  (including confirm). If the page has no fields yet, the item is still
  saved.
- **Edit** is on the item record (next to **Fill**). Name, login, card,
  identity, note, SSH, bank, license, and passport fields, websites,
  notes, and custom fields are writable. **Save** writes the item; Cancel
  returns to the record. Passkeys stay attached (they are not edited
  here). Organization items you cannot write hide **Edit**. License and
  passport dates are `YYYY-MM-DD`. There is no delete in this panel.
- **Passkeys (get):** when a site calls WebAuthn
  `navigator.credentials.get`, the vault panel opens (unlock first if
  needed) with a **list of matching passkeys**; pick one to complete
  sign-in. The intercept is installed at **document-start** in **every
  frame** (Google sign-in, Gemini Exchange 2FA, and so on), so the page
  cannot keep Chromium's native `get`. Off-screen rendering cannot show
  Chromium's passkey sheet; the hook advertises a platform authenticator
  and opens Bitwarden instead. Duplicate or retried `get()` calls for the
  same site stay one picker; they do not fail the page before you pick.
- **Passkeys (create):** when a site calls `navigator.credentials.create`,
  the vault panel opens (unlock first if needed) with **Save a passkey**.
  Confirm creates a new Bitwarden login for the site (the name is the
  apex domain, the username comes from the request). Matching logins for
  the page are listed so you can attach the passkey to one of those
  instead. Chromium's own passkey window is not used.

### Unlock speed

Bitwarden's master-password KDF is expensive (about 600,000 PBKDF2
iterations). Bitwarden unlock needs a release build;
`cargo make install browser` builds release. A debug build (`--debug`)
compiles the crypto crates at opt-level 3, but is still slower than a
full release build.

### Vault files

Vault prefs (the remembered email) live at
`~/.config/sola/browser/vault.json` (shared across profiles). The
unlocked session is `~/.local/share/sola/browser/vault-session.json`
(age-encrypted with `~/.config/sola/key`; mode 0600). **Log out** deletes
it.

## Copy, paste, and click

`⌘C` / `⌘V` and triple-click selection work on form fields and body text.
`⌘C` / `⌘V` on the page go through the chrome (River takes those chords).
Copy extracts the selection in the engine helper and writes the system
clipboard; paste reads the compositor clipboard and inserts **once** into
the focused field. An image offer (`image/png` and siblings) is a `File`
paste event, so Slack and similar composers accept a screenshot. Text is
a `paste` event with `text/plain` (newlines are kept in contenteditable
fields); if the page does not handle it, a textarea gets the value and a
contenteditable gets a line break between lines. `solactl browser fill`
and `type` use that same paste into the snapshot ref (including a
ProseMirror editor nested under the accessibility node). `key --chord`
inserts once. Chromium's own clipboard never reaches Wayland.

In-page **Copy** buttons (`navigator.clipboard.writeText` and
`document.execCommand('copy')`) are hooked the same way. Newlines in the
copied text are kept. Triple-click selects a line or field the way
Chromium expects. The location bar and vault fields stay text-only.

`⌘`-click (or Ctrl-click) a link to open it in a **page window behind**
the current one.
A right-click on the page opens a small kit menu (on a picture, **Use as
Desktop Background**; open and copy link; cut, copy, and paste when
editing; back, forward, and reload) instead of Chromium's off-screen
popup. Click back or forward to go one step;
**hold** the button for a menu of that tab's session history. That list
is saved with the session, so it survives a browser restart.

Clicking the page focuses the engine (caret and IME). Shift+wheel scrolls
sideways. The number pad types digits in page fields and the location bar
(NumLock is on by default; turn it off for Home, End, and the arrows).
Composition (dead keys, CJK) is forwarded to Chromium when the page owns
keys; the candidate window sits on the last caret box.

`<select>` dropdowns paint as an overlay on the page (Chromium's
off-screen popup buffer).

## Not covered here

This chapter does not yet include a full keyboard reference for the
chrome. There is no Save As and no custom download folder.

See the capability row **browser** in
[`docs/capabilities.md`](../capabilities.md) and the profiles design
[`docs/specs/2026-08-10-sola-browser-profiles-design.md`](../specs/2026-08-10-sola-browser-profiles-design.md).
