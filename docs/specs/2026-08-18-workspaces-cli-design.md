# sola-workspaces CLI control plane — target freeze

**Date:** 2026-08-18  
**Status:** approved for implementation  
**Parent:** [`2026-08-13-sola-agent-terminal-design.md`](2026-08-13-sola-agent-terminal-design.md)  
**Call plane:** [`2026-08-13-sola-call-plane-design.md`](2026-08-13-sola-call-plane-design.md)  
**Product:** [`crates/sola-workspaces/PRODUCT.md`](../../crates/sola-workspaces/PRODUCT.md)

**Implementation:** methods + payloads + `solactl` invoke timeouts in this slice; `workspace.rm` / `project.rm` reply before tearing down tmux (self-close from a pane does not hang); `workspace.rm --worktree` also `git worktree remove`s after the tab closes (gone checkouts reap the tab even without the call); `workspace.set --name` `git worktree move`s to `.worktrees/<slug>` (id stays; restamps `SOLA_WS_PATH`); `--branch` is `git branch -m`; `pane.send` / `workspace.exec --prompt` bracketed-paste via tmux then Enter
**Dogfood:** `solactl workspaces` still needs a desk smoke after install  
**Gaps:** confirm gates remain **D3** (do not invent); Claude still presence-only (D4); no UI rename modal / recolor / reorder

---

## Goal

Make `solactl workspaces` a **first-class** control plane for Workspaces: same verbs
the app understands, kept in lockstep with the iced surface. A root Grok
session can list projects, spawn a sibling, brief it, read/send, and wait
for done — without Orca’s mailbox.

This is not a second product and not `sat`. Face stays `solactl workspaces …`.

---

## First-class rule (locked)

Any change to Workspaces **verbs, arguments, payloads, targeting, or
timeouts** updates, in the **same change**:

1. `crates/sola-workspaces/src/calls.rs` (advertised `MethodSpec`)
2. Dispatch + behavior in the app
3. Tests for the changed contract
4. [`docs/manual/solactl.md`](../manual/solactl.md)

Do not ship a rail-only verb that agents cannot call. Do not let the manual
drift. UI-only conveniences (name modal, hover ×) stay UI-only; everything
an agent needs to orchestrate is on the call plane.

---

## Decisions (locked)

| Topic | Choice |
|---|---|
| Face | `solactl workspaces <method> [flags]`. Owner is `workspaces` (not `ws`). App or sola-call down → fail. No `sat` |
| Audience | Operators **and** agents (a root Grok in a Workspaces pane) |
| Fan-out | `workspace.spawn --prompt` / `--prompt-file` is the briefed sibling. No mailbox |
| First-class CLI | **Grok** only for `--agent`. Other names error |
| Targeting | Explicit pane id wins. A workspace name prefers the Grok leaf, else the active leaf |
| Parent | `--parent` accepts workspace id/name **or** pane id **or** path. `solactl` fills `--parent` from `SOLA_PANE_ID` when unset (so a sibling nests under the caller) |
| Who am I | `whoami` reads `SOLA_PANE_ID` / `SOLA_WS_PATH` when flags are omitted |
| Start Grok in an existing checkout | `workspace.exec` — reuse a Grok leaf; else start `grok` in the preferred pane |
| Wait | `pane.wait` holds the call reply (does **not** block the iced thread). Default status `done`. `--fresh` waits for a *transition* onto that status |
| Timeouts | MethodSpec may advertise `timeout_ms`. `solactl` uses that, or `timeout` arg + 2s slack. Spawn **60s**, add-project **15s**, wait default **300s** (arg overrides) |
| Confirm | **D3** still open. Every live method is as privileged as the socket |
| Drop | Unregister + kill tmux. Hover × / plain `workspace.rm` leave the checkout. `--worktree` also `git worktree remove` (`--force` if dirty / toss). A worktree tab whose path is already gone is reaped. |
| Spawn focus | CLI `workspace.spawn` is background (rail stays). `--select` jumps. UI spawn / ⌘T always select. `workspace.exec` does not select |

---

## Method catalog

### Existing (payloads enriched)

| Method | Args | Reply (JSON) |
|---|---|---|
| `ps` | — | `{projects:[{id,name,root,workspaces:[{id,name,path,kind,parent,status,agent,selected}]}], selected}` |
| `project.list` | — | `{projects:[{id,name,root}]}` |
| `project.rm` | `--project` | `{ok:true}` |
| `workspace.list` | `--project?` | `{workspaces:[{id,name,path,kind,parent,status,agent,project}]}` |
| `workspace.spawn` | `--project --name [--branch] [--base-branch] [--title] [--agent] [--prompt] [--prompt-file] [--parent] [--select]` | `{id,name,title,path,kind,parent,project,selected}` |
| `workspace.rm` | `--workspace [--worktree] [--force]` | `{ok:true}` — `--worktree` also removes the git checkout (after tmux dies). `--force` needs `--worktree`. |
| `pane.list` | `--workspace?` | `{panes:[{id,status,agent}]}` |
| `pane.send` | `--text [--pane] [--enter]` | `{ok:true, pane}` — bracketed paste into the tmux session, then optional Enter |
| `pane.read` | `[--pane] [--lines]` | `{text, pane}` |

