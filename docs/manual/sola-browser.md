# Browser

Browser is Sola's web browser. It is one program, `sola-browser`, in
Oath's `pkg:sola-browser`: Sola's own chrome, built with iced, around
Chromium's engine, CEF 156, which renders each page off-screen on the CPU
in a helper process. The chrome has the location bar, find, downloads,
the page's dialogs, and a panel onto your Bitwarden vault, which
[Vault](sola-vault.md)'s service holds. Browser also hosts
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
  you opened them.
- **Open Link in New Window** in the right-click menu opens the link in a
  new page window behind the current one.
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
| A link clicked in Terminal, Projects, Mail, Calendar, or Bots; `xdg-open` from any program | Sola's opener ([Opening links and files](session.md#opening-links-and-files)): a web link goes to Browser's `open` call, and with Browser not running the session starts it first, so it is tracked and comes back when the desk restarts |
| `solactl open <url>` | The same; an image path goes to Paint and an `.ics` file to Calendar instead |
| The launcher's Browser field | The same |
| Bus `OpenUrl` | A running Browser opens the page; the shell starts Browser only when it is not running |

A link from outside opens in a new page window in front, focused, right
after the focused window in the bar. A local HTML or PDF path (absolute,
or relative to the program that opens it) is opened as a `file://` URL.
`SOLA_BROWSER` names another binary to start. If the binary is missing,
the open fails.

On Oath `/bin/xdg-open` is Sola's opener, so a program that opens a web
page with `xdg-open` reaches Browser. A link in a page to a scheme Browser
does not load (`mailto:`, `claude-cli:`) goes to the app Sola keeps for it
when you clicked it, and nowhere without a click.

## Profiles

A **profile** is a separate web identity: cookies, site storage, saved
pages. Every desktop workspace has one of its own; there is nothing to
pick and no Profiles menu. A profile has no name (it is a uuid on disk),
so renaming a workspace changes nothing about its profile.

- Every page on a workspace runs its profile. Send a page to another
  workspace and it reopens there, in the same window, at the same address,
  under that workspace's profile; its back history and form state do not
  come along. A desk restart moves no page: each stays on its workspace,
  under its profile.
