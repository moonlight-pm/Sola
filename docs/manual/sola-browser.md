# Browser

Browser is Sola's web browser. It is one program, `sola-browser`, in
Oath's `pkg:sola-browser`: Sola's own chrome, built with iced, around
Chromium's engine, CEF 156, which renders each page off-screen on the CPU
in a helper process. The chrome has the location bar, find, downloads,
the page's dialogs, and a Bitwarden vault. Browser also hosts
[web apps](web-apps.md).

## Windows

Every page is a window of its own, a row in the shell's bar like any
other window. Browser has no tab strip and no tab groups.

The window you are in is the active page. Under its titlebar it carries
the whole top bar: back, forward, reload, copy address, the location bar,
the vault, and downloads. Its find bar, panels, and the page's dialogs
open there too. Every other page window on screen shows its page live
under a read-only bar with its address; click into one and it becomes the
active page. A page whose window is not on screen rests (see
[Media](#media)).

The titlebar holds Browser's menus (Browser, Edit, and Window) and, at the
right, the page's title, or its address while it has none.

- ⌘T opens a blank page window right after the focused window in the bar,
  with the location bar ready for typing.
- ⌘W closes the page and its window, and the zone shows the window it
  showed before (the page you opened it from, if you did not switch
  away). The location bar there starts unfocused. Closing a page window
  from the bar does the same. Closing the last page window quits Browser.
- ⌘⇧T (Browser → **Reopen Closed Tab**) reopens the most recently closed
  page in a new window, with its back and forward history; press it again
  for the one before. Browser keeps the last 25 closed pages with its
  session and reopens each under the showing workspace's profile.
- ⌘-click (or Ctrl-click) a link to open it in a page window behind the
  current one, right after it in the bar. Several in a row keep the order
  you opened them. **Open Link in New Tab** in the page menu also opens
  the link behind.
- A `target=_blank` link, or a page's `window.open` (database consoles,
  SSH terminals, a call's popup), opens a page window in front, focused,
  right after the page that opened it. Browser never opens a window of
  Chromium's own.
- A middle-click on a link does nothing.

The open pages are saved in each profile's `session.json`, each with a
stable id, so every page window comes back in its place in the bar after
a restart. Pages you closed stay closed. With nothing saved, Browser
starts with one blank page.

## Launch

Pick **Browser** in the launcher and type an address or a search (see
[the shell](sola-shell.md#launcher)): it opens in a new page window, as
Enter in the location bar would. Starting `sola-browser` again while
Browser runs opens a blank page window, as ⌘T does.

Only one Browser runs. A second `sola-browser` hands its address to the
running one over `~/.local/share/sola/browser/chrome.sock` and exits;
with no address, it asks for a blank page window.

Browser is also a call-plane owner, `browser`: `solactl browser` lists
pages, opens, focuses, and closes them, navigates, and snapshots, reads,
and clicks a page (YAML with refs). See
[solactl](solactl.md#browser-solactl-browser). If Browser or `sola-call`
is not running, `solactl browser` fails; it does not start Browser.

## Links from other applications

Every Sola application opens web links in Browser. There is no fallback
to another browser.

| From | What happens |
|------|--------------|
| A link clicked in Terminal, Projects, Mail, Calendar, or Bots | `sola_core::open_url`: the address goes to the running Browser over `chrome.sock`, or starts `/bin/sola-browser` with it |
| `solactl open <url>` | The same; an image path goes to Paint and an `.ics` file to Calendar instead |
| The launcher's Browser field | The same |
| Bus `OpenUrl` | A running Browser opens the page; the shell starts Browser only when it is not running |

A link from outside opens in a new page window in front, focused, right
after the focused window in the bar. A local HTML or PDF path (absolute,
or relative to the program that opens it) is opened as a `file://` URL.
`SOLA_BROWSER` names another binary to start. If the binary is missing,
the open fails.

Sola's Oath packages carry no `.desktop` files, so `xdg-open` is not set
up to hand web pages to Browser.

## Profiles

A **profile** is a separate web identity: cookies, site storage, saved
pages. Every desktop workspace has one of its own; there is nothing to
pick and no Profiles menu. A profile has no name (it is a uuid on disk),
so renaming a workspace changes nothing about its profile.

- Every page on a workspace runs its profile. Send a page to another
  workspace and it reopens there, in the same window, at the same address,
  under that workspace's profile; its back history and form state do not
  come along.
- A new workspace starts with a new, empty profile.
- A deleted workspace's profile stays on disk with its sign-ins and saved
  pages, but nothing opens it.
- A web app keeps the profile it was wrapped from, wherever its window
  goes.

Each profile runs in its own headless engine process, all at once: pages
on every workspace stay live, and showing a workspace shows its pages
without reloading.

| Piece | Location |
|-------|----------|
| Registry (profiles, and which workspace has which) | `~/.local/share/sola/browser/profiles.json` |
| Pages and closed pages | `~/.local/share/sola/browser/profiles/<uuid>/session.json` |
| Cookies and site storage | `~/.local/share/sola/browser/profiles/<uuid>/cef/` |
| Discardable cache | `~/.cache/sola/browser/profiles/<uuid>/` |
| Notification permission | `~/.local/share/sola/browser/profiles/<uuid>/notifications.json` |
| Microphone and camera | `~/.local/share/sola/browser/profiles/<uuid>/media.json` |
| HTTP Basic / Digest sign-ins | `~/.local/share/sola/browser/profiles/<uuid>/http-auth.json` (age-encrypted) |
| Downloads list (shared) | `~/.local/share/sola/browser/shared/downloads.json` |
| Visit history (shared) | `~/.local/share/sola/browser/shared/history.json` |
| Vault email (shared) | `~/.config/sola/browser/vault.json` |
| Vault session (shared) | `~/.local/share/sola/browser/vault-session.json` (age-encrypted) |

The engine uses Chromium's **basic** password store, so cookie encryption
works without a desktop keyring. When Browser starts, it asks engine
helpers left from its last run to quit cleanly, which writes their
cookies, before it ends any that remain, so a restart does not sign you
out of sites.

## Location bar

The location bar is **plain text** until you click it (or press ⌘L, which
also selects its text); then it is an edit field. While idle it leaves
out `https://`, the query string, and the fragment; the path stays.

Type an address or a search and press Enter:

- Text with a space in it is a search, and so is a single word with no
  dot in it (`localhost` excepted). Searches go to Kagi.
- A host gets `https://`; **localhost and loopback** (`127.0.0.1`,
  `[::1]`, `*.localhost`) get `http://`. An explicit scheme is left as
  typed. A local file path becomes a `file://` URL.

Typing searches the **visit history**: the top five matches appear as a
list under the bar. The field is not autocompleted and **keeps focus**,
and no row is selected for you. ↓ or Ctrl+N and ↑ or Ctrl+P, or a click,
highlight a row; further typing refines the list.

- **Enter** loads the highlighted row, or the address you typed, or
  Kagi's first result for a search.
- **Shift+Enter** loads Kagi's results page.

Typed text always loads in the current page, even when another window
already shows that page or site. Only a highlighted row jumps: if its page
is open in another window, Enter brings that window forward instead of
loading it twice (and closes the page you typed in, if it was a blank
one).

- The field **unfocuses on submit**. Its text changes from what you typed
  to the resolved address, then to the page's own; it does not flash
  empty in between.
- While a page loads, a **thin accent line** grows along the bottom of the
  field and Reload becomes **Stop**. Escape stops the load too.
- The **copy** button just left of the field puts the page's address on
  the clipboard (not text you are still typing) and flashes a check. It is
  off on a blank page.

Visits are kept browser-wide, the newest 2,000, without Kagi's search
pages. The launcher's Browser field reads the same history.

## Find and reload

⌘F opens the find bar over the page (**Find in page**, with a count such
as `3 of 12`); Enter or ⌘G is the next match, ⌘⇧G the previous. The
location bar still works while the find bar is open: Enter goes to
whichever field has focus.

⌘R reloads. ⌘⇧R (Browser → **Hard Reload**), or Shift-click on Reload,
bypasses the HTTP cache for this load. It does not empty the profile's
disk cache.

Click back or forward to go one step; **hold** the button for a menu of
that page's history, up to 12 steps. That history is saved with the
session, so it survives a restart.

## Media

Pages autoplay as they do in Chrome: a muted video plays while it is in
view and pauses when you scroll away; sound waits for your first click on
the page. A page whose window is not on screen (on another workspace,
behind another window in its zone, or under the desk's idle fade once the
fade is complete) rests, and its videos pause until it shows again.

Oath's `pkg:cef` is built with H.264 and AAC, so typical MP4 video,
Steam store trailers included, plays.

## Page menu

Right-click the page for Sola's menu (Chromium's own does not show):

- on a picture, **Use as Desktop Background**;
- on a link, **Open Link in New Tab** (a page window behind) and **Copy
  Link Address**;
- in a field, **Cut** and **Copy** with a selection, and **Paste**; with
  a selection elsewhere, **Copy**;
- **Back**, **Forward**, **Reload**;
- **Open Developer Tools** and **Inspect Element**;
- on an `http` or `https` page, **Wrap This Page…** (see
  [Web apps](web-apps.md)).

### Use as Desktop Background

Browser fetches the largest version of the picture it can find (the link
the picture sits in, Wikimedia's original behind a thumbnail, or the
picture itself; up to 150 MB). When its shape is the screen's, it is used
as it is. Otherwise a lightbox shows it with a frame in the screen's
shape: drag the frame to choose what fills the screen, scroll to take less
or more of the picture, Return to use it, Escape to leave.

The picture, or the part you chose as a JPEG, is saved in
`~/Pictures/Backgrounds/` (`$XDG_PICTURES_DIR/Backgrounds` when that is
set) and becomes the desktop's background.

## Developer Tools

Developer Tools docks Chromium's inspector **under the page** in the
page's window, not in a window of its own. Drag the hairline to resize it.
Choose the action again to close it. One page has it at a time; it shows
while that page is the active page.

- Browser → **Developer Tools** (⌘⌥I), or **Open Developer Tools** in the
  page menu: the console.
- **Inspect Element**: the elements panel, with the node under the
  pointer selected.

There is no F12 binding.

## JavaScript dialogs

`alert()`, `confirm()`, and `prompt()` show as a dialog over the page
(OK / Cancel; `prompt` has a field). Leave-page confirms use Leave /
Stay. Escape cancels (OK on `alert`). A second dialog waits until the
first is answered.

## Uploads

A page that asks for a file (**Choose File**, a drop zone's picker) opens
Sola's file panel over the page, the same one every Sola app opens and
saves with: Places, the folder trail, the files (only the kinds the page
accepts, when it says), **Upload**. Pages that take several files let you
choose several (Shift, ⌘); a page that wants a folder gets **Choose a
folder**; a page that saves a file gets the panel's Save. Keys: arrows,
Return, Escape, ⌘↑ (up a folder), typing a name to jump to it, ⌘⇧. for
hidden files. The next upload starts in the folder the last one came from
(Downloads, the first time).

## Downloads

The **download** button (right of the vault) is always in the top bar.

- A download **saves on its own** to `~/Downloads`. If `report.pdf`
  already exists, or a leftover `report.pdf.crdownload` is still there,
  the file is `report (1).pdf`. There is no Save dialog.
- While a file is coming in, the button turns accent and a thin progress
  line grows on it. The panel does not open by itself.
- Click the button for the list. Long names shorten in the middle. Rows
  in progress show the percentage and **Cancel**. Finished rows open the
  file in its default application (`xdg-open`). A failed row says why (no
  space, timed out, incomplete, network error, and so on). **×** removes
  the row from the list only; the file stays.
- After a download finishes, the button stays accent until you open the
  panel.
- Finished and failed downloads stay in the list across a restart;
  downloads in progress end with Browser.

## Notifications

A page that calls `Notification.requestPermission()` gets a dialog
(**Allow** / **Block**). The choice is stored per profile in
`notifications.json`. After **Allow**, `new Notification(title, { body })`
shows a Sola desk card at the top right of the screen, under the top row,
not a banner over the page. Click the card to bring that page's window
forward. Missed cards collect behind the bell in the top row.

Sites you have not allowed cannot notify. Notifications have no sound and
no action buttons.

## Microphone and camera

A page that calls `getUserMedia` (calls, huddles, a camera) gets the same
Allow / Block dialog, which names what the site wants. Grants are stored
per site in the profile's `media.json` and in Chromium's camera and
microphone settings for that site. Without **Allow**, the request is
denied. A page that asks to share the screen gets the same dialog.

## HTTP sign-in (Basic / Digest)

A site that asks for HTTP Basic or Digest authentication gets a **Sign
in** dialog (username and password), not Chromium's prompt. **Sign in**
saves the credentials for that host and realm in this profile
(`http-auth.json`, age-encrypted). The next visit, after a restart too,
uses them without asking. If the server rejects them, the dialog comes
back with the username filled in. **Cancel** stops the load.

## Bitwarden vault

The vault is built in (the `bitwarden` feature, on by default; Oath's
package has it). The **vault** button in the top bar is one control.
Locked, it is a lock; unlocked, a key. It becomes a shield when this page
has a login with an authenticator code, and a fingerprint during a
passkey request. While its panel is open, the button has the accent wash.

- **Unlock** with your Bitwarden email and master password (and the
  two-step code when the account asks). The same panel then shows the
  vault. The vault stays unlocked across restarts until you **Log out**.
  **Log out** on the unlocked card wipes the tokens, keys, and session
  file; the email stays, so the next unlock is filled in.
- **Browse** searches every item you can decrypt (personal plus every
  organization after unlock and sync): logins, cards, identities, notes,
  SSH keys, and the rest. Type chips filter. Logins whose website matches
  the current page are suggested first; **Fill** on one fills the
  username and password and closes the panel. Click a row to open the
  **record**.
- **Record** shows the whole item: usernames, passwords (reveal and copy),
  websites, card number and CVC, identity fields, notes, custom fields,
  SSH keys, and a live authenticator code with its countdown. **Fill** on
  a login, card, or identity fills the page. The authenticator's **Fill**
  copies the code and tries to fill a code field. Copying does not close
  the panel.
- **Create login** is the **+** on the unlocked card. The username is the
  last one you used, selected so typing replaces it. The password is a
  fresh 16-character generated one (shown; **Regenerate** for another).
  The URL is the page's domain (`google.com`, no `https://`). **Create**
  or Enter writes the item to your **personal** vault **first**, then
  fills every username and password field on the page (the confirm field
  too). If the page has no fields yet, the item is still saved.
- **Edit** is on the record, next to **Fill**. Name, login, card,
  identity, note, SSH, bank, license, and passport fields, websites,
  notes, and custom fields can be changed. **Save** writes the item;
  **Cancel** returns to the record. Passkeys stay attached; they are not
  edited here. Organization items you cannot write have no **Edit**.
  License and passport dates are `YYYY-MM-DD`. Items cannot be deleted
  here.
- **Passkeys, signing in:** when a site calls WebAuthn
  `navigator.credentials.get`, the vault panel opens (unlock first if
  needed) with the **passkeys saved for that site**; pick one to finish
  signing in. Browser installs this in every frame before the page's own
  scripts run (Google sign-in, for one), so the page cannot reach
  Chromium's own passkey sheet, which cannot show off-screen. Repeated
  requests from the same site stay one picker.
- **Passkeys, saving:** when a site calls `navigator.credentials.create`,
  the vault panel opens (unlock first if needed) with **Save a passkey**.
  Saving makes a new Bitwarden login for the site (named for its domain,
  with the username the site asked for), or attaches the passkey to one
  of the logins listed for the page.

Bitwarden's master-password key derivation is slow on purpose (600,000
PBKDF2 iterations unless the account says otherwise), so unlocking takes
a moment in a release build, Oath's package included, and much longer in
a debug build.

The unlocked session is `~/.local/share/sola/browser/vault-session.json`,
age-encrypted with `~/.config/sola/key`, mode 0600. **Log out** deletes
it.

## Copy, paste, and click

The shell takes the ⌘ chords of the focused application's menus, so ⌘C
and ⌘V reach Browser as Edit → Copy and Paste, and Browser hands them to
the page or the field that has focus. Copy takes the page's selection in
the engine and writes the system clipboard; Paste reads the clipboard and
inserts **once** into the focused field. A copied picture (`image/png`
and the like) is pasted as a file, so chat composers that take a
screenshot accept it. Text keeps its line breaks. Chromium's own
clipboard never reaches the desktop.

A page's own **Copy** buttons (`navigator.clipboard.writeText` and
`document.execCommand('copy')`) write the system clipboard the same way,
line breaks kept. Triple-click selects a line or a field. The location
bar and the vault's fields stay text-only.

Clicking the page gives it the keyboard (caret and input method).
Shift+wheel scrolls sideways. The number pad types digits in pages and in
the location bar (NumLock is on at session start; turn it off for Home,
End, and the arrows). Composed input (dead keys, CJK) reaches the page,
with the candidate window at the caret. `<select>` dropdowns open over
the page.

## Light or dark

Pages are told whether you prefer light or dark (`prefers-color-scheme`):
**Settings → Appearance → Light or dark**, Dark by default. A site with
both looks shows the one you chose; one with a single look is unchanged.
Pages that load after a change follow it at once; one already open
follows it when it is next on screen. Web apps follow it too. Design's
boards do not. The browser's own chrome stays dark either way.

## Keys

| Key | Action |
|-----|--------|
| ⌘T | Browser → **New Tab**: a blank page window |
| ⌘W | **Close Tab**: the page and its window |
| ⌘⇧T | **Reopen Closed Tab** |
| ⌘R / ⌘⇧R | **Reload** / **Hard Reload** |
| ⌘L | **Focus URL**: the location bar, its text selected |
| ⌘← / ⌘→ | **Back** / **Forward** |
| ⌘⌥I | **Developer Tools** (the console) |
| ⌘Q | **Quit Browser** |
| ⌘X / ⌘C / ⌘V / ⌘A | Edit → **Cut** / **Copy** / **Paste** / **Select All** |
| ⌘F / ⌘G / ⌘⇧G | Edit → **Find** / **Find Next** / **Find Previous** |
| Escape | Stop the load |

Browser → **Wrap This Page…** has no key.

## Limits

- No Save As, no choice of download folder, no "show in folder", and no
  deleting a downloaded file from the list.
- The page menu has no Save Image or Copy Image.
- Back-and-forward history is per page; there is no history window
  beyond the location bar's matches.
- Pages' icons show nowhere (a web app takes its icon from one).
