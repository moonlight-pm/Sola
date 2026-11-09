# Vault

Vault is Sola's own Bitwarden client. It comes in Oath's `pkg:sola-vault`
as two programs: the service `sola-vaultd`, which alone holds the
unlocked vault, and the app `sola-vault`, the window you manage the vault
in. Its launcher row is **Vault** (`lucide/key-round`). Browser's vault
panel ([Browser](sola-browser.md#the-vault)) and **Vault ▸** in the
right-click menu ([Shell](sola-shell.md#the-right-click-menu)) are clients
of the same service, so signing in or unlocking in one opens the vault
for all of them.

Vault holds one Bitwarden account at a time, on bitwarden.com,
bitwarden.eu, or a self-hosted server. It reads and writes every kind of
item: logins, cards, identities, notes, SSH keys, bank accounts, driver's
licenses, and passports, with folders, organizations' collections,
favorites, and the trash.

## The service

`sola-vaultd` runs as Oath's `svc:sola-vault`. It holds the Bitwarden
session, the keys, and the synced items, and does the syncing, the
matching of logins to pages, one-time codes, passkey signing, the
generator, and deliveries into other applications' fields. Vault,
Browser, and the Shell ask it for what they show; none of them holds a
key. If Vault or Browser crashes, the vault stays unlocked; if the service
restarts, it locks ([Locking](#locking)).

It answers on its own socket, `$XDG_RUNTIME_DIR/sola-vault.sock`
(`/run/user/1/sola-vault.sock` on Oath), mode `0600`. A second
`sola-vaultd` that finds the socket answering exits.

### Who may use it

The service checks each connection once, when it opens. The kernel says
which process is on the other end, and that process's program must be a
Sola binary in Oath's store (`/oath/store/pkg/`) or in `/opt/sola/bin/`.

| Program | May |
|---|---|
| `sola-vault`, `sola-browser`, `sola-shell` | Everything: sign in, unlock, lock, list, read, write, fill pages, passkeys, the generator, export, and deliver a value into another application |
| Any other `sola-*` program (Terminal, Projects, Mail, Settings, …) | Only collect a value the Shell had the service deliver to that same process ([Vault values](sola-shell.md#vault-values)) |
| `solactl` and every other program | Nothing: each request is refused with "This program may not ask the vault that." |

`SOLA_VAULT_DEV_ROOTS`, in the service's environment, adds directories
(absolute, colon-separated) whose Sola binaries may connect, for programs
built in a checkout (`target/release`). The service logs a warning while
it is set.

### Nothing secret leaves the socket

- **The bus** carries `VaultState` (sticky): signed in, unlocked, the
  account's email, its server, the last sync, and how many items. No item
  names and no values ([The bus](bus.md#app-state)).
- **The call plane** has owner `vault` with four methods: `status`,
  `lock`, `sync`, and `open`
  ([solactl](solactl.md#vault-solactl-vault)). An agent can see whether
  the vault is unlocked, lock it, sync it, or open Vault on an item. No
  method returns an item or a value, and none fills a page.
- **Logs** name a page by its host only and never hold a value. The
  service writes Sola's log (`~/.local/state/sola/sola.log`), and PID 1
  keeps its output (`oath logs svc:sola-vault`).
- Other processes of `home`, short of `sudo`, cannot trace the service,
  read its memory, or get a core dump of it. Its memory is kept out of
  swap only when its memlock limit is unlimited; otherwise its log says
  so at start.

This keeps values from leaking by accident and from being handed to an
agent that asks. It does not stop a program of yours that sets out to
get in: `home` can use `sudo`, and a program can read the master password
as you type it ([Security](security.md#what-stays-open)).

## Signing in

With no account signed in, Vault's window is one card: **Email address**,
**Master password** (**Show** reveals it), and **Server**:
**bitwarden.com** (the default), **bitwarden.eu**, or **Self-hosted**
with its address (`https://vault.example.com`). A self-hosted server must
be an `https://` address; the service reaches its API at
`<address>/api` and its sign-in at `<address>/identity`. **Sign in**, or
Return. Browser's panel has the same form.

The card remembers the last email. Bitwarden lists this computer as
**Sola Vault**, a Linux desktop, under a device id the service makes once
and keeps, so signing in again later is not a new device.

Bitwarden's key derivation is slow on purpose (600,000 PBKDF2 iterations
unless the account says otherwise), so signing in and unlocking take a
moment.

### Two-step login

When the account asks for a second step, the card asks for it:

- **An authenticator app**: six boxes, digits only; **Verify** or Return
  sends the code. **Email a code instead** switches to email when the
  account has it.
- **An emailed code**: one field. **Send it again** if nothing came.
- **A new device**: Bitwarden's new-device check emails a code by itself
  ("This computer is new to Bitwarden. It sent a code to …").

Security keys, Duo, and YubiKey OTP are not supported. An account that
has one of them besides an authenticator or email codes is told so on the
card; an account with only those cannot sign in here. A sign-in waits ten
minutes for its second step. **Back** returns to the form.

## Locking

The vault's key is kept in the service's memory only, never on disk.

- After a reboot, or when `svc:sola-vault` restarts (an update of
  `pkg:sola-vault` restarts it), the account is still signed in and the
  vault is locked. Your master password unlocks it, in Vault, in Browser's
  panel, or through **Unlock Vault…** in the right-click menu, which brings
  Vault forward. The unlock itself needs no network; the sync after it
  does. Without one, the vault shows the items from the last sync, which
  the service keeps on disk ([Files](#files)), with the sync's error and
  **Retry**.
- **Lock after** (Vault → Settings; 1 hour by default) locks it when idle.
  With the desk's idle fade on (Settings → Appearance → Idle, on by
  default), idle means no input on the desk: the vault locks that long
  after your last input, and not before the windows start to fade. With
  the fade off, the time counts from the last time Vault, Browser, or the
  right-click menu used the vault. **Never** leaves locking to the screen
  lock and to restarts.
- **Lock when the screen locks** (on by default) locks it as the lock
  screen comes up: `sola-lock` says so on the bus (`ScreenLock`), and the
  service locks on it ([Security](security.md#locking)).
- **Vault → Lock** (⌘⇧L), the lock in Vault's foot, **Lock Vault** in
  the Shell's flower menu (there while the vault is unlocked;
  [Shell](sola-shell.md#flower-menu)), and `solactl vault lock` lock it
  at once.

Locking drops the key, the synced items, and any value waiting to be
delivered. The window goes back to the unlock card: the account's email
(and its server, when that is not bitwarden.com), **Master password**,
and **Unlock**. **Use another account** on that card signs the account
out at once, without asking.

Signing out is more than locking. **Vault → Sign Out…** asks first; then
the service forgets the account on this computer, and the last sync it
kept, keeping only its email for the form. Signing in again needs the
master password and the second step. When Bitwarden refuses to renew the
session, the service signs out the same way and Vault shows the sign-in
card.

## The window

Once the vault is unlocked, Vault has three columns, as Mail does: the
places, the list, and the record. The bar over the list and the record
names the place and how many items it holds, with the search field
(**Search vault**, ⌘F) and **+** for a new item. The foot says when the
vault last synced (click it to sync), who is signed in, and has a lock.

While the service is not running, the window says **The vault service
isn't running** and connects by itself once it starts. A Vault the
service will not let in (a program outside Sola's places) says **The
vault service didn't let Vault in**.

### Places

- **All items** and **Favorites**.
- **Logins**, **Cards**, **Identities**, **Notes**, and **SSH keys**;
  **Bank accounts**, **Licenses**, and **Passports** when the vault has
  any.
- **Folders**: each folder, **No folder** once there are folders, and
  **+** for a new one. Double-click a folder to rename it.
- Each organization by name, with its collections that hold items.
- **Trash**.

Each place shows its count. Items in the trash show only in **Trash**.

### The list

Each row is an item: a plate (a login's first letter, or the kind's
glyph), its name with a fingerprint for a passkey and a clock for a
one-time code, a star for a favorite, and its username, card, or person
under it. **View → Sort by Name** is the default; **Sort by Last Used**
puts first the items that last filled a page, went into a field, or
signed in with a passkey.

The search narrows the place you are in. Every word you type must be in
an item's name, usernames, websites, or the first words of its note,
never its password. Return opens the first match; Escape clears the
search. ↑ and ↓ walk the list.

When the last sync failed, a line at the top of the list says why and
when the items are from, with **Retry**; when there is no sync to show
yet, it says there are no items until it syncs. Items an organization's
key could not open are counted at the foot of **All items**.

### The record

The record shows the item in grouped rows, label over value:

- The kind's own fields: a login's username and password; a card's
  cardholder, brand, number, expiry, and security code; an identity's
  name, address, and numbers; an SSH key's fingerprint and public and
  private keys; and so on. Secrets show as dots until **Show**. Every row
  has **Copy**.
- **One-time code**, large, with a ring for the seconds it has left, and
  **Copy**. A stored authenticator key Vault cannot make codes from
  (another algorithm, Steam) shows as **Authenticator key**, kept as it
  is.
- Websites, each with its match type when that is not the default,
  **Open** (in Browser), and **Copy**; then the login's passkeys, by
  account and the day each was saved.
- Custom fields; notes; **Password history**, folded, each old password
  with **Show** and **Copy**; attachments, by name and size.
- When it was edited and created.

The head has the star (**Favorite**), the pencil (**Edit**, when your
account may change the item), and **···**: **Clone**, **Move to
Folder…**, **Move to Organization…**, and **Move to Trash**. An
organization's item you may only read has no pencil. When an organization
hides an item's password from you, the record shows neither the password
nor its history; Browser's **Fill** and the right-click menu still put it
in, as Bitwarden's own clients do.

### Editing

**Edit** (⌘E, or the pencil) turns the record's groups into fields in
place. A dot marks each changed field, and **Save** writes only those,
onto the item as Bitwarden has it then: a change made elsewhere to another
field is kept. Changing a login's password puts the old one in password
history. The form has:

- the kind's fields; on a password, **Generate** opens the generator, and
  **Use It** puts its result in the field;
- **Websites**: add or remove them, each with a match type: Default
  (Bitwarden's, the domain), Domain, Host, Starts with, Exact, Regular
  expression, or Never;
- **Fields**: **Add field** and **Add hidden field**;
- notes, the folder, and **Ask for the master password to open it**.

Return in a field saves. Escape or **Cancel** leaves the form; with
changes, Vault asks **Discard your changes?** (**Keep editing**,
**Discard**), and so does choosing another item. If the item changed on
the server since, **Save** says "This item changed elsewhere. Sync, then
edit it again."

### New items

**+** in the bar lists the kinds (Login, Card, Identity, Note, SSH Key,
and the newer kinds the vault has) and **Folder…**. **File → New Item**
(⌘N) makes an item of the place's kind, a login anywhere else; in a
folder, the new item goes in that folder. The form is the edit form,
empty; a name is all it needs. New items go to your own vault, not an
organization's. **Clone** opens the form on a copy, its name ending in
" - Clone", saved only on **Save**.

### Favorites, folders, and organizations

- The star in the record's head, or **Item → Favorite**, puts the item in
  **Favorites** or takes it out.
- **Move to Folder…** lists **No folder** and every folder.
- **File → New Folder…** (⌘⇧N), **+** beside **Folders**, or **Folder…**
  in the bar's **+** makes a folder. In a folder's place, the folder
  button in the bar has **Rename Folder…** and **Delete Folder…**.
  Deleting a folder asks first; its items stay, in no folder.
- **Move to Organization…** shares an item of your own with an
  organization you belong to: choose the organization, then at least one
  of its collections you may add to. The item cannot come back to your own
  vault.

### Trash

**Move to Trash** asks nothing; the item moves to **Trash**. There its
record says since when it has been in the trash, with **Restore** and
**Delete Forever…**, which asks first. There is no way to empty the whole
trash at once.

### Re-prompt

An item set to **Ask for the master password to open it** shows a form in
place of its record: **Master password** and **Continue**. Once you give
it, the item stays open to you until the vault locks or Vault quits.
Browser's panel asks the same before it opens or fills such an item. The
right-click menu cannot ask, so it puts none of such an item's values into
a field; it shows them dimmed, with **Open in Vault…**
([Shell](sola-shell.md#the-right-click-menu)).

### Copying

**Copy** on a row, or **Item → Copy Username** (⌘U), **Copy Password**
(⌘P), and **Copy Code** (⌘T) on the open item, put the value on the
clipboard, and the row says **Copied** for a moment. After the clipboard
time (Settings; 30 seconds by default) Vault takes the value off the
clipboard again, but only if the clipboard still holds it: anything you
copied since stays. Quitting Vault takes it off at once. Vault keeps a
keyed hash of what it copied, not the value.

The right-click menu never uses the clipboard
([Vault values](sola-shell.md#vault-values)).

### The generator

**File → Generator…** (⌘G) makes a **Password**, a **Passphrase**, or a
**Username** (a random word).

- Password: **Length** (5 to 128, 16 at first), **Capitals A–Z**,
  **Lower case a–z**, **Digits 0–9**, **Symbols** (`!@#$%^&*`), and
  **Avoid look-alikes (l, 1, O, 0)**. It has at least one digit and one symbol
  when those are on.
- Passphrase: **Words** (3 to 20, 4 at first), **Between words** (one
  character, a hyphen at first), **Capitalize**, and **Include a number**.

A line under the result says what it is made of. Every change makes a new
one, and so does **Make another**. Opened from the menu, **Copy** copies
it (and it leaves the clipboard after the clipboard time); opened from a
password field, **Use It** puts it in the field. The options last until
Vault quits.

### Settings

**Vault → Settings…** (⌘,), while an account is signed in:

| Setting | Choices | At first |
|---|---|---|
| Account | The email and server; **Open the Web Vault**; **Sign Out…** | — |
| Lock after | 1, 5, 15, or 30 minutes, 1 or 4 hours, Never | 1 hour |
| Lock when the screen locks | On or off | On |
| Clear a copied value after | 10, 20, or 30 seconds, 1, 2, or 5 minutes, Never | 30 seconds |
| Filling a login copies its code | On or off | On |

The service keeps these, and each change takes effect at once. The
clipboard time also applies to what Browser's panel copies, and the last
row is Browser's ([Filling a page](sola-browser.md#filling-a-page)).

### Export

**File → Export…**, while the vault is unlocked, writes the vault in
Bitwarden's formats: **JSON, encrypted** (the default; with a file
password of its own, typed twice), **JSON**, or **CSV**. It asks for
your master password, as Bitwarden's own apps do: **Export…** checks it
(nothing is written while it is wrong, "That isn't your master
password.") and then opens the file panel's Save in `~/Downloads`, with
the name `bitwarden_export_YYYYMMDD.json` (or `.csv`); cancelling the
panel asks for it again next time. The file holds your own items and
folders, not organizations' items or the trash, and no attachments. Only
you can read it (mode `0600`). A **JSON** or **CSV** export is every
password in plain text.

## Opening Vault on an item

`sola-vault --item <id>` opens Vault on that item; an item's id is the
UUID Bitwarden gave it. Only one Vault runs: a second `sola-vault` hands
its item to the open window, which comes forward, and exits.
`solactl vault open --id <id>`, or with no id, does the same through the
service, which has the session manager start Vault; Browser's **Open in
Vault** uses it. On a locked vault, the item opens after you unlock. An
id the vault does not have shows "That item isn't in the vault."

## Sync

The service syncs when you sign in or unlock, after every change saved
through it (from Vault or Browser), and when asked: **Vault → Sync Now**
(⌘R), a click on the sync line in Vault's foot, **Sync now** in Browser's
panel, or `solactl vault sync`. Browser's panel also syncs when it opens
more than five minutes after the last sync. Nothing syncs on a timer, so
a change made elsewhere (the web vault, a phone) shows after the next
sync. The service syncs before it saves an edit, so the edit lands on the
server's latest copy.

## Menus and keys

| Menu | Items |
|---|---|
| Vault | Settings… (⌘,), Sync Now (⌘R), Lock (⌘⇧L), Sign Out…, Close Window (⌘W), Quit Vault (⌘Q) |
| File | New Item (⌘N), New Folder… (⌘⇧N), Generator… (⌘G), Export… |
| Edit | Undo (⌘Z), Redo (⌘⇧Z), Cut (⌘X), Copy (⌘C), Paste (⌘V), Select All (⌘A), Find (⌘F) |
| Item | Edit (⌘E), Copy Username (⌘U), Copy Password (⌘P), Copy Code (⌘T), Favorite, Clone, Move to Folder…, Move to Organization…, Move to Trash |
| View | Sort by Name, Sort by Last Used |
| Window | The Window menu every Sola application has |

The Item menu acts on the open record. While the vault is locked, only
Settings, Lock, Sign Out, Close Window, Quit, and Edit's field actions do anything. Lock
is ⌘⇧L because ⌘L locks the screen.

| Key | Action |
|---|---|
| ↑ / ↓ | The previous or next item in the list |
| Return | In the search, open the first match; in a form, sign in, unlock, verify, or save |
| Tab / ⇧Tab | The next or previous field |
| Escape | Back out one step: a menu, a sheet, the edit (asking first if something changed), a re-prompt, the search, the second step |

## Files

| Path | What |
|---|---|
| `~/.local/share/sola/vault/` | The service's folder, mode `0700`; `$XDG_DATA_HOME/sola/vault/` when that is set |
| `…/vault/prefs.json` | The device id Bitwarden knows this computer by, the last email, and when each item (by id) was last used. Nothing secret |
| `…/vault/settings.json` | Vault → Settings: lock after, lock with the screen, the clipboard time, copying the code on fill |
| `…/vault/session.json` | The signed-in account, as Bitwarden's own clients keep a locked account: the email and server, the access and refresh tokens, the key derivation's settings and salt, the user id, the account key as your master password wraps it, and the private key that account key encrypts. Age-encrypted with `~/.config/sola/key`, mode `0600`. **Sign Out** deletes it |
| `…/vault/sync.json` | The last sync, as Bitwarden sent it: the items, folders, and collections, encrypted with the account key (and organizations' keys), and the organizations with their keys as the account key wraps them. Age-encrypted with `~/.config/sola/key` besides, mode `0600`. Written after every sync that works; read at an unlock; **Sign Out** deletes it |
| `$XDG_RUNTIME_DIR/sola-vault.sock` | The service's socket |
| `$XDG_RUNTIME_DIR/sola/vault.sock` | Vault's own: a second `sola-vault` hands its item to the open window here |

Never on disk: your master password, the unwrapped account key, and any
item in the clear. The items on disk are as Bitwarden sends them, and
open only with the account key, which only your master password unwraps.
At an unlock the service reads them from `sync.json`, then syncs; they
stay in its memory until the vault locks.

## From Browser's vault

Before Vault, Browser held the vault itself, and kept the unlocked
session, decrypted key and all, in
`~/.local/share/sola/browser/vault-session.json`. Each time
`sola-vaultd` starts, it takes over what Browser left:

- From `~/.config/sola/browser/vault.json` it takes Browser's device id,
  unless it has one already, so Bitwarden sees no new device and sends no
  new-device email; and with it the last email and when items were last
  used. That file stays where it is; nothing reads it afterwards.
- From `vault-session.json` it takes the account without the decrypted
  key: signed in on bitwarden.com, locked. Unlock it once with your master
  password. A session file Browser wrote before it kept the wrapped key
  gives only the email, and you sign in again. Either way the service then
  deletes the file.

## Limits

- No import.
- Attachments are listed by name and size only: they cannot be opened,
  saved, added, or removed here.
- No Send, and nothing of the account itself: the master password,
  two-step setup, emergency access, organizations' administration, and
  billing are on the web vault (Vault → Settings → **Open the Web
  Vault**).
- Passkeys show on their logins but are made only by Browser, when a site
  asks ([Passkeys](sola-browser.md#passkeys)); Vault cannot add, change,
  or remove one.
- SSH keys are stored and shown; Vault does not make them and does not
  serve them to `ssh`.
- Until the service has synced once (after signing in, or the first
  unlock after an update from a build that kept no `sync.json`), an
  unlock while Bitwarden cannot be reached has no items to show; the
  list says so until a sync works.
- Nothing syncs on a timer.
- An item's notes have no **Undo** or **Redo**; the one-line fields do,
  each with its own history ([Text fields](theme.md#text-fields)).
- The sort order and the generator's options are not kept after Vault
  quits.