`--prompt` and `--prompt-file` are mutually exclusive. `--prompt-file` is
read by the **app** (same machine). `--prompt` implies `--agent grok`.
`project.list` includes `startup: bool` (script is non-empty). Spawn may
include `startup_error` if the project script failed; the workspace still
exists.

`workspace.spawn` is **background** by default: worktree + catalog row +
tmux/PTY, rail and grid stay put. `--select` jumps to the new workspace
(same as the UI name modal / ⌘T). Do not invert with `--background` /
`--no-select`. `workspace.exec` does not select. `workspace.select` is the
dedicated interrupt. Reply includes `selected: true|false`.

### New

| Method | Args | Reply |
|---|---|---|
| `project.add` | `--path` | `{id,name,root,workspace}` — same as the Add project dialog (`~` expanded) |
| `project.startup` | `--project? [--script]` | `{project,name,script}` — omit `--script` to read; pass it (including empty) to set. Runs after each sibling worktree is created. Script env: `PROJECT` (folder on disk), `WORKTREE` (this tab), `NAME` (tab name). |
| `workspace.select` | `--workspace` | `{id,selected:true}` — rail + attach |
| `workspace.set` | `--workspace [--name] [--title] [--branch]` | workspace JSON — `--title` empty clears. `--name` slugs the rail label and `git worktree move --force`s to `.worktrees/<name>` (id stays; project root cannot rename). `--branch` is `git branch -m` in that checkout (does not move the folder). |
| `workspace.exec` | `--workspace [--agent] [--prompt] [--prompt-file]` | `{workspace,pane,started,sent}` |
| `pane.wait` | `[--pane] [--status] [--timeout] [--fresh]` | `{pane,status}` or error `timeout` |
| `whoami` | `[--pane] [--path]` | `{pane,workspace,workspace_name,project,project_name,path,kind,status,agent}` |

`workspace.exec`:

1. Prefer the Grok leaf in that workspace.
2. If that leaf is already Grok: **bracketed-paste** the prompt (if any) into the tmux session, settle, then Enter. `started=false`, `sent=…`. Do not dump raw keys into the client PTY (`send-keys -l` truncates; coalesced CR often never submits).
3. Else if no tmux session: attach a **new** session with `grok` [prompt] as argv. `started=true`.
4. Else attach if needed and type a quoted `grok …` line into the shell. `started=true`.

`pane.wait`:

- `--status` is `working` / `waiting` / `done` / `idle` (default `done`).
- Default: return when the pane is **currently** that status (immediate if already).
- `--fresh`: if already that status, wait until it leaves and returns.
- Host holds `ReplyTx` and completes on hook / OSC / presence / timeout.
- `--timeout` is seconds (default 300). `solactl` invoke timeout follows.

---

## `solactl` behavior

- Live owner: `solactl workspaces` lists methods; `solactl workspaces <method> …` invokes.
- Bool flags (`--enter`, `--fresh`, `--select`) do not consume the next `--flag`.
- Optional `timeout` arg (seconds) raises the invoke deadline to `timeout+2`.
- Advertised `MethodSpec.timeout_ms` is the default invoke deadline when no
  `timeout` arg is present (spawn / add / wait).
- From a Workspaces PTY, `whoami` and spawn `--parent` pick up pane env.

---

## Non-goals (this freeze)

Mailbox / `worker_done` / ask-reply. MCP adapter. D3 confirm UI. Claude
`--agent`. Split-from-CLI. UI rename modal / recolor / reorder.

---

## Tests (same change)

- JSON builders (project / workspace / pane / spawn reply)
- Prompt vs prompt-file (exclusive; file contents)
- Grok pane preference
- `resolve_workspace` by id, name, pane id, path
- Shell-quoting for `grok '…'`
- `solactl` bool-flag parse (`--enter --text` order; `--select --name` order)
- Empty paste skips tmux; `pane.send` / exec `--prompt` use `paste-buffer -p` then Enter
- CLI spawn leaves the previous workspace selected; `--select` and UI spawn switch
- Wait-status parse + default timeout
- `workspace.rm --worktree` / `--force`; gone worktree path reaps the tab
- `workspace.set --name` moves `.worktrees/<slug>`; `--branch` renames HEAD; root cannot rename
