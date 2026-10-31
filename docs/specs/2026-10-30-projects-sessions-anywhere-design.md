# Projects — sessions on any computer, opened from any computer

**Date:** 2026-10-30  
**Status:** **approved for implementation** (Joshua, 2026-10-30: "Your recommendations on the rest … Go ahead and get it started"). D16 answered.  
**Branch:** `next` (Projects on Oath)  
**Supersedes, when approved:** parts of the [Projects freeze](2026-08-13-sola-agent-terminal-design.md) (the 2026-10-28 "another computer" and "closing one closes it" headers; the in-window tab bar of 2026-10-21) and the [CLI freeze](2026-08-18-workspaces-cli-design.md) (`tab.*` → `session.*`).

Joshua (2026-10-30): "what i'd like to be able to do, really, is connect from
any machine to any machine and connect to any ongoing session. Sessions run on
some computer through tmux or whatever — but i want to make a distinction
between say closing the window on this computer to a remote session, and that
session being alive. We should have to deliberatly tell sessions to close."
"first thing, if i have a session started on this machine (novus) i want to be
able to open it up and watch it from canto." "tab should always show the host
even if local" ("Oath | novus"). "our current in-window tabs don't really make
sense for this .. we'll probably need to remove that". Longer term: "make it
real easy to put a bunch of oath machines on a loose cluster."

This walks back two 2026-10-28 answers on purpose: closing a project's window
no longer ends its sessions, and the tab shows the host again.

---

## As built (what has to change)

- **Ownership is the viewer's.** A session's facts (project, tab, kind, agent,
  agent session id, status) live in the catalog of the Projects that made it
  (`catalog.json`, `last-status.json`). Another computer cannot know them.
- **Two sockets per host.** A computer's own tabs are on `-L sola-ws`; tabs
  opened from elsewhere are on `-L sola-ws-remote` there, kept apart on purpose.
- **Ids are unique per catalog only.** novus and canto both run `sws-ws-main`;
  remote root ids are `ws-<host>-<folder>`, so two viewers of one folder land
  on the same session and either one's close kills it for both.
- **Closing kills.** Window close (`end_project`), tab ×, `tab.rm` and ⌘W on a
  leaf send `kill-session`. Only quitting Projects detaches.
- **Status goes to the creator.** Hooks `curl` the socket in
  `SOLA_WS_HOOKS_SOCK`, stamped at `new-session` with the creator's socket (a
  remote one exists only through the creator's `ssh -N -R` link). The remote
  look (`ps` per host every 3 s) runs only for linked hosts.
- **Local panes force the size.** Every resize runs `resize-window -x -y`
  (tmux.rs:699-713), which puts the window in manual size: two attached
  computers would fight. Remote panes don't.
- **Multi-client is otherwise fine.** tmux 3.8 on novus and canto; attach never
  uses `-d` (no kicking); `window-size` is tmux's default `latest`; novus ↔ canto
  ssh by key both ways.

---

## Target model

**Host** — this computer, or an ssh host; named by its hostname.

**Session** — a tmux session on its host's one Projects server, `-L sola-ws`
(the `sola-ws-remote` socket goes away). Named `(host, sws-<id>)`; `<id>` is
unique **on that host**: chosen against that host's sessions, and tmux is the
arbiter (`new-session` refuses a taken name → next id). It runs whether or not
anyone is watching.

**Viewer** — Projects on some computer. It holds windows onto sessions (any
host, this one included) and remembers which, to reopen them after it
restarts. It owns no session.

### The session describes itself

At creation, and on rename / move, the session carries its facts as tmux user
options, so every viewer reads the same thing:

| Option | Example |
|---|---|
| `@sola-project` | `/home/Projects/Oath` |
| `@sola-name` | `sc-1234` (worktree folder / slug) |
| `@sola-title` | `fix login` |
| `@sola-kind` | `main` · `root` · `worktree` |
| `@sola-parent` | `sws-ws-main` |
| `@sola-agent`, `@sola-agent-session` | `claude`, `<uuid>` |
| `@sola-status`, `@sola-status-at` | `waiting`, epoch |

A file per session on its host (`~/.config/sola/projects/sessions/<id>.json`)
keeps the same facts across a reboot (tmux gone).

### After a reboot, sessions come back by themselves

