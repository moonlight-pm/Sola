# Open questions

Unresolved **design forks**. Not the implementation backlog (that lives in
[`roadmap.md`](roadmap.md) / [`capabilities.md`](capabilities.md) / plans).

Priority tags: **P0** blocks current work · **P1** near-term · **P2** later.

**Agents:** If work depends on a row under
[Decision points (ask human)](#decision-points-ask-human), **stop and ask**.
Do not invent product policy.

Progress model: [`progress-model.md`](progress-model.md).

---

## Decision points (ask human)

### D1 — Permission fan-out when multiple agents attach — **closed (2026-08-28)**

**Answered:** `crates/sola-agent` is retired (unused iced ACP/Grok-leader GUI).
Coding agent work is Workspaces (`grok` CLI in PTYs). Informational named
sessions are **Bots** (2026-09-15 freeze): daemon owns `grok agent stdio`;
UIs are dialog viewers, not that GUI. Do not rebuild `sola-agent`. Permission
fan-out for a multi-client ACP *coding* GUI is moot; Bots yolo all tools.

**Related:** `agent` capability (retired); historical ACP freezes.

---

### D3 — Which call-plane methods need a human confirm (P1)

**Context:** `sola-call` is desk-equivalent privilege (local-user `0600` socket, same as the bus). Desk-equivalent methods may want a confirm gate (close app, input, Projects tab spawn).

**Ask:**

1. Which owners/methods require a prompt in v1+?  
2. Who owns the prompt (shell, a dedicated surface, the calling agent)?

**Until decided:** do not invent confirm policy. Every live method is as privileged as the socket.

**Related:** [call-plane freeze](specs/2026-08-13-sola-call-plane-design.md); `call` capability.

---

### D5 — sola-design board origin: `file://` or a local http origin (P2) — **closed (2026-10-12)**

**Answered:** serve boards over a loopback http origin, **project root as `/`**, one `*.localhost` host per board (per-board zoom → per-board raster scale).

**Context:** Boards load from `file://`. That breaks root-absolute URLs
(`/fonts/x.woff2` → filesystem root), fetch/XHR of local files, ES module
scripts, and iframe `contentDocument` (Exit field report, 2026-10-12). Boards
raster through page zoom, which Chromium keeps **per host**; every `file://`
board shares one host, so a canvas has one raster scale and one board too big
for 2× (Exit's 6800 px sheets) drops the whole canvas to 1×.

**Option:** Design serves each project over a loopback http server
(`127.0.0.1`, random port, GET only, files under the project root) and loads
each board from its own `*.localhost` host
(`http://<board>.<canvas>.localhost:<port>/design/<canvas>/page.html`).
Fixes the file:// class, gives each board its own zoom (per-board scale), and
lets boards link product assets the way the app does. Cost: an http server in
Design; project files readable by any local process that finds the port; a
per-canvas web root would still be needed for `/fonts/…`-style product URLs.
Alternative for the scale half only: tile the plane's textures past 8192 px.

**Ask:**

1. Serve boards over a loopback http origin, or keep `file://`?
2. If served: project root as `/`, or a per-canvas `root` in `canvas.toml`?

**Related:** [sola-design freeze](specs/2026-10-12-sola-design-design.md); `design` capability.

---

### D7 — Bots HTTP API exposure (P0) — **decided (2026-10-28)**

**Context (found 2026-10-19, writing the manual):** `sola-botsd` bound
`0.0.0.0:27419` with a bearer token that was a constant in public source,
so anyone on the LAN who read the source could drive bot sessions, which
run their AI with every tool approved in `~/Bots/<slug>/`.

**Decided (Joshua, 2026-10-28):** "If its local why not a socket file —
we'll use a cloudflare tunnel later for remote access." The API is HTTP on
the Unix socket `$XDG_RUNTIME_DIR/sola-bots` (mode 0600), with no TCP port
and no token; the token constant is gone from source. Remote access (the
iPhone app) comes later through a Cloudflare tunnel to the socket, which
must bring its own sign-in (Cloudflare Access) — decide that when the
tunnel is built. Question 3 (always running or opt-in) is open again only
if the tunnel needs it; on Oath the daemon needs a `svc:sola-botsd`.

**Related:** `bots` capability; [process manager](manual/process-manager.md);
[solactl § Bots](manual/solactl.md#bots-solactl-bots).

---

### D8 — Agents on another computer driving Projects (P2)

**Context:** A project on another computer works as one here, but an agent
running in its tab cannot drive this Projects: `solactl projects` there
talks to that computer's own call plane (if any). The link could forward
this computer's call socket there too (`SOLA_CALL_PATH` on the remote
panes, as the hook socket is forwarded), so `solactl projects session.new`,
`pane.send`, `whoami` etc. from there reach this Projects. But the call
plane is desk-equivalent privilege (D3): every owner (browser, shell,
design…) would be reachable by anything running as that user on that
computer.

**Ask:** forward the call plane to remote tabs? Options: no (as built);
forward it whole; forward a Projects-only proxy (only owner `projects`).

---

### D9 — Unpackaged apps in the launcher on Oath (P1) — **answered (2026-10-30)**

**Answer (Joshua, 2026-10-30):** "lets get rid of arcade (it remains in git
history), lets bring in scope, preview, monitor"; Kit: "i'm not sure we need
the app anymore -- we use the appearance settings and tweaking a bunch of
padding and whatnot is just not something a user wants to bother with. Thus
we can also get rid of the state that kit app would have saved". Arcade and
the Kit storybook (with its `CustomTheme` presets) are removed; Monitor,
Preview and Scope ship in `pkg:sola`. Bots and Install wait.

**Context:** Kit, Monitor, Preview, Arcade and Scope are launcher
built-ins (Spotify was one; `sola-spotify` is retired on `next` since
2026-10-30) (`crates/sola-shell/src/builtins.rs`), but no Oath package carries
them (`crates/sola-make/src/oath/packages.rs`). On Oath, choosing one fails
with a launch error. *Found 2026-10-29 by the novus manual pass over `next` 30a43224 (read in code).*

**Ask:** hide them from the launcher on Oath, or package them (which)?

**Related:** `shell-launcher`, `oath-platform` capabilities.

---

### D10 — A Bots service on Oath (P2)

**Context:** Oath's desk profile has no `svc:sola-botsd` (Oath repo,
`crates/oath-core/seed/desk.json`), so the `bots` owner, `solactl bots` and
the Bots window have nothing to talk to on Oath. *Found 2026-10-29 by the novus manual pass over `next` 30a43224 (read in code).*

**Ask:** add `svc:sola-botsd` to the desk profile (an Oath change), or leave
Bots off Oath?