- A new workspace starts with a new, empty profile.
- A [pinned](sola-shell.md#pinned-windows) page keeps the profile it has, on
  every workspace. Unpinned, it reopens under the profile of the workspace it
  stays on, if that is another.
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

The vault belongs to no profile: one account serves every profile, and
its files are Vault's ([Vault](sola-vault.md#files)).

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

Pages show at 100%. Super with the wheel scrolls; it does not zoom. A
page zoomed with Ctrl and the wheel goes back to 100% the next time it
loads.

Click back or forward to go one step; **hold** the button for a menu of
that page's history, up to 12 steps. That history is saved with the
session, so it survives a restart.

## Media

Pages autoplay as they do in Chrome: a muted video plays while it is in
view and pauses when you scroll away; sound waits for your first click on
the page. A page whose window is not on screen (on another workspace,
behind another window in its zone, or under the desk's idle fade once the
fade is complete) rests, and its videos pause until it shows again.

A video in a page is fully opaque: the desk's opacity is left off it,
while the rest of the window shows the picture through at the usual
amount. This applies to a video that shows a picture and takes at least
15 % of the page's view, playing or paused, so small previews stay as
they are. A video that plays with sound and is at least half in view
holds the desk's idle fade where it is until it stops
([Compositor § The idle fade](compositor.md#the-idle-fade)). While a panel,
dialog, menu, or the find bar is over the page, its videos are not cut
out. Videos inside a frame from another site (an embedded player) are not
seen yet.

Oath's `pkg:cef` is built with H.264 and AAC, so typical MP4 video,
Steam store trailers included, plays.

## Page menu

Right-click a page for the desk's right-click menu, which the Shell draws
([Shell](sola-shell.md#the-right-click-menu)); Chromium's own menu does
not show. The page's rows come first:

- on a picture, **Use as Desktop Background**;
- on a link, **Open Link in New Window** (a page window behind) and
  **Copy Link**;
- **Back** (⌘←) and **Forward** (⌘→), dimmed with no page that way, and
  **Reload** (⌘R);
- **Inspect Element** and **Developer Tools** (⌘⌥I), except in a web
  app's window;
- on an `http` or `https` page, **Wrap This Page…** (see
  [Web apps](web-apps.md)).

Then the Edit rows: **Cut** with a selection in a field, **Copy** with a
selection, and **Paste** and **Select All** in a field; never Cut or Copy
out of a field Chromium will not copy from (a password field). In a
field, **Vault ▸** offers the page's logins first: **Fill** fills the
whole login as the vault panel does, and each of the login's values
(**Username**, **Password**, **One-time code**, and the rest) goes in at
the field's caret. **Search vault…** finds any item, and its values go in
the same way. Every field gets the same rows, a password field too
([Shell](sola-shell.md#the-right-click-menu)). The Shell gets the page's
address without its query or fragment, to find its logins.

Browser's own fields (the location bar, the vault panel's fields) get the
menu every Sola text field gets.

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

- Browser → **Developer Tools** (⌘⌥I), or **Developer Tools** in the
  right-click menu: the console.
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
  file in the app Sola keeps for its type (Paint for an image, Browser for a
  PDF); a type nothing claims does not open. A failed row says why (no
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

## The vault

The **vault** button in the top bar opens the vault panel, a sheet under
the button. Browser holds no keys and no Bitwarden library: the panel
asks [Vault](sola-vault.md)'s service, `sola-vaultd`, for everything it
shows, and does the page's part, the fills and passkeys. Signing in,
unlocking, and locking are the vault's, not Browser's: unlock it here and
Vault and the right-click menu are unlocked too.

The button shows the vault's state: a key in full ink while the vault
is unlocked; the key dimmed with a small lock on its corner while it is
locked; the key dimmed alone while you are signed out or the service is
not running; a fingerprint while a site waits for a passkey. It stays
pressed while the panel is up. A click
on the page closes the panel; a sign-in, a two-step code, or an unsaved
edit waits there for the next open.

### Before it opens

- **The service is not running**: "Vault isn't running", and **Try
  again**.
- **Signed out**: the sign-in form, as in Vault: **Email**, **Master
  password**, and **Server** (bitwarden.com, bitwarden.eu, or
  Self-hosted with its address). Then the second step, if the account has
  one: an authenticator code in six boxes, which the sixth digit sends
  (**Email a code instead** when the account has email codes), or an
  emailed code (**Send it again**). See
  [Signing in](sola-vault.md#signing-in).
- **Locked**: **Master password** and **Unlock**. **Use another account**
  signs the account out at once.

### The list

On a page with logins of its own, the panel opens on them under **On
this site**, the most recently used first; **All items ›** shows the
whole vault. A site with none says **No logins for this site**, with **New
login for this site**. On a page with no site, or past the site's logins,
the panel lists every item: **Recently used** (five), then **A–Z**, with
the kinds **All**, **Logins**, **Cards**, **Identities**, and **Notes**
above. Typing in **Search vault** searches every item, the page's own
first, with a count.

- On a site's login, a click or Return **fills** it, and the arrow at its
  right opens it. Anywhere else, a click or Return opens the item.
- ↑ and ↓ choose a row; ↓ past the site's last login goes on into the
  whole vault. → opens the chosen item.
- When the last sync failed, a line says so, with **Retry**: the items
  shown are the last sync's, or, when there is none to show, there are no
  items until it syncs. Opening the panel syncs when the last sync is more
  than five minutes old.

The head names the page's site (or says **Vault**) and has **+** for a
new login and **···**: **Sync now** with when it last synced, **New
login…**, **Open Vault**, **Log out**, and the account's email. **Log
out** signs the account out, as Vault's **Sign Out** does, without
asking. The foot says when the vault synced and who is signed in.

### A record

An item's record has its fields in groups, label over value: secrets as
dots until shown, **Copy** on every field, **Open** on a website (a page
window in front), the one-time code large with its ring and **Copy**,
passkeys, custom fields, and notes; then when it was edited and last
used. **Fill** at the foot (Return) fills a login; a card has **Fill
card** and an identity **Fill identity**. The pencil edits the item, when
your account may. **···** adds **Open in Vault**: Vault comes forward on
the item, starting if it is not running, and the panel closes.

A value copied from the panel leaves the clipboard after Vault's
clipboard time, if nothing was copied since
([Settings](sola-vault.md#settings)).

### Filling a page

**Fill** asks the service for the item, for the page's address. The
service gives a login only to a page one of its websites matches
("This login is not for this page."). Browser then runs the fill in the
frames that may have it, while the page stays on the origin it had:

- **A login**: its username and password into the page's frames of the
  same origin, and into frames one of its websites matches (a sign-in form
  in another site's frame gets nothing). A login with a one-time code also
  puts the code into the page's code field, if it has one, and copies it
  (Vault → Settings → **Filling a login copies its code**, on at first);
  the code leaves the clipboard after the clipboard time.
- **A card**: the card's fields, in every frame of the page, since
  payment forms live in the payment processor's frames.
- **An identity**: name, address, email, phone, and numbers, in the
  page's frames of the same origin.

Filling closes the panel. An item that asks for your master password asks
first, in the panel; the answer holds while the panel stays open. The
right-click menu's **Fill** does the same as the panel's, without opening
it ([Page menu](#page-menu)); for a login the panel's list does not have
yet, Browser asks the service whether it asks for your master password
before it fills. When the service refuses the fill, or the page can't be
filled, the panel opens and says why.

### Editing and new logins

- **The pencil** turns the record into fields: the name, the kind's
  fields (a password has **Generate**, for a new 16-character one; dates
  as `YYYY-MM-DD`; a card's month as `MM` and year as `YYYY` or `YY`),
  websites with their match type, custom fields (**Add field**), and
  notes. A dot marks what changed, and **Save** writes only that; a
  changed password goes to the login's history. **Cancel** or Escape goes
  back. Trash, folders, favorites, and moving items are Vault's.
- **+** (or **New login…**) makes a login for the page: **Username**
  (empty), **Password** (a generated 16-character one, shown, with
  **Generate** for another), and **Website** (the page's host, without
  `www.`). **Save and fill** (or **Save**, off a page) writes it to your
  own vault, named for the site's domain, and then fills every username
  and password field on the page, the confirm field too. A page with no
  such fields keeps the panel open: "The login is saved. This page had no
  username or password field to fill." A login needs a password.

### Passkeys

- **Signing in**: when a site calls WebAuthn `navigator.credentials.get`,
  the panel opens (on the sign-in or unlock form first, if needed) on
  the passkeys saved for that site ("Sign in to github.com with a
  passkey").
  Pick one, or press Return, to finish signing in. Escape or **Cancel**
  hands the page its error, so it can offer another way in. Browser puts
  this in every frame before the page's own scripts run (Google sign-in,
  for one), so pages never reach Chromium's own passkey sheet, which
  cannot show off-screen.
- **Saving**: when a site calls `navigator.credentials.create`, the panel
  opens on "Save a passkey for github.com". **Add it to a login here**
  lists the site's logins; **Save as a new login** makes a login named
  for the site, with the username the site gave.

The same site asking again while the panel is up stays one request. A
request ends when its page leaves the site, and the foot says how long
the site still waits. The service checks the site's relying-party id
against the frame's own origin and signs; the passkey's private key never
leaves it.

## Copy, paste, and click

The shell takes the ⌘ chords of the focused application's menus, so ⌘C
and ⌘V reach Browser as Edit → Copy and Paste, and Browser hands them to
the page or the field that has focus (the location bar, a vault field).
⌘Z and ⌘⇧Z undo and redo in the field that has focus (the location bar,
the find bar, a vault field), each with its own history
([Text fields](theme.md#text-fields)), and otherwise in the page; never
in a vault item's notes. Copy and Cut work in the part of the page that
has the keys, an embedded frame too, and write what the page copies (the
selection, or a web terminal's own) to the system clipboard; Paste reads the clipboard and
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

## Favicons

Each page's row in the bar shows its favicon (the browser's globe until the
page has one; an SVG favicon, which CEF's image download does not decode,
is fetched and drawn to a PNG): the browser writes each page's latest favicon to
`~/.cache/sola/browser/favicons/` and names it on the page's tab. A web
app made before its page had a favicon (its icon the globe) takes the
page's favicon as its icon once one comes, in the bar and the launcher;
a web app with an icon keeps it.

## Light or dark

Pages are told whether you prefer light or dark (`prefers-color-scheme`):
**Settings → Appearance → Light or dark**, Dark by default. A site with
both looks shows the one you chose; one with a single look is unchanged.
Pages that load after a change follow it at once; one already open
follows it when it is next on screen. Web apps follow it too. Design's
boards do not. The pages a desk start brings back wait for it (or for the
bus to say it has replayed everything it holds, when none is saved) before
they load, so a site that reads it only once,
as it loads, comes up in the right one. The browser's own chrome wears
the desk's theme, light or dark.

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
| ⌘Z / ⌘⇧Z | Edit → **Undo** / **Redo** (in the focused field, else the page) |
| ⌘X / ⌘C / ⌘V / ⌘A | Edit → **Cut** / **Copy** / **Paste** / **Select All** |
| ⌘F / ⌘G / ⌘⇧G | Edit → **Find** / **Find Next** / **Find Previous** |
| Escape | Stop the load |

Browser → **Wrap This Page…** has no key.

## Limits

- No Save As, no choice of download folder, no "show in folder", and no
  deleting a downloaded file from the list.
- The right-click menu has no Save Image or Copy Image.
- The vault panel does not trash, restore, or delete items, or change
  their folders and favorites; Vault does
  ([Vault](sola-vault.md#the-window)). Its edit cannot add a hidden
  custom field.
- Back-and-forward history is per page; there is no history window
  beyond the location bar's matches.
- Pages' icons show nowhere (a web app takes its icon from one).