Joshua: "yeah start them — as other sessions might expect them to be there…
alternatively other sessions could tell them to start." So both:

- **At boot.** A headless restore (`sola-projects --restore`: no window, the
  same code the app uses) starts every session whose file is there and whose
  tmux is not, resuming its agent (`claude --resume` / `grok -r`) when its
  transcript is on disk. On Oath it runs once at boot as `home`
  (`svc:sola-sessions`, Oath's desk profile), so a computer with no desk
  brings its sessions back too. Projects also runs it when it starts.
- **On request.** *(As built: no separate verb.)* Any computer that links
  the host — opening a project there, or a window of one of its sessions —
  runs the same restore there over ssh first, so a host with no Sola (tmux
  only) gets its sessions back that way too.
- The file: `~/.config/sola/projects/sessions/<id>.json` (project, name,
  title, kind, folder, agent, agent session). Written when a session is made
  or first attached, rewritten when its agent's session changes or it is
  renamed, removed when it is ended or its shell exits with its computer up.

`SOLA_PANE_ID` and `SOLA_WS_PATH` stay in the environment; `SOLA_WS_HOOKS_SOCK`
goes.

### Status lives on the host

*(As built, 2026-10-30 — changed from tmux options: a hook's payload can be
megabytes (a tool's input), and a Codex hook can run outside its session.)*

- **Hooks append to their computer's hook log**
  (`~/.local/state/sola/projects/hooks.log`): one line per event,
  `<agent>\t<SOLA_PANE_ID>\t<base64 payload>`, under `flock`, rotated at
  8 MiB. No socket, no `curl`, no reverse link.
- **Viewers follow each host's log**: this computer's by reading the file,
  another's with one `ssh host tail -F` per host (the link). Each event goes
  through the same mapping and placing as before (`SOLA_PANE_ID`, else the
  payload's cwd), tagged with its host. Status reaches every viewer at once.
- **Seen is shared:** a viewer that looks at or answers an island appends
  `seen\t<id>\t`; every viewer lets that island go.
- What still needs a look goes on the existing per-host tick through
  `Machine`: presence from `ps`, Codex rollouts, `×N`, gone worktrees, now for
  every host a viewer has a window on.
- Retired: the hooks server and its sockets, the per-host `ssh -N -R`
  forward. (The boot adopt / quarantine of local sessions stays for now.)

### Windows

- **One window per session.** The in-window tab bar, its `+` card, drag
  reorder and the project window's roll-up go. The shell's tab bar is the one
  list; each row carries its own session's mark.
- **Label** — three parts, always all shown, this computer included: the
  project, the session (when it is not the project's main one: its title,
  else its name) and the host. Drawn with the kit, not as one run of text
  (Joshua: "lets avail ourself of our gui toolkit and not rely on text alone
  to make these different data in the tab distinctive"): the host is a chip in
  **the host's color**, the hue Oath gives that computer's prompt
  (`oath-core` `seat::prompt_colors`: FNV-1a of the hostname, hue = hash mod
  360, HSL 0.53 / 0.50; novus `#C33C96`), so a computer looks the same in its
  prompt and on every desk; the session's part is quieter than the project's.
  Same parts in the shell's row, the window's frame and islands.
  - **How the shell gets the parts:** the xdg title stays the only thing that
    crosses from Projects to the shell (through sola-river), so it is written
    and read by one typed convention, `sola_core::window_label`
    (`format` / `parse`): `Oath › fix login | novus`. Parsed from the right,
    so a folder may hold any text. A title that does not parse draws as
    plain text, as today. `sola_core::host_color` is a copy of Oath's
    function, its test pinned to Oath's values.
- **New session** stays a first-class verb (design law 4): ⌘T / Project → New
  Session in a session window opens the spawn card (worktree or root, agent,
  prompt) for that project on that host. The new session opens in its own
  window.

### Closing and ending

| Act | Session |
|---|---|
| Close a window (close dot, ⇧⌘W, shell stoplight) | **keeps running**; this computer's client detaches |
| Quit Projects | keeps running (as today) |
| **End Session** (menu, `session.end`) | ends: `kill-session`, its file removed; worktree stays unless "End and Remove Worktree" |
| Process inside exits | tmux ends the session; every window on it shows it ended |

### Opening

- **Launcher → Projects:** hosts (this computer first, then recent; later the
  cluster's roster), each host's `~/Projects` folders, and under each folder its
  live sessions with marks. Pick a session → its window opens here, or comes
  forward if already open. Pick a folder with none → its main session starts
  there.
- **CLI:** `session.list [--host] [--project]`, `session.open --host H
  --session S`, `session.new` (was `tab.spawn`), `session.end` (was `tab.rm`),
  `session.set`; `pane.*` and `whoami` take the session. `whoami` can answer
  from the session's own options, with no Projects on that host.

### Two computers on one session

Joshua: "can't the output be reformatted to each screen? … i know for sure
canto's output windows are going to be much smaller."

- **One size at a time.** A session has one grid, and so does the program in
  it: Claude, Codex and Grok lay out their screen for one width and height,
  and redraw when it changes. Two screens cannot each get their own layout of
  the same live TUI. (Reflowing per screen works only for plain scrolling
  text, which these are not.)
- **The size follows the computer you are using.** A viewer claims the size
  when its window gains focus or gets a key or a click (the KVM moving you to
  the other desk counts at the first key): it sets the session to its own grid
  (`resize-window`, as local panes do today, but only on a claim, not on every
  resize). The program redraws for that screen, and tmux rewraps its history,
  so scrolling back reads right at the new width.
- **Every other viewer shows a mirror.** It sizes its tmux client to the
  session's grid (the control client reports `window_width` / `height`) and
  scales the text to fit its window: smaller on canto for a novus-sized
  session, larger the other way. Nothing cut off, no tmux dot padding. A
  quiet mark says whose size it is ("sized for novus"). A key or a click in
  the mirror claims the size back.
- **Inline and full screen both work.** Inline (Claude Code's default), the
  output lands in tmux's history and our scrolling covers it, rewrapped at
  each claim. Full screen (alternate screen), the program keeps its own
  history and redraws on the claim; the wheel goes to it as today (`smcup`
  forwarded). A mirror scales either the same way.
- Attach as today (no `-d`). Optional **Watch** (`attach -r`): a read-only
  mirror that never claims.

---

## Slices

1. **Sessions belong to hosts.** One socket per host; ids unique per host;
   `@sola-*` options and the session file at creation; close window / quit =
   detach, End Session explicit; size claimed on focus / input instead of on
   every resize; one window per session (in-window tab bar removed); the
   three-part label with the host chip (`window_label`, `host_color`); the
   chooser lists a host's sessions and opens one. **Done when:** a session
   started on novus opens from canto, both type in it, the session follows the
   desk in use, and closing canto's window leaves it running on novus.
2. **Status on the host.** Hooks → the host's hook log; viewers follow it
   (here by reading, elsewhere by `ssh tail -F`); islands on every desk with
   that session open, seen shared; hooks server and the reverse forward
   removed.
3. **Mirrors — not doing** (Joshua, 2026-10-30: "keep it working like it
   currently does … the scaling will just look silly"): the window not
   holding the size shows the session as tmux draws it.
4. **Reboot.** `sola-projects --restore` at boot (Oath `svc`), at Projects
   start, and on every link to a host.
5. **CLI** `tab.*` → `session.*` with the manual (`solactl.md`) in the same
   change.

Migration: sessions already on `sola-ws` get their options stamped the first
time a viewer sees them (from today's catalog); sessions on `sola-ws-remote`
keep attaching there until they end, and nothing new is made on it.

Later, not here: a cluster roster instead of recent hosts (Oath shares keys
and names among its machines); D8 (agents in a session driving a viewer).

---

## D16 — decision points (answered 2026-10-30)

1. **What ends a session:** only End Session (menu / CLI) or its process
   exiting; a confirm when its agent is working or waiting.
2. **Splits:** dropped. The shell's zones put two session windows side by
   side.
3. **Label:** project, session and host as distinct kit parts, the host in its
   prompt color.
4. **Islands across computers:** a desk shows an island for a session it has a
   window on; looking or answering on any desk clears it on all.
5. **After a reboot:** sessions start again by themselves; anything may also
   start one.
6. **Size:** follows the desk in use; the others show a scaled mirror (Joshua:
   "i like that idea. However, we might as well support the agents full screen
   mode if possible as well").
