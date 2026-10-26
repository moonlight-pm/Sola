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

### D2 — sola-kvm permanent input ACL (P1)

**Context:** Input device access currently needs per-boot `setfacl` (or similar).
Permanent udev/ACL is a host-policy choice with security trade-offs.

**Ask:**

1. Prefer udev rule installed with Sola, documented manual udev, or group
   membership?  
2. Acceptable blast radius (all input nodes vs tagged devices)?

**Until decided:** keep documented operator workaround; no silent broad ACL
install from agent sessions.

**Related:** `kvm` capability; [`manual/sola-kvm-operator.md`](manual/sola-kvm-operator.md).

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

### D7 — Bots HTTP API exposure (P0)

**Context (found 2026-10-19, writing the manual):** `sola-botsd` is a managed
component, so it runs in every session, and it binds `0.0.0.0:27419`
(`crates/sola-bots/src/auth.rs`). The bearer token is a constant in that
file, and the repo is public, so the token is public (in history since
2026-09-15). The NixOS firewall is off on the desk, so anyone on the LAN
who reads the source can drive bot sessions, which run `grok agent stdio`
in yolo mode in `~/Bots/<slug>/`. `bot.sola.computer` does not answer from
outside today. The iOS SolaBot app compiles in the same token.

**Ask:**

1. Rotate the token and move it out of source (a file under
   `~/.config/sola/`, or a secret the module provides), rebuilding SolaBot to
   match?
2. Bind to loopback or the LAN address only, or keep `0.0.0.0` behind a
   firewall rule (turn the firewall on and open 27419 only to the proxy)?
3. Should `sola-botsd` stay a managed component in every session, or start
   only when enabled?

**Until decided:** do not publish `bot.sola.computer` again; the manual
states the port is open in every session and does not print the token.

**Related:** `bots` capability; [process manager](manual/process-manager.md);
[solactl § Bots](manual/solactl.md#bots-solactl-bots).

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

1. ~~Display name / window title~~ **decided 2026-08-14 / amended 2026-08-18:** crate / app id `sola-workspaces`; window **Workspaces**; owner `workspaces` (`solactl workspaces`); tmux `sola-ws` / `sws-`; config `~/.config/sola/workspaces/`. **Renamed 2026-10-21** (branch `next`) to free "workspace" for desktop workspaces ([Next N5](specs/2026-10-21-next-tab-bar-design.md)): crate / app id `sola-projects`, window **Projects**, owner `projects` (`solactl projects`, no alias), a workspace is now a tab (`tab.*`), config `~/.config/sola/projects/`; tmux names unchanged.  
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
| 2026-08-15 | Kit storybook | Always update the matching storybook page in the same change; do not ask | `.grok/rules/kit-storybook-pages.md`, AGENTS |
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
