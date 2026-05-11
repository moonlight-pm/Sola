# Clippy lint configuration — Design

**Date:** 2026-05-11
**Branch:** TBD (project-wide; pick up after `sola-kit-preact` lands)
**Scope:** Workspace-wide Clippy policy + per-crate inheritance + test relaxations.

## Problem

Sola has no Clippy configuration today. Root `Cargo.toml` has no
`[workspace.lints]` table; no crate sets `lints.workspace = true`;
there is no `clippy.toml`. Clippy runs with its built-in defaults
(`correctness`, `suspicious`, `style`, `complexity`, `perf` at warn —
nothing else). That's a low bar for a project where:

- Most code is written by an AI agent and reviewed by one person.
  Lints catch classes of mistakes that experienced humans would
  intuit; the agent does not have that intuition.
- The compositor is a single long-lived process. A surprise
  `panic!` or `.unwrap()` panic on a corner-case Wayland message
  takes the whole desktop down.
- We have async/event-loop code with shared state. `await`-holding-
  lock and lost-future bugs are exactly the kind of thing that's
  invisible in review and catastrophic at runtime.
- The codebase is still small enough that adopting a strict policy
  now is cheap; postponing it gets more expensive every week.

## Reference reading

Two recent posts framed the decision:

- **emschwartz.me, "Your Clippy config should be stricter"** — argues
  for a *curated* lint set organized by failure mode (Don't Panic,
  Don't Fail Silently, Don't Do Bad Async Stuff, Don't Do Unsafe
  Memory Stuff, Easy-to-Avoid Mistakes, Deliberate Suppression).
  Explicitly rejects enabling whole categories: Clippy's own docs
  warn against blanket `restriction` enablement (its lints
  contradict each other), and `pedantic` produces hundreds of
  warnings per crate of mixed value. Calls out AI coding agents as
  exactly the workflow that benefits most.
- **billylevin.dev, "Clippy config"** — opposite stance: enable
  `pedantic` + `restriction` (and optionally `nursery`) wholesale
  with `priority` ordering, then `#[expect(lint, reason = "…")]`
  the ones that don't fit. Allowlist mindset: every lint visible
  by default, suppression deliberate.

## Decision

**Adopt Schwartz's curated approach, with one Levin idea grafted on.**

The full reasoning is in the chat thread; the short version:

1. With ~13 crates of careful hand-built work, triaging the
   hundreds of `restriction`/`pedantic` warnings up front is a
   session of pure noise filtering. The curated set solves the
   actual failure modes (panics, lost futures, await-holding-lock,
   silent error swallowing) without that overhead.
2. The "agents lack intuition" framing maps directly to Sola's
   development model: agent writes, user reviews. Lints that catch
   panic-prone patterns *before* review save real time.
3. Schwartz's `allow_attributes` + `allow_attributes_without_reason`
   pair gives us Levin's deliberate-suppression discipline without
   the bulk allowlist. Every `#[expect(lint, reason = "…")]`
   documents why; every accidental `#[allow]` warns. This is the
   single most valuable idea in either article and it costs us
   nothing to take.
4. `pedantic` is a strict superset over time — we can add it later
   in a dedicated triage session. The reverse (start with pedantic,
   prune it) is not cheaper, it's just more work front-loaded.

## Lint set

Grouped by failure mode. All as `warn` unless noted.

### Don't Panic

Catches sources of process death.

- `clippy::unwrap_used`
- `clippy::expect_used`
- `clippy::panic`
- `clippy::unreachable`
- `clippy::todo`
- `clippy::unimplemented`
- `clippy::indexing_slicing`
- `clippy::string_slice`
- `clippy::arithmetic_side_effects` *(consider — may be noisy in
  compositor scaling math; gate on first triage)*

Rationale: every one of these is a panic source. In tests they're
fine; in the compositor or session manager they're a desktop crash.

### Don't Fail Silently

Catches discarded results, swallowed errors, lost futures.

- `clippy::let_underscore_must_use`
- `clippy::let_underscore_future`
- `clippy::map_err_ignore`
- `clippy::missing_errors_doc` *(consider — currently we don't doc
  every error; may want to defer)*

The future variant is critical for async code — `let _ = some_fut;`
silently drops a future that was supposed to run.

### Don't Do Bad Async Stuff

Catches async footguns.

- `clippy::await_holding_lock`
- `clippy::await_holding_refcell_ref`
- `clippy::future_not_send` *(consider — may conflict with our
  single-threaded event-loop design where Send is irrelevant)*

### Don't Do Unsafe Memory Stuff