**Related:** `bots` capability; [D7](#d7--bots-http-api-exposure-p0).

---

### D11 — What Quit Sola means on Oath (P1) — **answered (2026-10-29)**

**Answer (Joshua, 2026-10-29):** "the idea here is that it would just drop
you to a tty if that is possible". Not built: Oath runs no login on a VT
(only `serial-login`, idle without a UART), and Oath has no PAM and no
passwords (`/etc/shadow` fields empty), so a TTY there needs an Oath console
login with a credential — the same question as a lock screen (D12).

**Context:** After **Quit Sola** on Oath, `sola-shell`, `sola-river` and
`sola-session` exit cleanly and stay down: a clean exit is not restarted
(on-failure / always policies). `sola-bus`, `sola-call` and `sola-kvm` keep
running. Nothing brings a desktop back short of a reboot. *Found 2026-10-29 by the novus manual pass over `next` 30a43224 (read in code).*

**Ask:** intended? If not: Quit Sola restarts the desk, logs out to a
console, is hidden on Oath, or something else?

**Related:** `oath-platform`, `shell-menubar` capabilities;
[manual/sola-shell.md](manual/sola-shell.md#flower-menu).

---

### D12 — The public manual says the desk has no login and no lock screen (P1) — **answered (2026-10-29)**

**Answer (Joshua, 2026-10-29):** make it true that a secure setup exists, and
say so — "I'd like to resolve these so that our docs can honestly describe a
secure setup, that exists" (network and physical access are trusted for him,
so it stays optional). Built: the [secure access
spec](specs/2026-10-29-secure-access-design.md); `next`'s manual has a
Security chapter. Master's Introduction ("Starting a session") still says no
login and no lock screen: reword it at the merge to "by default", with a link
to Security.

**Context:** The manual's Introduction ("Starting a session"), published at
sola.moonlight.pm/manual, says the desk starts at boot with no login and no
lock screen. *Found 2026-10-29 by the novus manual pass over `next` 30a43224 (read in code).*

**Ask:** fine to say publicly, or reword / drop it (and is a lock screen
wanted)?

---

### D13 — Bot refusals in the compositor can be bypassed (P1)

**Context:** The `SOLA_BOT` refusals on compositor input can be bypassed
through `solactl sola-river input.*` and over MCP. The browser's rules follow
the calling owner; these don't. *Found 2026-10-29 by the novus manual pass over `next` 30a43224 (read in code).*

**Ask:** close the bypass (refuse by caller everywhere), or accept that
anything with the call socket is desk-equivalent (D3)?

**Related:** [D3](#d3--which-call-plane-methods-need-a-human-confirm-p1), `call`, `bots` capabilities.

---

### D14 — What `pkg:sola` ships on Oath (P2)

**Context:** *Found 2026-10-29 by the novus manual pass over `next` 30a43224 (read in code).* `pkg:sola` carries none of:

- `.desktop` files, so `xdg-open` does not route to Sola apps
  (`sola-browser.desktop` also hardcodes `Exec=/opt/sola/bin/sola-browser`);
- `cloudflared` for Design's share links;
- log rotation: nothing rotates `sola.log` on Oath (only the `sola` process
  manager did).

**Ask:** which of these should Oath get, and from which package (Sola's or
Oath's)?

**Answered 2026-10-31 (opening links and files):** Sola owns it (Joshua:
"sola owns it.."). `pkg:sola` ships Sola's own opener as `/bin/xdg-open`,
not xdg-utils and not `.desktop` files; the rest of the routing is the
handler registry in the decision log (Link and file handlers).
**Answered 2026-11-03 (`cloudflared`):** its own package from Oath,
`pkg:cloudflared`, not part of `pkg:sola`; on novus, Design's tunnel runs
through it. Log rotation is still open.

**Related:** `oath-platform`, `install-make`, `kvm`, `design` capabilities.

---

### D15 — Google Calendar's client on Oath (P2) — **answered (2026-11-04)**

**Context:** the client was in the source until 2026-09-09, when GitHub's push
protection rejected it and it was taken out, so every build after had none:
novus's Google account last synced 2026-09-08, and sign-in needed
`SOLA_GOOGLE_CALENDAR_CLIENT_ID` / `_SECRET` in the environment.

**Answer (Joshua, 2026-11-04: "we need this to work out of the box for other
sola users"; "published it, go ahead with the four items"):** Sola ships its own
client. `cargo make` compiles it in from `~/.config/oath/sola-google-client.env`
(crate `sola-google-client`), never from git, and the release plan exports it. A
publish refuses without it; a clone without it has no Google sign-in and may set
the environment variables at run time. Google calls a desktop client public, so
the programs and the public plan carrying it is expected.

**Google's side:** project `sola-508017`, published (in production, unverified:
a warning, and 100 new users), with the two narrow scopes on Data Access and the
home page, `/privacy` and `moonlight.pm` on Branding. `/privacy` is live (2026-11-09).
`moonlight.pm` is proven in Search Console (2026-11-09, auto-verified by the
`google-site-verification` TXT record it already had; keep that record).
The demo video is on YouTube, unlisted. Branding is verified and published, and
the verification request went in on 2026-11-09: Data Access reads "under review".
Google answers by email. On 2026-11-11 its Trust and Safety team passed homepage,
branding, app functionality and minimum scopes, and failed the privacy policy: it
did not say which Google data Sola accesses, how it uses it, or how it protects
it, and Google asked about AI. `/privacy` now has a section for each
(`#data-accessed`, `#data-use`, `#data-protection`, `#ai`; live 2026-11-11). The
console has nothing to resubmit: the review goes on when the email thread is
answered.

**Related:** `calendar` capability.

---

### D16 — Projects sessions on any computer (P1) — **answered (2026-10-30)**

**Context:** Joshua (2026-10-30): connect "from any machine to any machine and
connect to any ongoing session"; closing a window is not ending a session; the
tab shows the host ("Oath | novus"); in-window tabs go. Draft:
[sessions anywhere](specs/2026-10-30-projects-sessions-anywhere-design.md).

**Answer (Joshua, 2026-10-30):** the label in kit parts with the host in its
prompt color; sessions restart by themselves after a reboot; size follows
the desk in use with scaled mirrors (agents' full screen too); the rest as
proposed (End Session only, splits dropped, islands on every desk with the
session open). Recorded in the spec's D16.

**Related:** `projects` capability; D8.

---

### D17 — Vault: the desk's Bitwarden client, its service, the right-click menu (P1) — **answered (2026-10-31)**

**Context:** Joshua (2026-10-31) asked for a Bitwarden client of Sola's own
("Vault" / `sola-vault`), the browser's vault logic moved into a service, the
vault reaching other apps, and a desktop-level right-click menu with standard
items (pasting from vault items, copy) and apps' own. Draft:
[vault](specs/2026-10-31-sola-vault-design.md). Found while drafting: the
browser's `vault-session.json` holds the decrypted user key, encrypted with
`~/.config/sola/key` — both readable by any process running as you, agents
included.

**Ask:** the spec's V1–V7 — how long an unlock lasts and what stays on disk;
agents and the vault; who draws the right-click menu; apps that are not
Sola's; the server; copying the code on fill; what "everything" covers.

**Answer (Joshua, 2026-10-31):** "All your recommendations sound good" —
V1–V7 as proposed (memory-only unlock, locking on idle and the screen lock;
agents never read a value; the shell draws the menu; foreign apps later;
US / EU / self-hosted; the code copied on fill; everything but Send and
account settings). Recorded in the spec.

**Related:** `vault`, `desk-menu`, `browser` capabilities; D3.

---

### D18 — The new Oath app's package (P2) — **decided (2026-11-05)**

**Context:** the Oath app shipped in `pkg:sola` (`crates/sola-make/src/oath/packages.rs`), and moving that package restarts the desk, so every overlay of the app would too. A package of its own moves alone, but needs an Oath pin in the profile.

**Decided (Joshua, 2026-11-05):** "it needs to be stand-alone outside of 'sola'". The new app is `pkg:sola-oath` (branch `sola-oath`), out of `pkg:sola`. A release carries it once Oath's desk profile names it.

**Related:** `oath-platform` capability; [CURRENT](../CURRENT.md).

---

## Open technical questions

### T1 — Agent pin UI surface — **closed (2026-08-28)**

Moot: `crates/sola-agent` deleted. Overlay/pin chrome went with the crate.

### T2 — CEF default vs WPE — **closed (2026-08-11)**

WPE content-plane dogfood failed (`naturalethic/browser`). Product browser is
**CEF-only** in single crate `sola-browser`. Reopen only if a future Mesa/dma-buf
comparison warrants a second engine.

### T3 — D-Bus replacement on Sola Bus (P2)

D-Bus is deprecated as a requirement (decision log 2026-10-18). The target
is Sola's own replacement on Sola Bus / sola-call. Open: its shape — what
replaces each legacy use, and how Sola reaches services and apps that only
speak D-Bus (BlueZ, logind / systemd, other players' MPRIS, Chromium's
`libdbus`). Inventory: [architecture § D-Bus](architecture.md#d-bus-deprecated).

### D4 — sola-workspaces product forks (P2) — **closed (2026-10-08)**

**Context:** Freeze is in; persist + spawn landed. Call plane owns fail-if-down
(was old D3.3). The last fork was Claude hooks.

**Ask:**

1. ~~Display name / window title~~ **decided 2026-08-14 / amended 2026-08-18:** crate / app id `sola-workspaces`; window **Workspaces**; owner `workspaces` (`solactl workspaces`); tmux `sola-ws` / `sws-`; config `~/.config/sola/workspaces/`. **Renamed 2026-10-21** (branch `next`) to free "workspace" for desktop workspaces ([Next N5](specs/2026-10-21-next-tab-bar-design.md)): crate / app id `sola-projects`, window **Projects**, owner `projects` (`solactl projects`, no alias), a workspace is now a tab (`tab.*`), config `~/.config/sola/projects/`; tmux names unchanged. **Renamed 2026-10-31:** a tab is a session (`session.list|new|end|select|set|exec`, `--session`, JSON `sessions` / `session` / `session_name`; the agent's own id `agent_session`), and `sola-session`'s owner is `desk` (`solactl desk`).  
2. ~~Default worktree base~~ **decided 2026-08-13:** `<project-root>/.worktrees/<name>`.  
3. ~~If CLI runs and the app is down~~ **decided via call plane:** fail, do not launch.  
4. ~~Claude in v1~~ **decided 2026-10-08:** first-class, same path as Codex.
   `--agent claude` on spawn/exec. Hooks merge into `~/.claude/settings.json`
   (permissions, plugins, and other hooks stay). Rail rollup. Quiet `×N`
   counts `compact_boundary` in the session jsonl. A new tmux after reboot
   runs `claude --resume <id>` when that transcript is still on disk.
   OpenCode stays presence-only.

**Closed.** OpenCode is presence-only by design, not an open fork.

**Related:** `projects` capability (was `workspaces`);
[`specs/2026-08-13-sola-agent-terminal-design.md`](specs/2026-08-13-sola-agent-terminal-design.md).

---

### D6 — sola-design publish: where shared canvases live (P2) — **closed (2026-10-12)**

**Answered:** **live**, not a snapshot. Design is the web server: a thin
server that limits each link to its canvas, piped to `design.moonlight.pm`
by a Cloudflare Tunnel. Every canvas gets a link automatically, rotatable.

**Context:** the human asked to share canvases on the internet behind an
unguessable path. A snapshot on R2 + a Worker (links that outlive the
machine) was proposed; the human chose live sharing from Design.

**As built:** 128-bit base32 token per canvas (`share.json`); the link
reaches only that canvas's folder, shared `design/` files, its `root`, and
the project files its boards load (real paths; no dotfiles); a viewer page
lays the canvas out and reloads on change. Remotely managed tunnel
`sola-design` (ingress → `127.0.0.1:47820`), proxied CNAME `design`; Design
runs `cloudflared` while open. Links work only while Design runs.

---

## Decision log

| Date | ID | Decision | Where recorded |
|------|-----|----------|----------------|
| 2026-11-11 | A sign-in comes back to its app | A Browser page a web app's page sent to another site (a clicked link, an off-site popup) becomes a window of that app, in place, once it is back on the app's site; while away it stays a Browser page with its address in sight. Chosen over keeping every off-site link in the app's window (a sign-in link cannot be told from another) (Joshua, on an SSO sign-in that ended as a loose Browser row: "yeah make that fix") | [web apps](manual/web-apps.md#the-apps-window) |
| 2026-11-11 | Adopt Workspace | A window, not an app, picks its sign-in: **Adopt Workspace** in a window's menu, there only while its page runs another profile than the workspace showing's, gives that window the showing workspace's profile; it keeps it wherever it goes. For web apps and Browser pages (a pinned page is the only Browser page that can differ). ⌘N keeps the sign-in of the window it was pressed in; the launcher's window is on the app's own. No second app for the same site (Joshua: "i need to login to this app as separate user on a different workspace"; "this needs to be specific to each window -- i don't want to create extra apps for the same site"; "lets make the item 'Adopt Workspace' -- and its only present if the app is not connected to the current workspace profile"; "I'd rather ⌘N keep the sign-in of the window you pressed it in") | [web apps](manual/web-apps.md#another-workspaces-sign-in) |
| 2026-11-10 | Bus state | A sticky record is current state with one owner and a reader other than its writer; private state goes to its owner's file; replay is lossless (a client gets everything it asked for, or its connection is closed and it subscribes again); a record goes when its writer ends for good, and a writer that restarts keeps its records until it comes back and sends them again (Joshua, after the bus review: "do A+B+D --- whatever those address are the answers to your decisions"; chose "kept across restart") | [bus state spec](specs/2026-11-10-bus-state-design.md) |
| 2026-11-10 | Design opens, never makes | Design's projects are the folders of `~/Projects` with canvases in `design/`, found by scanning; Design does not create or delete projects or canvases — agents make them (Joshua: "design doesn't 'create' design folders .. like projects, it will scan the Projects/<...>/design folders which can be opened for viewing... only agents will create those folders"; "Its merely a window on them"). A new Design window lists them to open, like Projects' ("When i open a new design window, i'm then presented (like projects) the list of project/design i can open to view"). Walks back 2026-10-12's own hand-added project list. | [freeze § Projects](specs/2026-10-12-sola-design-design.md), `docs/manual/sola-design.md`, capabilities `design` |
| 2026-11-09 | KVM is its own package | `pkg:sola-kvm` holds `sola-kvm` and `sola-kvm-view`, out of the core: its restart does not take the session down, and a Remote Desktop change should not restart the desk (Joshua: "go ahead and split out sola-kvm"). The Oath session agreed the same day, with no Oath change: the core needs the new package until every machine has it, the new package never needs the core, heads go out `pkg:sola-kvm` first, then Oath pins both in `desk`. | `crates/sola-make/src/oath/packages.rs` |
| 2026-11-10 | The Oath app adds; a head is current | **Add** on a package on the origin only runs `sudo oath add` at once, with no second step, and says afterwards what came with it (Joshua: "add the ability to add packages (oath add) ... i could tap a button on 'on origin only' package, and add it"; chose "Add at once"). A package that runs its signed head reads **Current**, as the release's build does; "Local build" means neither (Joshua chose "Current"). Walks back 2026-11-03's "it only looks" for Add only. | capabilities `oath-platform`, [applications](manual/applications.md) |
| 2026-11-09 | Remote Desktop is a mode of KVM | The app is called Remote Desktop; its command is `sola-kvm view <host>`, not a new product (Joshua: "this would just be another mode for kvm"; "let's called it 'Remote Desktop'"); our own agent on the Mac rather than a VNC client; while its window has focus Super goes to the Mac as Cmd, and Ctrl+Alt+Esc or clicking another window gives Sola its shortcuts back. | capabilities `kvm`, [Remote Desktop](specs/2026-11-09-sola-kvm-view-design.md) |
| 2026-11-08 | Quit is back | Super+Q quits the focused app, every window at once, and each app's menu has Quit beside Close Window (Joshua: "i'd actually like that back ... we're 'window' based but that doesn't really work that way .. i want to be able to quit sola-design for example, without closing all its windows"). Walks back 2026-11-02's No Quit. | [shell manual](manual/sola-shell.md) |
| 2026-11-08 | Groups fold | A click on a group's name in the bar folds it to one line, per workspace and saved; folded, it shows its count, its loudest mark, and reads bright while it holds the focus. Walks back 2026-10-31's "Groups don't fold". | [shell manual](manual/sola-shell.md), [tab bar spec](specs/2026-10-21-next-tab-bar-design.md) |
| 2026-11-08 | Super+V with a picture | With a picture and no text on the clipboard, Super+V in a Projects session sends the agent Ctrl+V, so it reads the image itself; a shell gets nothing (Joshua: "i meant ctrl+v -- but yeah make that super+v .. not sure why it is different"). | [solactl manual](manual/solactl.md) |
| 2026-11-08 | Host on a Projects tab | A session on this computer does not name it on its row or in its frame; a session on another computer keeps its chip. The window title keeps the host, which the bar parses by (Joshua: "lets not put the machine name if the session is the current machine"). Walks back 2026-10-30's "tab should always show the host even if local". | [shell manual](manual/sola-shell.md), [sessions anywhere](specs/2026-10-30-projects-sessions-anywhere-design.md) |
| 2026-11-08 | Mail account form | The inbox has no Send from list: its checklist read Wicket's `/api/auth/me`, which other mail servers don't have (Joshua: "its not standard and wicket only, so toss it"). Replaces the inbox clause of 2026-09-04; added accounts keep typed addresses. An account's Save and Discard sit on its own row ("totally confusing to have it in the middle of the form") | [mail freeze](specs/2026-07-27-sola-mail-kit-design.md), `docs/manual/sola-mail.md` |
| 2026-11-05 | A hidden Design tab reloads | The canvas of a Design window behind other tabs closes after 10 idle minutes like any off-screen canvas, and reloads when the window comes up, with its view kept. The active window's stays. It frees VRAM and the CEF memory behind it (Joshua: "the reload is fine") | `docs/manual/sola-design.md`, `PERFORMANCE.md` § VRAM |
| 2026-11-04 | GPU and VRAM meters | The top row reads CPU GPU VRAM MEM: the card's processor and its memory are separate meters, each with its own panel, the GPU panel listing apps by processor and the VRAM panel by memory (Joshua: "i'd like meters for both gpu ram and processor") | `docs/manual/sola-shell.md` § CPU, GPU, VRAM, MEM, RX and TX |
| 2026-11-03 | The new Oath app's first version | Strata and Weight for the map, Flow for the tree; it only looks, with no on/off, apply or undo; this machine only, other machines later; Upstream is its own column; the audit report may be public on the origin (Joshua: "1. Yes 2. Yes 3. Not yet. I'm going to work on oath multi-computer visualization and configuration type stuff later") | `design/oath/`, CURRENT |
| 2026-11-03 | Screenshot keys swapped | F13 takes a selection, F14 the whole screen; F15 stays the focused window (Joshua: "swap the F13 and F14 keybindings") | `docs/manual/sola-shell.md` § Screenshots |
| 2026-11-03 | Agent islands go to the window | Clicking an island brings up its session's window; no card, and nothing is answered from the row. A hover × dismisses it until the session next asks or finishes (Joshua: "clicking them should just take me to the window.  Let's give them a 'hover close' just to dismiss them as well") | [agent islands](specs/2026-10-23-agent-islands-design.md) |
| 2026-11-02 | Pinned windows replace Common | No Common workspace, island or Super+0: a window is pinned to every workspace, from a toggle in the Window menu, for every window (Joshua: "lets get rid of the 0 Common island .. and we don't switch to the workspace -- in fact it doesn't have to be a workspace at all. Instead, we simply pin any window we want on all workspaces ... lets just make this a menu option toggle for now for all windows"). Chosen with it: unpinning leaves the window on the workspace showing; closing works from any workspace; Super+Shift+N sends it there and unpins it; a pinned page keeps its profile; a pinned window has one zone, as before. Supersedes the Common row below. | [shell](manual/sola-shell.md#pinned-windows) |
| 2026-11-02 | New session's workspace | A session started by a session opens on its parent's workspace, not the one showing (Joshua: "When a projects session starts a new session, the window for the new session appears in whatever workspace i'm in... The new session should instead appear in the same workspace as the one that created it"). In the zone last used there, behind, without moving you; `--select` goes there. A parent on Common, or with no window here, starts it where you are. | [shell manual](manual/sola-shell.md#zones); `Topic::TabOpener` in [bus](manual/bus.md) |
| 2026-11-02 | Common workspace | A workspace 0, **Common**, always first, whose windows are on every workspace, with a row in each one's group there; they can be removed only from Common (Joshua: "any window that is in it will be present in any of the other workspaces"; "it *cannot be removed* -- common windows can only be removed from the common workspace"). One zone everywhere; order and what's on top per workspace, saved across restarts ("i'd rather deal with the fiddly part now"). Pages on Common run a new Common profile and reload signed out; web apps keep theirs. Super+0 and Super+Shift+0 are Common's; Paint's Fit and Design's Fit All and Actual Size move. Super+Tab passes over it until its dot is lit. Sola blocks its own close controls; a foreign app's own button, a page closing itself and a crash still end the window. | [Next tab bar](specs/2026-10-21-next-tab-bar-design.md); [shell manual](manual/sola-shell.md#common) |
| 2026-11-02 | Super as Meta | Editor only (Joshua: "Super+W should only be 'copy' in the editor ... all of the 'meta' (SUPER+) keys in editor are editor only. They are not desk wide"). Everywhere else Super+W is Close Window, Terminal and Design included; Terminal's ⌘⇧W stays Close Pane. Supersedes ⌘W unbound in the terminal pane-splitting spec of 2026-06-16. | [shell manual](manual/sola-shell.md) |
| 2026-11-02 | No Quit | Super+Q and every app's Quit item go; Close Window takes their place (Joshua: "The system is 'window-based', not app based, however the underlying mechanism works."). | [shell manual](manual/sola-shell.md) |
| 2026-11-02 | Web app windows and one-window groups | Every web app opens several windows, with no setting (Joshua: "remove the configuration"); a new window starts at the start page, and the launcher opens one where it used to raise. For every app, one window on a workspace shows no group (Joshua: "make that universal"). A link opening its own window goes to the opener's app on its apex, else the web app on that site, else a Browser page; a link from outside the browser goes to the web app on its site too (Joshua: "from mail or anywhere"); the launcher's Browser field and Open in Browser stay Browser pages | `docs/manual/web-apps.md` § Several windows, `docs/manual/sola-shell.md` § the bar |
| 2026-11-02 | Screenshot and send keys | Screenshots on bare F13, F14, F15; Send Window to Workspace on Super+Shift+1…9 (Joshua). A system key needs Super, except F13 to F19, which no app types | `docs/manual/sola-shell.md` § Screenshots |
| 2026-11-02 | Workspace islands | A setting to try another way (Joshua: "An alternative layout for workspace widgets"): each workspace an island right of the flower, no switcher in the bar; a dot per island for Super+Tab, a drag to reorder, the showing one's click for its menu. In Appearance while it is one setting. Super+Tab honors the switches only in this layout: turning it off is exactly as before | `docs/manual/sola-shell.md` § Workspaces as islands |
| 2026-11-01 | Light mode | Settings → Appearance → Light or dark is global: Light gives the desk and every Sola window a light theme, and pages are told light; the light theme follows the picture as the dark one does (Joshua: "lets add dark/light mode as a global setting (in settings appearance) -- and lets also actually have a light theme -- This theme needs to be responsive to the background image too"; "just give it your best first effort", no Design pass) | [Background spec § Light mode](specs/2026-10-24-background-design.md#light-mode-2026-11-01), theme manual, capabilities `theme-bus`, `browser` |
| 2026-10-31 | Bar app groups and keys | The apps that open a window per document (Browser pages, Projects, Terminal, Paint, Design) group in the bar under the app's header, a group even with one window; every other window is a loose row (Joshua: "these aren't ad-hoc groups, but group by app … 'Stand alone' apps like mail or settings, or any wrapped browser -- these all are ungrouped"). Groups don't fold; Projects is one flat group, a new session after its project's rows. Groups move by their header, rows within their group, per workspace. Look (2026-11-01): a caption — the app's name in small capitals, no icon, the windows a step in (way A of `design/bar-groups/`); every loose row above every group ("all 'loose' (singleton) apps go at the top"). Keys: Super+Tab steps workspaces and Super+CapsLock the windows of the zone under the pointer, bar order (swapped 2026-11-01: "so super tab is workspaces, and super capslock is 'window here'"; first every window whose place covered the pointer), and Caps Lock alone does nothing (2026-11-01: "caps lock by itself should be a no-op for now"); Super+\` the focused app's windows on this workspace, bar order (Joshua: "no more walking down the sidebar which i find useless") | [Next spec § App groups and keys](specs/2026-10-21-next-tab-bar-design.md), shell manual, capabilities `shell-desks`, `keybindings` |
| 2026-11-04 | D15 Google client | Sola ships its own, compiled in by `cargo make` from a file outside git; the programs and the release plan carry it (Joshua: "we need this to work out of the box for other sola users") | capabilities `calendar`; [D15](#d15--google-calendars-client-on-oath-p2--answered-2026-11-04) |
| 2026-10-31 | Link and file handlers | Custom, with backward compatibility (Joshua: "Since i run almost only sola apps -- i wonder if we couldn't do something custom"; "we put this in sola-kit -- any app can register itself for any custom protocol"). Apps claim **link schemes and file types** through the kit; a claim never changes the default (a newcomer is an alternative in Settings); an app may ask to become the default, and only the user's confirm changes it; defaults are the user's, in Settings. Sola's opener is `/bin/xdg-open` on Oath (D14); unclaimed schemes go to `.desktop` handlers and `mimeapps.list`. Spec not written. | CURRENT Now, D14 |
| 2026-10-31 | Session names | "Session" in `solactl` means a Projects session. `sola-session`'s call owner is **`desk`** (`solactl desk launch\|close`, MCP `desk_*`; was `session`; Joshua: "I like desk"). Projects' `tab.*` are **`session.*`** as the sessions-anywhere spec named them (`session.list`, `session.new`, `session.end`, `session.set`, plus `session.select`, `session.exec`), `--tab/-t` is `--session/-s`, JSON `sessions` / `session` / `session_name`, the agent's own session id `agent_session`. No aliases. | manual solactl / session / call, CURRENT Locked models, capabilities `projects` / `session-mgr` |
| 2026-10-31 | Apps without a window | An app that isn't Sola's runs only while it has a window on the desk (Joshua: "make sure we never have steam and all its processes running without some desktop representation … if we close the window or SUPER+Q or SUPER+W (and the last steam window is closed) that it completely clear out all steam processes as if we had quit steam from its own menus"). Asked and answered: the rule covers **every app that isn't Sola's**, not Steam alone (Sola has no tray, so any app that hides in one would run on unseen); **a game's window keeps Steam open** (closing Steam's own window mid-game leaves it for the game; it quits when the game closes). Quitting is the app's own quit command first (`steam -shutdown`), then SIGTERM and SIGKILL to every process carrying its launch tag. Not a tray (D-Bus, T3), not `STEAM_FRAME_FORCE_CLOSE` (gone from the client), no Oath change. | manual `session.md`, `sola-shell.md`; capabilities `session-mgr` |
| 2026-10-30 | Zone layout | Z1–Z16: Settings draws the screen; rows × columns of equal cells, changed with − / + (− asks before deleting the zones it cuts; + keeps cells for now); a zone is a rectangle of cells (drag, or click one) with one key, Super added; Full, Cinema and Floating always exist (keys changeable, not deletable); a new window comes up in the zone last looked at (a workspace's first, and one opened while Cinema is up, in Full); zones shown by key and drawing, no names; edit a zone's cells keeping its key; no toggles (Cinema is a zone; send a window elsewhere to resize it); Cinema completely opaque; an app's fullscreen goes to Cinema and back; keys belong to a keybinding system; several screens parked. P1 (overlap rule) proposed, built. | [zone layout spec](specs/2026-10-30-zone-layout-design.md) |
| 2026-10-30 | Keybindings | K1–K7: one keybinding system, every named action editable (not plain movement and typing keys), no conflicts, Settings → Keybindings; no new process (the shell keeps registering and acting); shared menus (Copy, Paste, Quit, Window) come from the kit; system keys include Super; the two key bugs fixed by it; zone keys set on the Zones page. Where the code goes was left to the implementer: sola-core (the key table) and sola-kit (`keys`), no new crate. | [keybindings spec](specs/2026-10-30-keybindings-design.md) |
| 2026-10-30 | D2 sola-kvm input ACL | Moot: the server captures through the compositor (layer-shell strip, exclusive keyboard, pointer lock, relative motion, shortcuts inhibitor) and reads no `/dev/input`, so there is no ACL or udev rule to install (Oath R44; Joshua chose "All of it"). Pairing is by public key and neither side starts unpaired | capabilities `kvm`, KVM manual, KVM and secure-access specs |
| 2026-10-30 | D9 launcher apps on Oath | Arcade and the Kit storybook removed (Kit's `CustomTheme` presets with it: Settings → Appearance supersedes them; the 2026-08-15 storybook-page rule goes too); Monitor, Preview and Scope ship in `pkg:sola`; Bots and Install later | open-questions D9, capabilities, AGENTS |
| 2026-10-30 | Spotify | `sola-spotify` removed on `next`; Spotify runs as a web app | capabilities `spotify` |
| 2026-10-28 | Projects are ~/Projects folders | Projects keeps no list of projects (Joshua: "We don't need a 'Drop Project' anymore. Closing the window does that basically. Also we don't need to keep a list of projects. Projects are jut whatever is in the ~/Projects directory"): the catalog holds only open projects; closing a window ends its sessions and forgets it. Asked: a reopened project starts on its root tab only; startup scripts live in a settings file by folder; with nothing open Projects quits. A remote project's tab shows its folder alone, its frame "<folder> on <host>". | capabilities `projects`, manual `solactl.md` |
| 2026-10-28 | Projects on another computer | A project may be a folder in an ssh host's `~/Projects` (Joshua: "When i open projects through launcher, there should be a 'remote' option … type in a hostname, and it should remember recently used … ssh there to get a list of projects … open sessions over ssh to tmux … So the session runs on that machine"; then "remote projects should be identical in operation as local ones"). Each tab is a tmux session on that host on its own socket (`sola-ws-remote`, so a Sola there keeps its own apart); the chooser lists `~/Projects` there, not that host's Projects catalog. A link per host (`ssh -N -R`) brings its agents' hooks back, after Projects installs the same hooks there; worktrees, startup scripts, the pane CLI and resume run there through a `Machine` seam. | capabilities `projects`, manual `solactl.md`, `sola-shell.md` |
| 2026-10-28 | Themed flower | The top row's flower takes the theme as its own token, `shell-flower`, amber by default (Joshua picked it over painting the flower the accent, which would have turned it cyan on the grey theme): a picture's scheme recolors it as the accent, and the storybook's Shell page edits it. Built with it: Settings' Accent sets it too, and a grey picture keeps the amber. | capabilities `theme-bus`, manual theme / sola-shell, background spec |
| 2026-10-27 | Browser autoplay; pages off screen rest | Chromium's own autoplay policy replaces `no-user-gesture-required` (Joshua: "no reason that all 5 should be trying to play at once"): muted video plays only while in view, sound waits for a click on the page. A page whose window is off screen rests, the active one too ("make the active page rest off screen"), and under the idle fade once it is full ("let them keep running until the fade reaches full transparency"). | browser manual, compositor manual, capabilities, architecture |
| 2026-10-26 | Launcher Browser field | Picking Browser in the launcher asks where to go instead of opening a blank page (Joshua: "instead present a omni search bar, then open the browser with that"): the location bar's rules (`sola_core::omnibox`) — Enter opens the address or Kagi's top result; a search offers Kagi's results page; the browser's visits below; the page opens in a new page window. | shell manual, browser manual, capabilities |
| 2026-10-26 | App bar height | One height for an app's bar under its titlebar (Joshua: "pick a standard height here, and use where ever apps have a similar bar"): kit `style::BAR_H` = 42 (Next's mail head) — Projects and Design tab strips, Mail, Calendar, Bots, Browser, Paint, Preview, Scope, Monitor. | AGENTS, capabilities |
| 2026-10-26 | One file panel | The kit `FilePicker` is the Open / Save / Choose Folder panel for every app, as macOS has one (Joshua: "This component should be standard, like mac os does, and re-usable for apps. Same for an Open"): keys, several files, folders, replace-confirm, New Folder; web pages' file choosers use it too. | AGENTS, capabilities, manuals |
| 2026-10-26 | Launcher raise; ghost row | Picking a running app brings its window forward (Browser and Terminal open a new window); the top bar's "Opening …" toast gives way to a ghost row in the tab bar where the window will land, with a turning ring (Joshua: "replaced with a 'ghost' tab on the left where the app will go once its open… maybe some animation"). | shell manual, session manual, capabilities |
| 2026-10-26 | Mail HTML | Mail renders HTML letters itself, a **safe subset** (Joshua chose it over a locked-down CEF): html5ever into a small DOM, walked into kit prose — CSS whitespace collapsing, bold / italic / code, http / https / mailto links, headings, quotes, lists, `pre`, rules, tables as rows, pictures. Nothing runs and no stylesheet is read; hidden elements (preheaders) are dropped. Pictures in the message show; pictures on the web only after **Load Pictures**. Newsletters read as one column, not the sender's layout. | [mail kit](specs/2026-07-27-sola-mail-kit-design.md), CURRENT, capabilities, manual |
| 2026-10-26 | Mail bar | Mail's head runs across the window over the list and the letter (Joshua picked it over moving Search / New / Erase to the menus): the mailbox (a menu of them in a half tile) and its unread at the left; a search field, Erase and New Message at the right. | CURRENT, capabilities, manual |
| 2026-10-26 | Projects in ~/Projects | Every project lives in `~/Projects` (Joshua: "I actually want to stand on a convention here … All projects always go in ~/Projects"). Picking Projects in the launcher shows an interstitial: the folders, open ones marked (picking one brings up its workspace and window), others opened in the current workspace, or a new one by name (the folder made, then opened). Projects first; other apps may follow | shell manual (Launcher), `docs/specs/2026-10-21-next-tab-bar-design.md` (launcher row) |
| 2026-10-26 | Desk opacity | The windows, the bar and the top row are 90 % opaque over the background picture by default (Joshua: "make the default opacity for desktop items 90% … put the max transparency in our appearance settings ..default to 90"): Settings → Appearance → Desk → Opacity, 50–100 %; the idle fade starts from it | `docs/specs/2026-10-24-background-design.md` (the idle fade), `shell-background` |
| 2026-10-26 | Next workspace profiles | Workspace and browser profile join (Joshua): every workspace has a profile of its own, paired by the workspace's id; a profile is only a uuid, no name, so a workspace renames freely — none shared, nothing to pick, no Profiles menu; a page sent to another workspace reopens under that workspace's profile; browser titles drop the profile's name. Amends Next workspaces (2026-10-23). | Next spec § Desktop workspaces, capabilities `shell-desks` / `browser`, manual/sola-browser |
| 2026-10-23 | Next Full zone | A Full zone at the top of the bar covers the content area. Picking a Full tab leaves the halves off screen; picking a half's tab draws both halves and leaves Full off screen. A lone half keeps its half. Bar rows: lifted (on screen), full ink without the lift (its zone's shown tab, zone off screen), muted (behind). The first window on an empty workspace goes to Full. Super+KP_0 moves to Full (Fullscreen toggle retired); KP_Enter stays Cinema. Amends N2. | Next spec § Full, CURRENT, capabilities `shell-desks` |
| 2026-10-23 | Next app ports | The Sola apps that still keep their own list of documents move to a window per document (the shell's bar is the tab list), **one app at a time**. Paused before the first; candidates Paint, Design, Bots, the wrapper's popups. | CURRENT, Next plan |
| 2026-10-23 | Next grey base | The kit takes Next's dark grey base now (before the kit overhaul): surfaces and lines at 0.8× L*, text at the same L*, accent and status unchanged; a saved slate theme moves to the greys. The choosable accent still waits for the overhaul. Amends N8. | Next spec (N8), CURRENT, capabilities `theme-bus`, manual/theme |
| 2026-10-23 | Next workspaces | Board 04 as proposed (pills by the flower with names and marks, named workspaces, Super+Tab across all, a window on one workspace); each workspace picks a browser profile (several may share); a page sent to a workspace with another profile reopens there under it; N6 dropped (projects not tied to workspaces). | Next spec § Desktop workspaces, plan Phase 5 |
| 2026-10-22 | Next titlebars | Boards 01–02 picks as drawn (38 px row, unfocused menus dimmed, gap 10, 12 px corners, lit rim, black ground); dropdowns drawn by the kit in the window; foreign windows get gaps and a compositor hairline; the 28 px top band stays until the islands. | Next spec § Titlebars, plan Phase 6 |
| 2026-10-21 | Next tab bar | N1–N8: spec first; Left / Right 50 / 50 + Floating; new windows join the focused zone; the bar docked at 240 px; Projects rename (N5); browser groups dropped (N7); titlebars, gaps, islands after the bar (N8). | Next spec § Decided, CURRENT Locked models |
| 2026-10-18 | D-Bus | **D-Bus is deprecated as a requirement.** No new D-Bus dependencies; existing uses are legacy. In future Sola targets its own replacement on Sola Bus. Shape open (T3). | architecture, CURRENT locks, AGENTS, T3 |
| 2026-10-14 | sola-design annotations | Only agents edit designs. Annotations (sections, notes, exits / links, flows) are part of the design — what is not obvious, general information, flow. No human notes, comments, or feedback on the canvas. Flows are tours on the canvas, not a full-screen prototype mode. | [freeze](specs/2026-10-12-sola-design-design.md), capabilities, manual |
| 2026-10-13 | sola-design memory budget | Design holds its boards to a memory budget that **adapts** to what the machine has free (not a fixed size), if that can be done reliably. Over budget, Design first **discards idle boards** — boards nobody is looking at and no agent called lately: their page closes, their last picture stays on the canvas, and they reload from disk on the next look or call. A discard is not dropping a board (canvases are files; only in-page state such as scroll or typed text goes). Only when the boards in use still do not fit is Design **nice to the agent**: it refuses the new board / canvas with an error that says what is wrong (memory), what is live, and how to free room. (Ten agents' canvases filled 125G and froze the box; see CURRENT.) | CURRENT |
| 2026-10-13 | sola-design board pace | Boards nobody looks at hibernate: off-screen canvases sleep unless an agent is working there (then 1 fps); on the canvas on screen only boards in view paint, at 60 fps while Design has focus and 1 fps while it does not; loading, settling, and agent calls run at full rate. Frame-transport work stays parked. | freeze, capabilities, architecture, manual, CURRENT |
| 2026-10-13 | sola-design share / pages | Share links stay without password or expiry — **New Link** (`share.rotate`) is how one is revoked. Pages stay shared between the human and agents (one tab per artboard; no hidden agent copy). | capabilities, freeze, manual, CURRENT |
| 2026-10-12 | D6 sola-design share | **Live** share links served by Design (token per canvas, rotatable; only that canvas's files), through a Cloudflare Tunnel to `design.moonlight.pm`. Not snapshots. | freeze, capabilities, architecture, manual |
| 2026-10-12 | sola-design multi-agent | Agents never bring a canvas to the human's screen; any number of canvases live at once, one shown; page calls name their canvas (MCP: the agent's own). | freeze, capabilities, architecture, manual |
| 2026-10-12 | sola-design agent / human | Agent reads never depend on the human's view: screenshots / PDFs hold their board whole (no detail render from the camera reaches it) and the human's pointer leaves the page first; `view` moves the camera only until the human pans / zooms that canvas (`moved: false`). Agent touches glow on the overlay only. (Human ask: pan and zoom freely while an agent works.) | freeze, capabilities, architecture, manual |
| 2026-10-12 | D5 sola-design origin | Boards are served over a loopback http origin (Design's own server: GET/HEAD, project files only, no dotfiles, Host must name a registered project), **project root as `/`**, one `*.localhost` host per board so each board has its own zoom / raster scale. Replaces `file://`. | [freeze](specs/2026-10-12-sola-design-design.md), capabilities, architecture, manual |
| 2026-10-08 | workspaces Claude | Claude is first-class alongside Grok and Codex (D4.4 closed): `--agent grok\|codex\|claude`, spawn/exec argv, hooks merged into `~/.claude/settings.json` (other settings kept), rail rollup, `×N` from `compact_boundary`, reboot `claude --resume`. OpenCode stays presence-only. | freeze, CLI freeze, CURRENT, capabilities, architecture, manual/solactl, PRODUCT, design law |
| 2026-09-17 | Bots | Write fence default remains the bot home. Joshua can explicitly order an escape (whole computer / drop the fence / a path outside home). Seat steal stays forbidden. | [freeze](specs/2026-09-15-sola-bots-design.md), CURRENT locks |
| 2026-09-15 | Bots | Informational named sessions (not coding). Yolo. Per-bot `~/Bots/<slug>/`. Dialog UI hides compaction. Native iOS. Daemon + ACP Grok. Soft write fence via foundation prompt. This computer is the sandbox; `solactl browser` for web. | [freeze](specs/2026-09-15-sola-bots-design.md), CURRENT locks, capabilities |
| 2026-09-11 | Browser page snapshot | Agent-facing page is a **Playwright aria-snapshot YAML** (pruned AX tree + opaque refs), not JSON. Pipeline: `getFullAXTree` → Puppeteer interestingOnly → collapse nameless generic → ref=`backendDOMNodeId` (stale fails). `snapshot` vs later `read`; screenshot is fallback. Confirm still **D3**. | [freeze](specs/2026-09-11-sola-browser-agent-control-design.md), CURRENT locks |
| 2026-09-11 | Browser agent control | Confirmed: agent stays in Workspaces; browser is a **user-equivalent** control plane (`solactl browser`). Same strip / profile; isolation is an ordinary group; **Agent** pocket is a skill convention (chrome does not special-case it). Aside is a loose oracle for the page harness, not the product. Method catalog shipped; confirm still **D3**. | [freeze](specs/2026-09-11-sola-browser-agent-control-design.md), CURRENT locks, capabilities |
| 2026-09-09 | Browser agent control | Agent uses the **same** tabs as the human. It may create a normal tab group for its own work. No agent-only tab type or second profile. Chrome ships generic tab/group/page verbs; particular workflows (pocket, don’t steal the seat) are an agent skill. Face when built: `solactl browser`. Confirm still **D3**. | CURRENT locks, capabilities gap — **amended 2026-09-11** (freeze) |
| 2026-09-09 | workspaces Codex | Codex is first-class alongside Grok: `--agent grok\|codex`, spawn/exec argv, hooks merged into `~/.codex/hooks.json` (Impeccable groups kept), rail rollup. Claude stays presence-only (D4.4). First Codex session may need `/hooks` trust. | freeze, CLI freeze, CURRENT, capabilities, manual/solactl, skill |
| 2026-09-04 | mail From allowlist | Compose From is Settings-chosen identities. Inbox Send from is the Wicket `/api/auth/me` checklist (A–Z, no catch-alls). Extra SMTP accounts still type aliases. | [mail freeze](specs/2026-07-27-sola-mail-kit-design.md), CURRENT, capabilities, manual/sola-mail |
| 2026-09-04 | mail multi-IMAP | Every `imap_enabled` account is connected. Sidebar is six canonical boxes combined. Moves/undo stay on that message’s account. | [mail freeze](specs/2026-07-27-sola-mail-kit-design.md), CURRENT, capabilities, manual/sola-mail |
| 2026-09-03 | mail identities | One IMAP inbox. Extra accounts are SMTP send identities (forwarded Gmail, etc.). Inbox aliases are extra From addresses. `primary_from` is the default. Reply matches original To/Cc. Extra IMAP listed 2026-09-04. | [mail freeze](specs/2026-07-27-sola-mail-kit-design.md), CURRENT, capabilities, manual/sola-mail |
| 2026-09-01 | cargo make | `build` / `install` default to **release**. `--debug` for unoptimized. `--release` still accepted. | sola-make, AGENTS, CONTRIBUTING, CURRENT locks |
| 2026-09-01 | Screenshots | Super+Shift+3/4/5 copy Fastest PNG to the compositor clipboard (promised offer). No auto file / Preview. `solactl` still writes a path. | [image clipboard](specs/2026-09-01-image-clipboard-design.md), CURRENT, capabilities, manual/sola-shell |
| 2026-09-28 | Browser agent control | `fill --file` + read-back (`value`/`matched`, or error). `get` / `wait --control` / `links` (DOM hrefs + media duration). `--control` uses the actions list, then a contenteditable/aria-label search if AX misses the node. | CURRENT, agent-control freeze, capabilities, architecture, manual/solactl |
| 2026-09-28 | Browser agent control | `fill --file` + read-back (`value`/`matched`, or error). `get` / `wait --control` / `links` (DOM hrefs + media duration). `--control` uses the actions list, then a contenteditable/aria-label search if AX misses the node. | CURRENT, agent-control freeze, capabilities, architecture, manual/solactl |
| 2026-09-24 | Browser agent control | `fill`/`type` paste the whole string into a contenteditable / ProseMirror root (AX wrapper may have a contenteditable descendant). `key --chord` is `rawKeyDown` + one `char` (not keyDown-with-text plus char). A dead iced subscription fails the invoke; a timeout while `browser` is still advertised names the wedged window and the recovery (close X / Flower, relaunch — bots must not kill). | CURRENT, agent-control freeze, capabilities, architecture, manual/solactl |
| 2026-09-01 | Browser tab favicons | Tab strip shows a reserved 16px leading favicon (CEF `on_favicon_urlchange` + `download_image`). Globe while a http(s) page has no icon yet; empty slot on blank. No domain-name prefix. | CURRENT, capabilities, architecture, manual/sola-browser |
| 2026-09-01 | Browser popups | `window.open` with features (`NEW_POPUP`) and `about:blank` stay windowless CEF so the site gets a `Window`; chrome paints that as a focused tab beside the opener. `target=_blank` focuses a new tab; ⌘-click stays background. No native OS popup windows. Wrapper off-site still sola-browser; huddle OSR unchanged. | CURRENT, capabilities, architecture, manual/sola-browser |
| 2026-08-31 | Super+K / Window | Super+K is the system keyboard-shortcuts overlay (Omarchy chord). Kit Window menu is the mouse path for zones, float, hide, and cycle; the shell injects it when an app omits it. Paint Crop is Super+Shift+K. | [freeze](specs/2026-08-31-window-menu-and-shortcuts-design.md), CURRENT locks, capabilities, manual/sola-shell |
| 2026-08-28 | D1 / T1 | `crates/sola-agent` retired. No ACP/Grok-leader GUI; Workspaces is the agent product. Multi-client ACP permission fan-out and pin UI are moot. Do not rebuild. | CURRENT, capabilities, architecture, this log |
| 2026-09-11 | Browser vault | Item **Edit** on the record: same hanging card; all shown types; org items when writable; passkeys not edited; no delete. | [item edit freeze](specs/2026-09-11-sola-browser-vault-item-edit-design.md), CURRENT, capabilities, manual/sola-browser |
| 2026-08-28 | Browser vault | One toolbar Bitwarden control (not login / authenticator / card widgets). Search + type chips + full item record (notes, identities, cards, TOTP). Click opens the record; Autofill **Fill** injects. Icon may change (lock / key / shield / fingerprint). | [unified panel freeze](specs/2026-08-28-sola-browser-vault-panel-design.md), CURRENT, capabilities, manual/sola-browser |
| 2026-09-23 | workspaces | `workspace.rm` / hover × drop the tab on the next tick; `tmux kill-session` and `git worktree remove` run off the iced thread so a CLI rm does not freeze the rest of Workspaces. Last-leaf ⌘W still kills tmux on the UI thread (session name is reused). | CURRENT, CLI freeze, capabilities, architecture, manual/solactl |
| 2026-09-21 | workspaces | Quiet `×N` is the loudest Grok **or** Codex session in the tab. Codex counts `type: compacted` records in `~/.codex/sessions/**/rollout-*-{session_id}.jsonl`. Grok still uses session-dir segments/checkpoints. Shell panes still do not contribute. | CURRENT, DESIGN, PRODUCT, freeze header, capabilities, architecture |
| 2026-08-25 | workspaces | Split workspaces stay one rail row (no grok/shell child tabs). The status mark rolls up every Grok pane in the tab: waiting (needs attention) > working > done > idle. `×N` is the loudest Grok session. **Amended 2026-09-21:** `×N` includes Codex. | CURRENT, DESIGN, PRODUCT, freeze header, capabilities |
| 2026-08-28 | workspaces | Desk cards (not menubar whispers) for unfocused done/waiting. Title is `{project} · {tab}`; body is `grok is done` / `needs attention`. | CURRENT, DESIGN, notifications freeze, capabilities |
| 2026-08-27 | workspaces | Cross-pane prompt is tmux bracketed paste + Enter, not raw PTY `write`/`send-keys -l`. Fixes clipped composer that needed a tab switch + mousewheel to submit. | CURRENT, CLI freeze, manual/solactl, skill |
| 2026-09-14 | workspaces | Rail label is `--title` if set, else the worktree slug (`--name`). Do not prefix `slug ·` onto a custom title. Hover pencil / double-click is `--title` (empty or equal to slug clears). `--name` still moves `.worktrees/<slug>`. JSON `name` stays the slug/`root`; `label` is the rail. Resolve `--workspace` by id, pane, path, slug, then title. Drag reorders tabs inside a project and project groups. `--before` / `project.reorder` are the call-plane verbs. No project recolor. | CURRENT, CLI freeze, DESIGN, capabilities, manual/solactl |
| 2026-09-03 | workspaces | `workspace.set --name` is `git worktree move --force` to `.worktrees/<name>` (promote an ad hoc tab to a ticket folder). Catalog id stays (tmux sessions keep working). `--branch` is `git branch -m` in that checkout and does not move the folder. Project root cannot be renamed. **Amended 2026-09-14:** `--name` is the folder (rail default); `--title` is the rail label; UI rename is `--title`. | CURRENT, CLI freeze, manual/solactl, skill |
| 2026-09-03 | workspaces | A pane told to clean up its own worktree must not `git worktree remove` first (cwd dies; tab + working spinner stayed). `workspace.rm --worktree` replies, closes the tab, then `git worktree remove`. `--force` is toss/dirty. Hover × / plain `workspace.rm` still leave the checkout. A gone `.worktrees/<name>` path reaps the tab. | CURRENT, CLI freeze, manual/solactl, skill |
| 2026-08-25 | workspaces | `workspace.rm` / `project.rm` reply `{ok:true}` then teardown on the next tick so a pane can close itself without hanging `solactl`. SessionStart idles a leftover working ring. Cleanup is one shell (`git worktree remove` && `workspace.rm`); splitting those as two Grok tools leaves the tab (cwd gone). **Superseded 2026-09-03:** use `workspace.rm --worktree`; gone paths reap. | CURRENT, CLI freeze header, manual/solactl, skill |
| 2026-08-18 | workspaces | Per-project startup script after sibling spawn. Project menu + `project.startup`. Env: `PROJECT` / `WORKTREE` / `NAME`. | CURRENT, PRODUCT, CLI freeze, manual |
| 2026-08-18 | workspaces CLI | Face is `solactl workspaces` (owner renamed from `ws`). First-class. New verbs: `project.add`, `project.startup`, `workspace.select`, `workspace.set`, `workspace.exec`, `pane.wait`, `whoami`. Spawn `--branch` / `--base-branch` / `--title`. `--prompt-file`; richer list/spawn payloads; Grok-leaf targeting; parent from `$SOLA_PANE_ID`. Confirm still **D3**. | [CLI freeze](specs/2026-08-18-workspaces-cli-design.md), CURRENT, capabilities, manual/solactl |
| 2026-08-18 | D4.1 amend | Call owner is `workspaces` (`solactl workspaces …`). Tmux `sola-ws` / `sws-` unchanged. | CURRENT + freeze + PRODUCT |
| 2026-08-18 | workspaces | ⌘W closes the focused pane. Drop Project is menu-only. Kit splits (⌘⇧↓ / ⌘⇧→); leaf rows only after a split; last pane close keeps the workspace (Start new shell); a split leaf that exits retracts; hover does not spawn. Quiet `×N` only on a Grok leaf. **Amended 2026-09-21:** Codex leaves count too. | CURRENT, DESIGN, freeze header, capabilities |
| 2026-08-31 | Browser tab groups | Pocket color: edit-mode swatch (default kit well fill) opens the kit picker; persist hex on the group; omit when default. | [freeze](specs/2026-08-15-sola-browser-tab-groups-design.md), CURRENT, capabilities, manual/sola-browser |
| 2026-08-29 | Browser tab groups | New group is ⌘G on a selected loose tab (name focused and selected). Hover pencil to rename later. Strip right-click removed; join/leave stay drag. | [freeze](specs/2026-08-15-sola-browser-tab-groups-design.md), CURRENT, capabilities, manual/sola-browser |
| 2026-08-29 | wrapper huddle popup | Slack huddle is `window.open('about:blank')` NEW_POPUP. Allow as windowless CEF OSR tab and paint it; `window.close` returns to the channel. Off-site http(s) still sola-browser. | [wrapper freeze](specs/2026-08-21-sola-wrapper-design.md), CURRENT, capabilities, manual |
| 2026-08-28 | wrapper media | Huddle / `getUserMedia` is a kit Allow / Block (same overlay as notifications). Persist `media.json`. Not `--enable-media-stream`. Screen share uses the same dialog when requested. | [wrapper freeze](specs/2026-08-21-sola-wrapper-design.md), CURRENT, capabilities, manual |
| 2026-08-28 | wrapper links | Off-site `target=_blank` / ⌘-click / `window.open` open in sola-browser. Same-site stays in the wrapper. Main-frame navigations (SSO) are not cancelled. | [wrapper freeze](specs/2026-08-21-sola-wrapper-design.md), CURRENT, capabilities, manual |
| 2026-08-15 | Browser tab groups | In-strip folders (spaces later); groups at top, loose run at bottom; menu + drag join/leave; New group is menu-only; collapse keeps the page; empty dissolves; kit context menu | [freeze](specs/2026-08-15-sola-browser-tab-groups-design.md), CURRENT, capabilities |
| 2026-08-15 | Browser instance | One iced chrome via `chrome.sock`; second process hands off (does not reap live helpers). Helper death respawns + restores tabs. | CURRENT, capabilities, architecture, manual/sola-browser |
| 2026-08-15 | Browser passkey | `get()` intercept in every frame; same-site duplicate/retry coalesced (Gemini Exchange 2FA was failing the page before pick); `create()` vault confirm + persist (new login or attach) | CURRENT, capabilities, manual/sola-browser |
| 2026-08-15 | Kit storybook | Always update the matching storybook page in the same change; do not ask (*superseded 2026-10-30: the storybook is retired*) | AGENTS |
| 2026-08-14 | D4.1 | Product is `sola-workspaces`. Owner `ws` (`solactl ws …`). Tmux `sola-ws` / `sws-`. Config `~/.config/sola/workspaces/`. | CURRENT + freeze + PRODUCT |
| 2026-08-14 | workspaces | No `sat` binary. Face is `solactl ws …` only. | CURRENT + freeze |
| 2026-08-14 | Browser downloads | Auto-save `~/Downloads`; toolbar icon with progress; click-to-open panel; persist completed in `shared/downloads.json`; cancel + open + remove-row (no Finder) | [freeze](specs/2026-08-14-sola-browser-downloads-design.md), CURRENT, capabilities, manual |
| 2026-08-14 | Call plane | Third plane `sola-call`; fail if owner down; `solactl` face is `compositor`/`session` (not `call 'sig'`); advertise for unknown apps; MCP later; confirm is **D3** | [freeze](specs/2026-08-13-sola-call-plane-design.md), CURRENT, architecture, capabilities |
| 2026-08-13 | workspaces | Promoted idea → freeze. Spawn sibling is v1; design law; not `sola-agent`. Product forks now **D4** (D3 taken by call plane). | freeze + CURRENT + D4 |
| 2026-08-13 | D4.2 | Worktrees live in `<project-root>/.worktrees/<name>`. Not `~/orca/workspaces/…`, not sibling-of-main. | freeze + CURRENT + PRODUCT |
| 2026-08-13 | workspaces | **Grok is the first-class CLI** — implement and test Grok first. Claude remains D4 (presence-only until Grok hooks are solid). | freeze + CURRENT + design law |
| 2026-08-13 | Unified sidebar | Terminal density Large; keep `Row` name + redefault to etch; browser divider via `SidebarPanel::resizable_with`; settings/mail/preview lose selection-teal | unified-sidebar freeze + plan, CURRENT, capabilities |
| 2026-08-13 | Browser OSR | IME + Shift+wheel + `<select>` PET_POPUP dogfooded; passkey **registration** deferred until needed | CURRENT, capabilities, manual |
| 2026-08-13 | Browser vault | Create login: save Bitwarden cipher first, then fill; always available on unlocked card; last username + generated password + bare apex URL | create-login freeze, CURRENT, capabilities, manual |
| 2026-08-12 | Browser persist | YouTube login survives full quit; ARGB→BGRA swizzle confirmed (no red wash) | CURRENT, capabilities |
| 2026-08-12 | Browser vault | Passkey **get** dogfooded (Google): intercept → picker → assert; clean web `clientDataJSON`; wire field `clientDataJSON` (not camelCase) | CURRENT, capabilities, manual/sola-browser |
| 2026-08-12 | Browser install | Prefer `cargo make install browser --release` for Bitwarden KDF; restore `--release` on install *(superseded 2026-09-01: release is default)* | sola-make, CURRENT, manual |
| 2026-08-12 | D8 / Browser | Profiles menubar (switch/create/rename/delete); switch re-exec; CEF under `profiles/<uuid>/cef/` | profiles freeze, CURRENT, capabilities, architecture, manual/sola-browser |
| 2026-08-12 | Browser windows | One iced chrome window (`sola-browser`). Instant profile switch via headless per-profile CEF helpers, not extra Wayland windows / unique app_ids. | CURRENT, capabilities, architecture, manual |
| 2026-08-11 | T2 / Browser | CEF-only single crate `sola-browser`; WPE multi-crate path retired after failed dogfood | CURRENT, architecture, capabilities, AGENTS |
| 2026-08-06 | dist | Distribution branch merged to master; qcow e2e OK; ISO e2e still open; interim TZ US/Mountain | freeze + plan + CURRENT |
| 2026-08-05 | — | Progress documentation practice adopted for Sola | CURRENT, progress-model, AGENTS |
| 2026-08-05 | dist | ISO primary; wizard = username + disk only; US EN + Mac keyboard fixed; hostname `sola`; no password; loginless → Sola; flower brand splash | [distribution-image freeze](specs/2026-08-05-distribution-image-design.md) |
| (earlier) | UI stack | Iced + sola-kit; WebView apocrypha | AGENTS, CURRENT locks |
| (earlier) | Browser | WPE primary, CEF parallel *(superseded 2026-08-11)* | — |
| (earlier) | Agent backend | Shared Grok leader only | AGENTS, CURRENT locks |
