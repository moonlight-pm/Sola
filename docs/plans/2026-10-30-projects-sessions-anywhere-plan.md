# Projects — sessions anywhere: plan

**Spec:** [sessions anywhere](../specs/2026-10-30-projects-sessions-anywhere-design.md) (approved 2026-10-30, D16 answered)  
**Branch:** master (root tab on novus). Builds at half the CPUs (`CARGO_BUILD_JOBS=16`, `nice`).  
**Install:** `cargo make oath apply` only with Joshua's word, each time. Never reboot novus without it.

Each step builds on its own; check boxes as they land.

## Slice 1 — sessions belong to hosts

- [x] **1.1 Label parts.** `sola_core::host_color` (Oath's prompt hue,
  pinned to novus `#C33C96` / canto `#8D3CC3`) and
  `sola_core::window_label` (`Oath › fix login | novus`, parsed from the
  right). Kit row: `SidebarItem::detail` (quieter run after the title) and
  `SidebarItem::tag` / `SidebarTag` (chip). Shell tab bar: a
  `sola-projects` title that parses draws project, session, host chip
  (ink at lightness 0.72 for contrast, fill the prompt color at 22 %).
  Projects: every window's title is its label, this computer named too
  (`machine::this_host`). The window frame shows the same title as text
  until the kit frame takes parts.
- [x] **1.2 One socket per host.** Remote tabs use `-L sola-ws` there
  (`sola_terminal::remote::socket`); every remote command finds a session
  on either socket (`remote::pick`: `sola-ws`, unless only
  `sola-ws-remote` has it), alive / kill check both, nothing new is made
  on `sola-ws-remote`.

**Order from here:** 1.3, 1.4, 1.7 and 1.8 are one change. A viewer must
key a session by host and name (canto's own `ws-main` and novus's are
different sessions), which is the window-per-session model; keying the
current project / tab / split model by host first would be done twice.
So the next step is the core: `App` holds session windows keyed by
`(host, session)`, built from each host's `@sola-*` listing.

- [ ] **1.3 Ids unique per host.** A new session's id is checked against
  that host's live sessions (local tmux, or `list-sessions` over the
  shared ssh), not only this catalog; tmux `new-session` refusing a name
  moves to the next.
- [ ] **1.4 The session describes itself.** `@sola-project`, `@sola-name`,
  `@sola-title`, `@sola-kind`, `@sola-parent`, `@sola-agent`,
  `@sola-agent-session` set at creation and on rename / move / agent
  change; stamped on first sight for sessions made before this.
- [ ] **1.5 Closing detaches.** Window close, tab ×, ⌘W and Projects quit
  drop this computer's client only. **End Session** (menu, `tab.rm`)
  kills, with a confirm while its agent works or waits; worktree removal
  stays its own choice.
- [ ] **1.6 Size claim.** No `resize-window` on every resize; a window
  claims the session's size when it gains focus or gets a key / click.
- [ ] **1.7 One window per session.** The in-window tab bar, its `+` card,
  drag reorder, splits and the project roll-up go; ⌘T / New Session opens
  the spawn card for that project and host; each session its own window.
- [ ] **1.8 Open any session.** The launcher's chooser lists each host's
  projects and their live sessions (from `@sola-*`); `project.open` /
  a new `session.open` attach one here.
- [ ] **1.9 Docs.** capabilities `projects` row, CURRENT, manual
  (`solactl.md`, `sola-shell.md`), architecture (sockets).

## Slice 2 — status on the host

Hooks set `@sola-status` in their own session; a control-mode client per
host subscribes; islands on every desk with the session open; the link,
forwarded hook sockets and hooks server removed.

## Slice 3 — mirrors

A viewer not holding the size scales the session to fit; Watch
(`attach -r`).

## Slice 4 — reboot

`sola-projects --restore` (headless) at Projects start and at boot (Oath
`svc`, with the Oath tab); `session.start` from anywhere.

## Slice 5 — CLI

`tab.*` → `session.*`; manual in the same change.
