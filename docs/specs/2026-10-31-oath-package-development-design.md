# Developing Oath packages with several sessions at once

**Date:** 2026-10-31  
**Status:** approved (Joshua, 2026-10-31: "This all sounds good"; integration branch `next`, a user-level skill, the same rules on novus and canto, the package split now; "make sure our 'checkpoint' and 'cleanup' skills are updated to work with our new workflow")  
**Scope:** any project that makes Oath packages (Sola, Oath's own `pkgs/`, the next one). Sola's side is built here; Oath's side is the Oath session's (brief in § Who does what).

Joshua (2026-10-31): "when i've got multiple sessions working in the same
repo … each gets to a point to build something and apply it, one will
often overwrite the other — that is, publish some packages without the
features of the other … Only on merge and build or sync to master do they
begin to agree again." It happened on 2026-10-14 (a stale Design worktree
install dropped ten master commits for hours) and on 2026-10-30 (the zones
and sessions-anywhere applies took each other's features off novus).

---

## Two lanes

| | Release | Dev overlay |
|---|---|---|
| From | master, clean, pushed | a checkout here (master or `next`) |
| How | publish (plans built in `oath build`'s sandbox, uploaded to the origin) → the profile pins the hashes (Oath's tree) → Oath publishes its output → `oath update` | build → `oath store` → `oath set pkg:X hash=…` → `oath apply` |
| Reaches | every machine on the profile | this machine |
| Lasts | until the next release | until the next overlay or `oath update` |

Sola's `cargo make oath apply` and Oath's `cargo make store` are overlay
tools; `cargo make oath publish` and Oath's `cargo make publish` are
release tools.

## What goes wrong

1. **A pin is only a hash.** Nothing says which commit or branch a live
   package came from, so a tool cannot tell what an overlay would drop.
2. **The last overlay wins.** Each overlay replaces the whole package with
   one branch's build.
3. **Coarse packages.** `pkg:sola` holds ~20 programs, among them
   `sola-river`: every overlay moves all of them and restarts the desk.
4. **Overlays are invisible.** `oath update` drops them silently; a
   release may lack what an overlay had.

## The patterns

1. **Provenance on every pin.** A tool that pins a package records its
   source beside the hash: `{repo, commit, branch, dirty, built_by,
   built_at}`. It is metadata, not content: never part of the hash (or
   every commit would move every package), never a reason to move or
   restart anything. Oath carries it on the `pkg` object (`source`,
   shown by `oath get`); until a machine's `oath` has the field, the tool
   keeps it in `~/.local/state/oath-dev/pins.json` (per machine, keyed by
   `pkg:<name>` with the hash it describes).
2. **An overlay keeps what is live.** Before pinning, the tool reads the
   live pin's source. Same repo and its commit not in `HEAD`'s history →
   refuse, naming it (`pkg:sola runs bdbc8d5a from zone-layout; this tree
   lacks it — merge it, or apply from next`). `--force` overrides. A dirty
   tree overlays with a warning (anything after it replaces it). A live
   pin with no source (a release, or an older tool) passes.
3. **`next`: one integration branch per repo** while several sessions
   overlay. A session merges its branch into `next` (never `next` into its
   branch) and overlays from `next`, in its own worktree
   (`.worktrees/next`). When `next`'s branches are on master, `next` is
   reset to master. Overlaying from a feature branch works (pattern 2
   guards it), but makes the next session merge that unreviewed branch —
   the tool says so.
4. **Releases only from master**: clean, `HEAD == master == origin/master`.
   Every project's publish tool refuses otherwise.
5. **Package boundaries follow restart cost and update independence.**
   Programs whose restart tears the session down (compositor, bus, call,
   shell, session manager) in one core package that rarely moves; a leaf
   app per package; protocols between packages stay additive (a new topic
   or method, never a changed one). An overlay pins only the packages
   whose tree changed (reproducible builds: an untouched app builds to the
   same hash and does not move).
6. **Overlays are visible.** `oath get`, `oath diff` and `oath update`
   name packages whose source is not the profile's release ("`pkg:sola`
   is `sessions-anywhere@0d99e340`; update replaces it").
7. **Try a leaf app without an overlay**: run it from the build tree until
   its next restart (`cargo make run <app>` for Sola).

The same rules on every machine (novus daily, canto testing): overlays
from master or `next`, with the guard; a desk restart is asked for.

## Who does what

**Oath (its session, on buccina):**
- `pkg.source` (optional object: `repo`, `commit`, `branch`, `dirty`,
  `built_by`, `built_at`), desired-side metadata: `oath set pkg:X
  source=…` takes it; it is not part of the hash and never moves a
  package; `oath get` shows it.
- Pattern 6 in `oath get` / `oath diff` / `oath update`.
- Pattern 4 in `oath-make`'s `publish` (and pattern 2 for `cargo make
  store` if Oath sessions overlay its own packages).
- The desk profile for Sola's split (below) and how existing machines move
  to it (an update makes no new objects).
- Its `AGENTS.md` and `oath-docs` skill; its stale "novus (Joshua's NixOS
  desk)".

**Sola (here):**
- `sola-make`: provenance on apply (the sidecar now, `source=` once
  `oath` takes it), the guard, only changed packages pinned, publish from
  master only, `cargo make run <app>`.
- The split: `pkg:sola-desk` (bus, call, river, shell, session, kvm,
  lock, solactl, the Oath catalog app) and a package per app (terminal,
  projects, paint, preview, scope, monitor, calendar, mail, settings,
  bots); `sola-browser` and `sola-design` as now.
- Docs and skills (below).

## Skills and docs

| Where | Change |
|---|---|
| **New** `oath-packages` skill, user-level (`~/.claude/skills`, `~/.grok/skills`) | The two lanes, provenance, the guard, `next`, releases from master, package boundaries — for any project |
| Sola `AGENTS.md` § Installing | Overlays from master or `next` with the guard; publish from master; points to `oath-packages`; drops the NixOS `cargo make install` / `/opt/sola` text |
| Sola `.grok/skills/sync-project` | `next`: merge branches in, reset after merges; check what is live before applying |
| Sola `.grok/skills/sola-progress-docs` | Record applied state as package + source (`branch@commit`), not a bare hash |
| User skill `cleanup` (Claude and Grok copies) | Sessions, not Workspaces tabs: remove this session's worktree (from the root), end this session or close its window on request (`solactl workspaces` is gone; a session's worktrees are its own) |
| User skill `checkpoint` (Claude and Grok copies) | Works in the session's worktree or the root; merging to master and `next` follows `oath-packages`; never applies or publishes; records applied state with its source |
| Memories | "merge master before a Design install" and "install every iteration" → the guard and `next` |
| Oath `AGENTS.md`, `oath-docs` | Oath's session |

## Order

1. Spec (this), brief Oath.
2. `sola-make`: provenance (sidecar), guard, publish check, changed-only
   pins. Skills, docs, memories.
3. Split `pkg:sola` with Oath (plans, desk profile, migration).
4. `source=` on pins once `oath` takes it; pattern 6 is Oath's.
