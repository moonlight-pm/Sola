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
   same hash and does not move — so a tree's `components.json` says the
   crate's version, never `+g<rev>`, which moved every package on every
   commit until 92815a65).
6. **Overlays are visible, and a release replaces them.** `oath get`,
   `oath diff` and `oath update` name packages whose pin is not the
   release's ("`pkg:sola` is `sessions-anywhere@0d99e340`; update replaces
   it"); `oath update --keep pkg:X` keeps one (Joshua, 2026-10-31: replace,
   not survive-with-`--take`). An update also makes objects new to the
   profile.
7. **Try a leaf app without an overlay**: run it from the build tree until
   its next restart (`cargo make run <app>` for Sola).

The same rules on every machine (novus daily, canto testing): overlays
from master or `next`, with the guard; a desk restart is asked for.

## Who does what

**Oath (its session, on buccina):**
- `pkg.source` (optional, closed object, all six keys: `repo` as
  `https://host/owner/name` without `.git`, `commit` the full SHA,
  `branch` (`""` detached), `dirty`, `built_by` as `user@host`, `built_at`
  RFC 3339 UTC), desired-side metadata: `oath set pkg:X
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
- The split (the Oath session's plan, 2026-10-31): the core **keeps the
  name `pkg:sola`** (bus, call, river, shell, session, kvm, lock, solactl,
  the Oath catalog app, and `share/`: icons, cursors — `SOLA_SHARE_DIR` and
  `XCURSOR_PATH` name `/oath/store/pkg/sola/share`, and an update can
  neither rename nor remove a package); a `pkg:sola-<app>` per app
  (terminal, projects, paint, preview, scope, monitor, calendar, mail,
  settings, bots); `sola-browser` and `sola-design` as now. In one apply
  each `/bin` name passes from the core to its app package. Apps need only
  what they load (fonts, tmux, cef), not the core — a need pins an exact
  hash, so every core overlay would rewrite every app's needs; the same for
  `sola-browser` and `sola-design` today.
- Rollout: Oath Release A (`source`, overlays shown, `oath update` makes
  objects new to the profile and checks `/bin` names before it moves
  anything) → Sola publishes the split from master → one Oath commit pins
  the set and enables `svc:sola-sessions` → Release B (`oath update
  --confirm --reboot`).
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

## Order and state

1. Spec (this), brief Oath — done. Joshua's answers to Oath's questions
   (2026-10-31): keep `pkg:sola` as the core; `oath update` makes objects
   new to the profile; it replaces overlays (named, `--keep`); Oath keeps
   committing to master; publish refuses off a pushed master, no override.
2. `sola-make` — done on branch `oath-packages`: provenance (sidecar, in
   Oath's `pkg.source` shape), the guard (`--force`), changed-only pins,
   publish only from a pushed master. Skills (`oath-packages`, `checkpoint`,
   `cleanup` — on novus, canto and buccina), Sola's `AGENTS.md`,
   `sync-project`, `sola-progress-docs`, memories — done.
3. The split in `sola-make` — done (`pkg:sola` core + 10 app packages);
   an overlay makes the new packages (`oath new`). Overlaid on canto from
   master@92815a65 (2026-10-31, generation 431): the desk restarted once,
   every `/bin` name points at its own package, Paint ran from
   `pkg:sola-paint`. Not overlaid on novus: Release B moves it, one desk
   restart instead of two. The manual's package column changes when it
   ships.
4. Oath Release A — published (desk output 940c5d970175, 2026-10-31):
   `pkg.source`, overlays shown, the update changes, its publish guard.
   buccina, novus (generation 23) and canto (generation 430) run it; the
   update kept each machine's Sola pins. `svc:sola-sessions` is in A's
   profile, so a machine that took A without it treats it as removed by
   its owner: it takes one `oath new` per machine (Release B will not
   make it); the `pkg:sola-<app>` packages Release B does make.
5. Sola published the split from master@92815a65 (2026-10-31, from
   novus; `pkg:sola` 7a03048c5a39 and 12 more, entries with their
   source handed to Oath) → Oath pins it in one commit → Release B (`oath update --confirm --reboot`, Joshua's call).
   Then `sola-make` sets `source=` with each pin (`oath set
   --from-json`).
