# Vault — Sola's Bitwarden client, a desk service, and the right-click menu

**Date:** 2026-10-31  
**Status:** **Agreed in shape** 2026-10-31 (Joshua: "All your
recommendations sound good. Don't worry about stopgaps. We'll get this done
today."). V1–V7 decided as proposed. Slices 1–5 built the same day on branch
`vault`, merged to master and applied on novus the same evening (`sola-vault-proto`, `sola-vaultd`, `sola-vault`, the browser on
the service with the canvas's look, the desk menu in the shell, the kit,
terminal and Projects). Slice 6 (2026-10-31, Joshua): the launcher's vault search declined; the SSH agent, Mail
passwords, import, attachments, Send and other apps' windows parked in
[ideas](../ideas/2026-10-31-vault-later.md); in any field the right-click
menu offers every value of an item ("any field is fine to show the items").  
**Related:** [vault panel](2026-08-28-sola-browser-vault-panel-design.md);
[item edit](2026-09-11-sola-browser-vault-item-edit-design.md);
[browser agent control](2026-09-11-sola-browser-agent-control-design.md)
(vault fill never returns the secret);
[call plane](2026-08-13-sola-call-plane-design.md);
[keybindings](2026-10-30-keybindings-design.md) (the shared Edit actions);
canvas `design/vault`; open-questions D3, D17; capabilities `browser`, `vault`.

| | |
|--|--|
| **Asked** (Joshua, 2026-10-31) | "round out this overall feature for the whole desktop. I don't want to use the official bitwarden client, so lets make a new custom client that matches our desk .. called 'Vault' / sola-vault." Full features to manage everything in the vault; one account. Move the browser's vault logic into it, "which should run partly as a system service". Extend the vault to apps other than the browser. "a desktop-level right-click menu -- such menus can have some standard items (in this case, pasting from vault items) -- plus copy and whatnot. Applications can also add their own items (browser: inspect element, open devtools, etc)." |
| **Today** | The vault lives inside `sola-browser` (`src/vault/`, 8.5k lines, the Bitwarden SDK behind the `bitwarden` feature): a worker thread per chrome, the panel UI in `app.rs`, fill and WebAuthn scripts. Only the browser can use it. The unlocked session (`vault-session.json`) holds the **decrypted user key**, age-encrypted with `~/.config/sola/key` — both readable by anything running as `home`, agents included. Right-click menus: the browser draws its own (kit `context_menu`, in the window); no other app has one. |

## The shape

Three parts, and the browser becomes a client of them.

```text
                       Oath svc:sola-vault
                    ┌──────────────────────┐
  Vault (app) ─────▶│  sola-vaultd         │── Bitwarden (identity, api)
  browser panel ───▶│  keys, sync, match,  │
  shell (desk menu)▶│  TOTP, passkeys,     │── bus: VaultState (no secrets)
  any kit app ─────▶│  generator, deliver  │── call plane `vault`: status,
   (redeem only)    └──────────────────────┘   lock, sync, open (no secrets)
        private socket, peer-checked
```

1. **`sola-vaultd`** — the one place the vault is unlocked. A headless
   daemon run by PID 1 as `svc:sola-vault` (user `home`, restart always,
   wants `svc:sola-bus`, `svc:sola-call`). It owns the SDK, the keys, sync,
   the Bitwarden API, URI matching, codes, passkey signing, the generator
   and delivery. Everything else asks it.
2. **Vault** (`sola-vault`) — the app: manage everything in the vault, in
   the desk's look. A client of the daemon; it holds no keys.
3. **The desk menu** — the shell draws right-click menus for every kit app:
   the app's own items, the standard ones (Edit), and the vault's (insert a
   username, password or code into the field you clicked). Apps describe
   the menu; the shell shows it; a pick goes back to the app.

Package: `pkg:sola-vault` holds both binaries, so moving it restarts only
the vault (never the desk). The `svc:` entry is an Oath change (the desk
profile in Oath's repo); on novus it can be overlaid first.

## Proposed (to agree)

| | Proposal |
|--|--|
| P1 | **A daemon and an app, not one process.** The daemon is small and always on (tokio + the SDK, no GPU); the app opens when you want it, like `sola-bots` / `sola-botsd`. A UI crash never locks the vault. |
| P2 | **Secrets never ride the bus or the call plane.** The call plane is what agents reach (`solactl mcp` exposes every live method, and observers get a copy of every invoke). The daemon serves secrets only on its own socket (`$XDG_RUNTIME_DIR/sola-vault.sock`, `0600`). Its call-plane owner `vault` has status, lock, sync and "open this item in Vault" — never a value. |
| P3 | **The socket checks who connects.** `SO_PEERCRED` gives the pid; `/proc/<pid>/exe` must be a Sola package binary allowed for that use: Vault, the browser and the shell for reads and writes; any Sola kit app only to redeem a delivery made for its own pid. `solactl` is never allowed a secret. |
| P4 | **Delivery by ticket.** When you pick a vault item in the desk menu, the shell asks the daemon to deliver item + field to the app's pid; the daemon returns a one-time ticket (60 s), the shell tells the app "redeem T" (no secret on the wire), and the app fetches the value from the socket, bound to its pid, and inserts it at the caret (kit `edit_focused(Paste)`, never the clipboard). |
| P5 | **The browser keeps the page, loses the keys.** Fill and WebAuthn scripts, frame scoping and the panel stay in the browser; the SDK, the session and the worker move to the daemon. The browser sends a page's origin (from CEF) and gets fill material or a signed assertion back. It drops the `bitwarden` feature, so it builds without the SDK. |
| P6 | **The shell draws the desk menu.** One look everywhere, a menu that can leave its window, and the vault's item list never enters the app — the app only ever receives the one value you picked. If the shell is away, the kit draws the app's items and Edit in the window as today (no vault). |
| P7 | **The daemon is hard to read.** `prctl(PR_SET_DUMPABLE, 0)` (no ptrace, no `/proc/<pid>/mem` from the same user), key material `mlock`ed and zeroized end to end; no secret in a log line. |
| P8 | **Migration keeps the device.** The daemon takes the browser's `vault.json` (its device id, so no new-device email) and, by V1, either the session or nothing; the browser's files are removed after. |

## The daemon

**State:** `~/.local/share/sola/vault/` (prefs, device id, the encrypted
item cache Bitwarden already encrypts), nothing secret in plain text.
Unlock state per V1.

**Socket methods** (JSON lines, versioned; new methods and fields only, never
a changed one, since the browser, shell and Vault can be from different
commits):

| Group | Methods | Who |
|-------|---------|-----|
| Account | `status`, `login`, `two_factor`, `email_code`, `unlock`, `lock`, `logout`, `verify_master_password` | Vault, browser, shell |
| Read | `list` (summaries, no secrets), `get` (a full item), `matches` (an origin's logins), `code` (a TOTP now), `history` | Vault, browser, shell |
| Write | `create`, `update` (the diff-based apply, today's `edit.rs`), `trash`, `restore`, `delete`, `move`, folders, favorites, attachments | Vault, browser |
| Fill | `fill_material` (an item for an origin, today's `FillMaterial`) | browser |
| Passkeys | `passkey_candidates`, `passkey_assert`, `passkey_create` (the daemon checks the rp id against the origin again) | browser |
| Generate | `generate` (password, passphrase, username) | Vault, browser, shell |
| Deliver | `fields` (an item's values by label, never a value); `deliver` (item, field, target pid) → ticket; `redeem` (ticket) → value | shell; any Sola kit app (its own tickets) |
| Events | `subscribe`: lock state, sync, item changes (ids only) | all of the above |

**Bus:** `Topic::VaultState` (sticky): signed in, locked, email, last sync,
item count. For the browser's key button, the desk menu and Settings.

**Call plane `vault`:** `status`, `lock`, `sync`, `open` (show an item in
Vault). An agent can lock the vault or ask for a sync; it can never read
one. Fills an agent asks for (the value never returned) wait for D3.

## The app — Vault

Kit window, Mail's three columns:

- **Sidebar (220):** All items, Favorites; Logins, Cards, Identities,
  Notes, SSH keys; Folders; each organization's collections; Trash.
- **List:** plates in enamel (the canvas), name, subtitle, marks
  (passkey, code), sorted by name or last used; search over everything.
- **Record:** the canvas's grouped rows — copy on every field, reveal,
  open a website, the code with its ring; password history; attachments;
  "Edited / Created". Edit turns the groups into fields in place and saves
  only what changed.
- **Make:** new item of each kind, clone, move to a folder or an
  organization, favorite, trash / restore / delete forever.
- **Generator:** a sheet (password, passphrase, username), also offered
  in every password field.
- **Settings:** account (email, server — V5), lock (V1), clipboard clear
  time, re-prompt, delivery into other apps (V4).
- **Import / export:** Bitwarden JSON (encrypted or not) and CSV out; the
  SDK's importers in.
- **Menus:** Vault (Settings, Lock, Log out, Quit), File (New item ⌘N,
  New folder, Import, Export), Edit (the shared actions), Item (Edit,
  Clone, Favorite, Copy username / password / code, Move, Delete), View,
  Window.

Not in Vault: what the web vault does for the account — master password,
two-step setup, emergency access, organizations' admin, billing. A link
opens it. Send waits (V7).

## The desk menu

**Right-click in a kit app:**

```text
app ── kit context_area: the context + the app's items ──▶ shell
shell: app items │ Edit (Cut, Copy, Paste, Select All) │ Vault ▸
pick:  app item  → MenuAction to the app (as the titlebar menus do)
       Edit      → MenuAction "cut" / "copy" / "paste" / "select_all"
       Vault     → daemon deliver → ticket → app redeems → value at the caret
```

- **The context** the app sends: what was clicked (a field, a selection,
  a link, a picture, a page), whether it is editable or secret, whether
  there is a selection, and a site when the app has one (the browser's
  page). Never the text itself.
- **The app's items:** id, label, key, enabled, separators, one level of
  submenu. Sent with each right-click, not published.
- **Edit** follows the context (Cut only with a selection in an editable
  field, Paste only in an editable one).
- **Vault ▸**, in an editable field: the site's logins first, then
  Search vault… (the shell's picker over every item, of every kind);
  Unlock vault… when locked. Each item is a submenu of every value it
  has, by label — a login's username, password and code, a card's
  number, custom fields, notes. The same in every field, a secret one
  too (Joshua, 2026-10-31: "We don't need special handling, any field is
  fine to show the items").
- **Drawn by the shell** as an overlay like its menus (Graphite dropdown:
  raised, 12 radius, 26 px rows), at the pointer, with keys (arrows,
  Enter, Escape, typing to search in the vault submenu).
- **The browser** moves its page menu here: Open Link in New Window, Copy
  Link, Use as Desktop Background, Back, Forward, Reload, Inspect Element,
  Developer Tools — and in a page's field, Vault ▸ fills the login or
  inserts one value.
- **Terminal and Projects:** Copy, Paste, Clear, and Vault ▸ types into
  the session (a `sudo` or `ssh` prompt). **Mail, Calendar, Settings:**
  Edit and Vault in their fields.
- **Other applications** draw their own menus; the desk menu does not
  reach them unless V4 adds a key for it.

## The vault beyond the browser (later slices — parked, see [ideas](../ideas/2026-10-31-vault-later.md); the launcher's search declined)

- **Launcher:** vault items in its search; Enter copies the password,
  with keys for the username and code (cleared after the clipboard time).
- **The screen lock** locks the vault (V1).
- **SSH agent:** SSH-key items served on `SSH_AUTH_SOCK`; each use asks
  in a shell prompt naming the host, so an agent cannot sign silently.
- **Mail passwords:** an account can name a vault item instead of keeping
  its own password (a standing grant you make once).

## Security — what this protects, and what it cannot

Agents run as you. Anything an agent can do with a shell, a vault on this
machine has to assume. So:

- **Never leaked by accident:** no secret on the bus, the call plane, a
  log line, the clipboard (unless you copy), or a screenshot of Sola's own
  windows beyond what you reveal.
- **Never handed to an agent on request:** no method returns a value
  except on the private socket to Sola's own windows, and those deliver
  only into the field you picked.
- **At rest:** today's session file is the vault's key, readable by any
  process of yours. V1 decides what replaces it.
- **The limit:** a process of yours that sets out to — runs a Sola binary
  under `LD_PRELOAD`, or waits for you to type the master password — can
  still get in, as on any Linux desktop with any password manager. Real
  separation would be agents running as another user; that is outside
  this design.

## Decided — V1–V7 (Joshua, 2026-10-31: as proposed)

| | Question | Decision |
|--|--|--|
| V1 | **How long an unlock lasts, and what is on disk.** Today the decrypted key sits in a file any process of yours can read. | Keep the key **in memory only**: one unlock per boot (the daemon rarely restarts), plus lock after an idle time you set and when the screen locks (on by default). The file goes. |
| V2 | **Agents and the vault.** | Agents never read a value (as the browser spec already says). In v1 they cannot trigger a fill either; that waits for D3. |
| V3 | **Who draws the right-click menu.** | The shell (P6). The other way — each app draws it with the kit — is simpler but puts the vault list in every app. |
| V4 | **Apps that are not Sola's** (Chromium, Steam, a foreign terminal). | Not in v1. Later, a key that opens the desk menu at the pointer over any window and types the value through River. |
| V5 | **Server.** Today the SDK's default, bitwarden.com (US). | bitwarden.com, bitwarden.eu or a self-hosted URL, chosen at login; one account. |
| V6 | **Fill a login that has a code: copy the code too?** (Bitwarden does.) | Yes, cleared after the clipboard time. |
| V7 | **What "everything" covers.** | Items of every kind, folders, collections, trash, attachments, history, generator, SSH keys, import / export. Send later. Account settings stay on the web vault. |

## Slices

1. **The daemon.** `sola-vaultd` from the browser's `vault/` (client,
   identity, persist, prefs, item, edit, match, TOTP, passkey signing,
   generator); `sola-vault-proto` (the socket protocol and its client, no
   SDK) for the browser, shell and kit; the browser on it, behavior
   unchanged; `svc:sola-vault` overlaid on novus. V1 applied.
2. **Vault, the app:** sign in, browse, search, record, edit, new,
   trash, folders, generator, settings. The canvas gets its boards first.
3. **The browser panel's new look** (`design/vault`) on the daemon.
4. **The desk menu:** the shell's overlay, the kit's `context_area`, Edit;
   the browser's page menu moves; Terminal, Projects, Mail adopt it.
5. **Vault in the desk menu:** tickets, insert into kit fields, the
   terminal's session, a page's fields.
6. **Further:** launcher search, the screen lock, the SSH agent, Mail
   passwords, organizations, attachments, import / export.

## As built: the contracts

- **Socket protocol:** `crates/sola-vault-proto` — the types (summaries,
  records, drafts, fill material, settings, server), `Request` / `Reply` /
  `Event`, and a blocking `Client`. JSON lines, `{"id","body"}`; additive
  only.
- **Unlock (V1):** the daemon keeps on disk only what Bitwarden's clients
  keep for a locked account — the email, server, KDF and salt, tokens, and
  the user key *as the master key wraps it* (age-encrypted with the desk key
  besides). The decrypted user key lives in memory only. After a restart the
  vault is signed in and locked; one master password unlocks it, with no
  round trip. The browser's `vault-session.json` is migrated without its
  `user_key_b64` and deleted.
- **Bus:** `VaultState` (sticky), `DeskMenuOpen` (app → shell),
  `DeskMenuPick` (shell → app), in `sola-bus` topics. A new topic needs the
  bus host from the same build, so the first apply moves `pkg:sola`.
- **The desk menu in a field:** the kit's text input opens the menu on a
  right-click itself (it knows editable, secret, selection) and applies the
  pick itself — Edit, or a vault value it redeems — so every kit app's fields
  get it without app code. An app adds its own rows for its own surfaces
  (a page, a terminal) with `sola_kit::desk_menu`. No in-window fallback:
  with the shell away, a right-click does nothing.

## Open technical questions

- The shell's menu overlay needs submenus and the pointer's position
  over another app's window (the app's click point plus the window's frame
  the shell already places, or River's pointer).
- A kit app keeps its field's focus while the shell's overlay has the
  keyboard; the redeemed value must land in that field after the overlay
  closes (the Edit menu already relies on this).
- Peer checks on NixOS (`/opt/sola/bin`) and in development
  (`cargo make run`) need their own allowlist.
- The SDK's attachment and import support at the pinned rev.
