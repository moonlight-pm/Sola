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

- [x] **1.3 Ids unique per host.** A new session's id is checked against
  that host's live sessions (local tmux, or `list-sessions` over the
  shared ssh), not only this catalog; tmux `new-session` refusing a name
  moves to the next.
- [x] **1.4 The session describes itself.** `@sola-project`, `@sola-name`,
  `@sola-title`, `@sola-kind`, `@sola-parent`, `@sola-agent`,
  `@sola-agent-session` set at creation and on rename / move / agent
  change; stamped on first sight for sessions made before this.
- [x] **1.5 Closing detaches.** Window close, tab ×, ⌘W and Projects quit
  drop this computer's client only. **End Session** (menu, `tab.rm`)
  kills, with a confirm while its agent works or waits; worktree removal
  stays its own choice.
- [x] **1.6 Size claim.** No `resize-window`: Projects sizes its client
  only (`PtyBackend::resize_client`), unpins a session it attaches
  (`tmux::follow_latest`, and in the remote attach script), and our
  tmux.conf says `window-size latest` — the computer that typed last sets
  the size. Focus alone does not claim (tmux counts keys); revisit with
  mirrors (slice 3).
- [x] **1.7 One window per session.** The in-window tab bar, its `+` card,
  drag reorder, splits and the project roll-up go; ⌘T / New Session opens
  the spawn card for that project and host; each session its own window.
- [x] **1.8 Open any session.** `project.open` (the chooser's pick, here
  or **On another computer…**) opens every session of the project on its
  computer; the listing is shared (`sola_core::project_sessions`). The
  chooser lists each folder's running sessions under it (this computer's
  at once, a host's in the same ssh call as its folders); a session's row
  opens it alone (`project.open --session`).
- [~] **1.9 Docs.** capabilities `projects` row, architecture, manual
  `solactl.md`, AGENTS.md + the Projects skills (worktrees are the
  session's). Left: `sola-shell.md` (the row's parts), CURRENT on merge.

## Slice 2 — status on the host

- [x] Hooks append to their computer's hook log (`hooks/log.rs`, the script
  in `hooks/install.rs`: base64 under `flock`, never an argument — tested
  with a 300 KB payload; rotated at 8 MiB).
- [x] This computer follows its own log (`log::follow_local`, a thread;
  rotation followed); a link follows another's (`ssh::follow` +
  `tail -n0 -F`), events tagged with their host.
- [x] Seen shared: `islands.sync` / an island action → `announce_seen` →
  `seen\t<id>\t` in that host's log → `Msg::Seen` everywhere.
- [x] Hooks server, its sockets, `SOLA_WS_HOOKS_SOCK`, `ssh::forward` gone.
- [ ] Smoke: an agent on novus marks and islands on canto; seen on one
  clears both; a Codex hook outside its session still placed by cwd.

## Slice 3 — mirrors (not doing, 2026-10-30)

Joshua: "for slice 3 -- i may be inclined just to keep it working like it
currently does. sometimes the windows will be so vastly different in size
the scaling will just look silly." The window that does not hold the size
shows the session as tmux draws it (padded or cut) until you type in it.

## Slice 4 — reboot

- [x] A record per session on its computer
  (`~/.config/sola/projects/sessions/<id>.json`, `records.rs`): written
  when a session is made or first attached (never over another's),
  rewritten when a hook says its agent's session changed or it is renamed
  (its tmux options too), removed when it is ended (End Session, `tab.rm`)
  or its shell exits with its computer up.
- [x] `records::restore`: every recorded session whose tmux is gone, made
  detached as it was (folder, env, options), its agent resumed when its
  transcript is there. Runs at Projects start (this computer), when a link
  comes up (that computer — any computer opening a project there brings
  its sessions back), and headless: `sola-projects --restore` (before
  `startup`, no Wayland). Tried on novus with a throwaway record.
- [x] `--restore` at boot waits up to 30 s for the `sola-ws` server and
  never starts one (`tmux -N`: one started in the oneshot's cgroup would
  die with it, the sessions too — the Oath agent's catch); then up to 15 s
  for the desk's Wayland name, and restored sessions get the desk's
  `WAYLAND_DISPLAY`, `XDG_SESSION_TYPE`, `DISPLAY` (a resumed agent keeps
  its environment: Claude pastes images through Wayland). At Projects start
  the same, without waiting. Tried on novus.
- [x] At boot on Oath: `svc:sola-sessions` in the desk profile — Release B
  (dd0f847aa9c9, 2026-10-31) turns it on, its exec just `sola-projects
  --restore`. Machines that took Release A first need `oath new` once
  (done on novus and canto); both rebooted and their sessions came back.
- Not done: `session.start` as its own verb (opening a project on a
  computer, or linking it, brings its sessions back).

## Slice 5 — CLI

`tab.*` → `session.*`; manual in the same change.