We do have `unsafe` blocks (CEF FFI, Smithay's raw GL paths). They
need to be deliberate, not casual.

- `clippy::undocumented_unsafe_blocks`
- `clippy::multiple_unsafe_ops_per_block`
- `clippy::transmute_ptr_to_ptr`
- `clippy::ptr_as_ptr`

### Easy-to-Avoid Mistakes

Cheap signal-to-noise.

- `clippy::float_cmp`
- `clippy::float_cmp_const`
- `clippy::dbg_macro` (deny in release; we sometimes use `dbg!`
  during local dev, so `warn` is fine — CI catches the leak)
- `clippy::print_stdout` / `clippy::print_stderr` *(consider —
  `println!` is fine in tests and in the `sola` process manager's
  startup path; gate on first triage)*
- `clippy::mem_forget`
- `clippy::lossy_float_literal`

### Numeric casts *(deferred)*

`cast_sign_loss`, `cast_possible_truncation`, `cast_precision_loss`,
`cast_possible_wrap`. Schwartz recommends these. Sola has little
numeric code outside compositor scaling and they're noisy. **Skip
in v1**; revisit when compositor math grows or we hit a numeric
bug that one of these would have caught.

### Deliberate Suppression (the Levin idea)

- `clippy::allow_attributes` — bans bare `#[allow(...)]`; forces
  `#[expect(...)]` (which warns if the lint stops firing,
  cleaning itself up over time).
- `clippy::allow_attributes_without_reason` — every suppression
  must carry a `reason = "…"`.

These two are load-bearing for the whole policy. Without them,
`#[allow]` becomes a graveyard of stale suppressions. With them,
the codebase tells us why every exception exists and prunes itself.

## Mechanics

### Root `Cargo.toml`

```toml
[workspace.lints.clippy]
# Don't Panic
unwrap_used                = "warn"
expect_used                = "warn"
panic                      = "warn"
unreachable                = "warn"
todo                       = "warn"
unimplemented              = "warn"
indexing_slicing           = "warn"
string_slice               = "warn"

# Don't Fail Silently
let_underscore_must_use    = "warn"
let_underscore_future      = "warn"
map_err_ignore             = "warn"

# Don't Do Bad Async Stuff
await_holding_lock         = "warn"
await_holding_refcell_ref  = "warn"

# Don't Do Unsafe Memory Stuff
undocumented_unsafe_blocks    = "warn"
multiple_unsafe_ops_per_block = "warn"
transmute_ptr_to_ptr          = "warn"
ptr_as_ptr                    = "warn"

# Easy-to-Avoid Mistakes
float_cmp           = "warn"
float_cmp_const     = "warn"
dbg_macro           = "warn"
mem_forget          = "warn"
lossy_float_literal = "warn"

# Deliberate Suppression
allow_attributes                = "warn"
allow_attributes_without_reason = "warn"
```

### Every crate's `Cargo.toml`

```toml
[lints]
workspace = true
```

One-line inheritance per crate. Add to every crate in `crates/`.

### `clippy.toml`

```toml
# Tests can panic; they're allowed to use the convenient APIs.
allow-unwrap-in-tests   = true
allow-expect-in-tests   = true
allow-panic-in-tests    = true
allow-indexing-slicing-in-tests = true
allow-print-in-tests    = true
allow-dbg-in-tests      = true
```

### CI

Whatever lint-check job runs in CI, invoke with `-D warnings` so
warns become hard errors. Locally Clippy stays `warn` so devs (and
the agent) aren't blocked mid-flow on every save.

## Rollout

The Schwartz bundle will produce a wall of warnings on the existing
code on day one — `unwrap()` calls in compositor init, `panic!`s in
unreachable arms, `dbg!`s left over from debugging sessions. Two
options:

1. **Bang-out triage** — one PR. Apply the config, fix everything,
   land green. Probably a half-day to full-day session. Clear cutover.
2. **Warn-only burn-down** — apply the config (still `warn`), don't
   enforce in CI yet, fix the warnings across follow-up commits,
   flip CI to `-D warnings` once the count is zero.

**Recommendation: option 1.** Sola is small enough that the
half-day pays for itself once. Option 2 leaves a long tail of
warnings that everyone learns to tune out, which defeats the
point. The one exception: any suppression that turns out to be
load-bearing (e.g. an `unwrap()` we genuinely can't prove safe yet)
gets `#[expect(clippy::unwrap_used, reason = "…")]` with a real
reason, not a hand-wave.

## Open questions

These are flagged for the implementing session, not now:

- **`arithmetic_side_effects`** — useful in principle, possibly
  noisy in compositor scaling math. Decide after first triage.
- **`print_stdout` / `print_stderr`** — the `sola` process manager
  legitimately prints startup info. Decide whether to enable +
  `#[expect]` the startup path, or leave off.
- **`future_not_send`** — our event loops are single-threaded; this
  lint may produce signal-free noise. Likely skip.
- **`missing_errors_doc`** — would be nice; not free. Decide if we
  want to commit to documenting every error variant.
- **`pedantic`** — defer to a later session, deliberately.
- **`nursery`** — Levin's article suggests it. Skip; nursery lints
  are nursery for a reason (unstable, false-positive prone).

## Non-goals

- Restyling the codebase. The point is correctness-class lints, not
  bikeshed formatting. `style` is already on by default.
- Adopting `restriction` wholesale. Its lints contradict each other
  by design; we cherry-pick.
- Adding `#![deny(...)]` per crate. Workspace inheritance is the
  single source of truth.

## When to do this

After the `sola-kit-preact` worktree merges. The kit work is
generating new code rapidly and adding new lints mid-stream would
mean fighting Clippy on every commit. Once the kit lands and the
codebase is briefly stable, do the triage pass cold.
