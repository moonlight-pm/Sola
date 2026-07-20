# sola-kit Hardening — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (or equivalent) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Work in `.worktrees/` only; never commit directly on `master` without explicit user permission. **Never** `cargo make install` — build only (`cargo make build` / isolated crate builds).

**Goal:** Finish the post-audit cleanup of the iced `sola-kit`: fix remaining theme/quit consumer bugs, scrub stale docs, harden bus scaffolding, and reduce maintainability debt — without porting legacy stacks or inventing speculative kit APIs.

**Architecture:** Prefer thin shared helpers already in `sola_kit::app` (`apply_theme_update`, `is_self_quit`) over hand-rolled theme arms in each app. Keep kit growth demand-driven. Mechanical refactors (module splits, docs) ship separately from correctness fixes so each PR is reviewable alone.

**Tech Stack:** Rust 2024, iced 0.14 (wgpu/wayland), `sola-kit`, `sola-bus`, `sola-core`. Build via `cargo make build` (or isolated crate path for iced consumers).

**Date:** 2026-07-19  
**Status:** Ready for execution  
**Supersedes / continues:** `docs/specs/2026-05-29-sola-kit-audit-and-cleanup-design.md` (most workstreams landed; this plan is residual + new findings)

---

## Global constraints

1. **No install.** Never run `cargo make install` or copy binaries to `/opt/sola/bin/`. Verify with build (+ unit tests). User installs and smoke-tests from a TTY.
2. **Worktrees only.** Code changes in `.worktrees/…`. Do not commit on `master` unless the user explicitly says so for that commit.
3. **No speculative widgets.** Do not add components, tokens, or a generic `run::<A>()` unless a task below requires it.
4. **Theme side-effects stay explicit.** `theme_from_bus` remains pure. Fonts + selection install only via `apply_theme_update` (or an equally explicit paired call).
5. **Design language** (`docs/manual/design-language.md`) is the visual north star for any chrome/style touch — macOS dark mode density, tokens first, no new hex in views.

---

## Explicit non-goals (do not touch)

| Path / crate | Why out of scope |
|--------------|------------------|
| `apps/*` | Deprecated frontends (agent/mail web stubs, etc.). Not on iced kit. |
| `crates/sola-app` | Legacy GTK4 + WebKit6 host. Frozen. Historical name confusion: this was the *old* app framework; iced `sola-kit` replaced the CEF kit, not this crate's entire role overnight. **Do not port, rewrite, or "convert" it.** |
| Apps / crates **not** depending on `sola-kit` | e.g. pure bus clients, `sola-session`, `sola-river`, `solactl`, process manager. No kit migration work. |
| Materials / vibrancy redesign of shell chrome | Product design pass; listed only as a **deferred** phase below. |
| Replacing the forked `text_input` with iced upstream | Keep the fork until a dedicated edit-parity plan exists. Only touch TODOs if a task explicitly says so. |
| Porting any remaining non-kit UI to iced | Out of scope. Only fix consumers **already** on `sola-kit`. |

### Active iced / sola-kit consumers (in scope for consumer fixes)

| Crate | Role | Theme path today | Quit path today |
|-------|------|------------------|-----------------|
| `sola-kit` (storybook) | Dogfood + theme editor | Correct (manual + selection) | own `"quit"` match |
| `sola-monitor` | Bus inspector | **Incomplete** (`theme_from_bus` + fonts; **no** `install_selection`) | `is_self_quit` ✓ |
| `sola-settings` | Settings | **Incomplete** (same) | **Hand-rolled** quit/`CloseApp` |
| `sola-shell` | Desktop shell | **Incomplete** (theme + fonts + `ShellStyle`; **no** selection) | special (shutdown, not app quit) — leave |
| `sola-terminal` | Terminal | `apply_theme_update` ✓ | `is_self_quit` ✓ |
| `sola-agent` | Agent UI | `apply_theme_update` ✓ | `is_self_quit` ✓ |
| `sola-browser-core` (+ cef/wpe engines) | Browser | `apply_theme_update` ✓ | `is_self_quit` ✓ |

Only the incomplete rows need consumer edits in Phase A.

---

## Background — what already landed (do not re-do)

From the May 2026 audit, already shipped in tree:

- **W1 theme schema:** `ATOM_BINDINGS`, `Atoms` hub, seed ≈ kit hex (`accent #00d4ff`, etc.), `warning` + `selection` + font roles, pure `theme_from_bus`
- **W2 partial:** single bus poller guard, poison recovery, 8 ms poll still present
- **W3 helpers:** `apply_theme_update`, `is_self_quit`, `QUIT_ACTION_ID`; dead `App` trait / placeholder `run` removed
- **W4 partial:** `style` module, conventions in `components/mod.rs`, `icon_handle` cache, `confirm_button`
- **W5:** color picker, icon page, `Page::ALL` exhaustiveness tests, shell storybook page
- **W6 as-needed:** `number_input`, `readable`, `fontdb` system families, titlebar + `FloatState`

**Headline residual bug:** `apply_theme_update` installs `selection`, but monitor / settings / shell rebuild themes without it. Editing the storybook `selection` atom will not recolour those apps' sidebar active rows until process restart.

---

## File map (expected touch sites)

### Phase A — correctness

| File | Change |
|------|--------|
| `crates/sola-monitor/src/main.rs` | Use `apply_theme_update`; drop manual `theme_from_bus` + fonts install |
| `crates/sola-settings/src/main.rs` | Use `apply_theme_update` + `is_self_quit` |
| `crates/sola-shell/src/app/bus.rs` | After theme apply, also `install_selection` (and keep `ShellStyle`) |

### Phase B — docs (no runtime behavior)

| File | Change |
|------|--------|
| `crates/sola-kit/src/lib.rs` | Fix crate docs (live theme is shipped) |
| `crates/sola-kit/Cargo.toml` | Drop stale `run::<A>()` claim |
| `docs/vault/sola-kit.md` | Status + roadmap refresh for iced kit |
| `CLAUDE.md` | Only if still contradictory after lib.rs fix (prefer minimal edit) |

### Phase C — bus scaffolding

| File | Change |
|------|--------|
| `crates/sola-kit/src/app.rs` | Log connect failure; optional notify-fd stretch |

### Phase D — maintainability (optional splits; independent PRs)

| File | Change |
|------|--------|
| `crates/sola-kit/src/components/sidebar.rs` | Split into modules (nav / panel / tabs) if/when touched |
| `crates/sola-kit/src/fonts.rs` | Doc/layout cleanup only |
| `crates/sola-kit/src/storybook/mod.rs` | Only if adding pages; no big rewrite required |

### Phase E — deferred product work (documented, not scheduled)

- Design-language materials / token-driven radius-space
- Storybook page-header redesign (atom-editing design §5)
- Generic single-window `run` helper
- FloatState correlation beyond title string
- text_input horizontal scroll TODO

---

## Dependency / execution order

```
A1 monitor theme  ─┐
A2 settings theme+quit ─┼─► independent, can parallel
A3 shell selection ────┘
         │
         ▼
B docs (after A so docs describe reality)
         │
         ▼
C1 connect logging (tiny)
C2 notify-fd (stretch; separate PR; may defer)
         │
         ▼
D splits (only if size pain; each independent)
E never auto-starts — product decision
```

Suggested subagent batching:

1. **PR1:** A1 + A2 + A3 (one branch: "consumer theme/selection correctness")
2. **PR2:** B (docs)
3. **PR3:** C1 (and C2 only if time/risk accepted)
4. **PR4+:** D as optional follow-ups

---

## Phase A — Consumer correctness

### Task A1: sola-monitor uses `apply_theme_update`

**Files:**
- Modify: `crates/sola-monitor/src/main.rs`

**Why:** Monitor installs theme + fonts but not `selection`. Sidebars that use `theme::selection()` stay on the process default until restart.

**Current (broken pattern):**

```rust
if let Some(Topic::Theme(bus_theme)) = &parsed {
    self.theme = theme_from_bus(bus_theme);
    sola_kit::fonts::install(sola_kit::theme::fonts_from_bus_theme(bus_theme));
}
```

**Target:**

```rust
use sola_kit::app::{BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings};
// …
// In Msg::BusMessage:
apply_theme_update(&message, &mut self.theme);
// keep float.update, is_self_quit, message log logic unchanged
```

- [ ] **Step 1:** Update imports: add `apply_theme_update`; remove unused `theme_from_bus` if nothing else needs it (keep `default_theme` / `parse as hex` if still used).
- [ ] **Step 2:** Replace the `Topic::Theme` arm with a single `apply_theme_update(&message, &mut self.theme);` (call early in the bus arm; return value may be ignored if other bus handling continues).
- [ ] **Step 3:** Confirm `is_self_quit` remains the quit path (already correct).
- [ ] **Step 4:** Build monitor:

```bash
cargo make build sola-monitor
```

Expected: success. (If make target naming differs, use the isolated iced crate build path already used for monitor.)

- [ ] **Step 5:** Commit on the worktree branch:

```bash
git add crates/sola-monitor/src/main.rs
git commit -m "fix(sola-monitor): apply_theme_update so selection atom reloads live"
```

---

### Task A2: sola-settings uses `apply_theme_update` + `is_self_quit`

**Files:**
- Modify: `crates/sola-settings/src/main.rs`

**Why:** Same selection gap as monitor; quit path reimplements menu/`CloseApp` with magic `"quit"` string instead of `QUIT_ACTION_ID` / `is_self_quit`.

**Current quit (replace):**

```rust
let our_quit = matches!(
    &parsed,
    Some(Topic::MenuAction(MenuActionPayload { app_id, action_id }))
        if app_id == APP_ID && action_id == "quit"
);
let close_us = matches!(
    &parsed,
    Some(Topic::CloseApp(app_id)) if app_id == APP_ID
);
// … later exit if our_quit || close_us
```

**Target bus arm sketch:**

```rust
use sola_kit::app::{BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings};

// In Msg::BusMessage:
apply_theme_update(&message, &mut self.theme);

if is_self_quit(&message, APP_ID) {
    return iced::exit(); // or whatever settings already uses for quit (preserve existing exit mechanism)
}

// Keep Application / MailConfig handling via Topic::parse as today
```

- [ ] **Step 1:** Add `apply_theme_update` and `is_self_quit` to imports; drop `theme_from_bus` if unused; drop `MenuActionPayload` if only used for quit matching.
- [ ] **Step 2:** Replace theme block with `apply_theme_update`.
- [ ] **Step 3:** Replace dual quit matches with `is_self_quit`; **preserve** the existing exit `Task` / `iced::exit` / process exit behavior settings already uses — only change the *predicate*.
- [ ] **Step 4:** Build:

```bash
cargo make build sola-settings
```

Expected: success.

- [ ] **Step 5:** Commit:

```bash
git add crates/sola-settings/src/main.rs
git commit -m "fix(sola-settings): live selection via apply_theme_update; is_self_quit for exit"
```

---

### Task A3: sola-shell installs selection on theme update

**Files:**
- Modify: `crates/sola-shell/src/app/bus.rs` (`on_theme`)

**Why:** Shell needs `ShellStyle` refresh (not in `apply_theme_update`). It must still install selection (and may keep explicit font install or compose helpers).

**Current:**

```rust
fn on_theme(&mut self, t: BusTheme) {
    self.theme = sola_kit::theme::theme_from_bus(&t);
    self.style = sola_kit::theme::shell_style_from_bus_theme(&t);
    sola_kit::fonts::install(sola_kit::theme::fonts_from_bus_theme(&t));
}
```

**Target (explicit pairing — do not hide ShellStyle):**

```rust
fn on_theme(&mut self, t: BusTheme) {
    self.theme = sola_kit::theme::theme_from_bus(&t);
    self.style = sola_kit::theme::shell_style_from_bus_theme(&t);
    sola_kit::fonts::install(sola_kit::theme::fonts_from_bus_theme(&t));
    sola_kit::theme::install_selection(
        sola_kit::theme::atoms_from_bus_theme(&t).selection,
    );
}
```

Alternatively, if a small kit helper is preferred (only if both shell and a second multi-part consumer need it later):

```rust
// NOT required this plan — YAGNI unless duplication reappears.
// pub fn apply_theme_parts(bus: &BusTheme) -> (Theme, Fonts, Color) { … }
```

**Do not** route shell quit through `is_self_quit` — shell maps quit to session shutdown deliberately.

- [ ] **Step 1:** Add `install_selection` call as above.
- [ ] **Step 2:** Build:

```bash
cargo make build sola-shell
```

Expected: success.

- [ ] **Step 3:** Commit:

```bash
git add crates/sola-shell/src/app/bus.rs
git commit -m "fix(sola-shell): install selection atom on Topic::Theme"
```

---

### Task A4: Smoke checklist for Phase A (human / agent notes only)

No code. After A1–A3 land, the user can verify:

1. Run storybook, create/edit a non-default theme, change **selection** color, Save (or live-edit depending on dirty model).
2. Open settings / monitor — active sidebar row should track the new selection without restart.
3. Shell surfaces that use selection (if any kit sidebars in-process) update; launcher/switcher still follow shell tokens.

Agents must **not** install or launch sola on a TTY.

---

## Phase B — Documentation truth

### Task B1: Fix `sola-kit` crate docs + Cargo.toml blurb

**Files:**
- Modify: `crates/sola-kit/src/lib.rs`
- Modify: `crates/sola-kit/Cargo.toml` (package description / top comments only)

**Replace false claims** in `lib.rs` module docs:

| Remove / rewrite | With |
|------------------|------|
| "Wiring iced apps to the live bus theme is a v0.2 task; today every kit app reads the hardcoded default at startup." | Live bus theme is the steady state: apps store `Theme`, subscribe via `bus_subscription`, apply with `apply_theme_update` (or shell's `on_theme` + `ShellStyle`). `default_theme()` is the pre-replay / offline default. |
| "shared with the legacy kit and the WebView apps" | Prefer: bus theme is process-wide via `Topic::Theme`; iced consumers map via `theme_from_bus`. Legacy WebView (`sola-app`) is frozen and out of kit scope. |
| Canonical first consumer | List current active consumers briefly: monitor, settings, shell, terminal, agent, browser-core, storybook. |

**Cargo.toml:** remove any mention of `sola_kit::run::<A>()` as the entrypoint; describe scaffolding as `startup` + `BusSetup` + app-owned iced builder.

- [ ] **Step 1:** Edit `lib.rs` docs to match reality (keep concise; mirror CLAUDE.md kit section tone).
- [ ] **Step 2:** Edit `Cargo.toml` package comments.
- [ ] **Step 3:** No build required for docs-only, but `cargo make build sola-kit` is fine if desired.
- [ ] **Step 4:** Commit:

```bash
git add crates/sola-kit/src/lib.rs crates/sola-kit/Cargo.toml
git commit -m "docs(sola-kit): correct crate docs for live theme + scaffolding"
```

---

### Task B2: Refresh vault note `docs/vault/sola-kit.md`

**Files:**
- Modify: `docs/vault/sola-kit.md`

**Content requirements:**

1. Status line: iced kit is production path; CEF/Remix kit **removed**.
2. Component inventory updated (include: `color_picker`, `number_input`, `readable`, `titlebar`, `spectrum`, `icon`, `FloatState`, forked `text_input`, shell tokens / `ShellStyle`).
3. Roadmap section: mark completed items; point residual work at **this plan** (`2026-07-19-sola-kit-hardening-plan.md`).
4. Explicit: `sola-app` is the **legacy GTK/WebKit** host (not iced). `apps/*` deprecated. Do not confuse with iced `sola-kit`.
5. Font story: system fonts via `ensure_system_fonts` / fontconfig; Inter + JetBrains Mono defaults (not bundled SF Pro narrative if outdated).

- [ ] **Step 1:** Rewrite outdated sections; do not invent APIs.
- [ ] **Step 2:** Commit:

```bash
git add docs/vault/sola-kit.md
git commit -m "docs(vault): refresh sola-kit iced status and inventory"
```

---

### Task B3: Cross-link from the May audit design

**Files:**
- Modify: `docs/specs/2026-05-29-sola-kit-audit-and-cleanup-design.md` (header only)

Add under the title:

```markdown
**Status (2026-07-19):** Most workstreams landed. Residual work and consumer
selection fixes are tracked in
`docs/specs/2026-07-19-sola-kit-hardening-plan.md`.
```

- [ ] **Step 1:** Add status pointer.
- [ ] **Step 2:** Commit with B2 or alone:

```bash
git add docs/specs/2026-05-29-sola-kit-audit-and-cleanup-design.md
git commit -m "docs(specs): point sola-kit audit residual work at 2026-07-19 plan"
```

---

## Phase C — Bus scaffolding

### Task C1: Log `BusSetup::install` connect outcome

**Files:**
- Modify: `crates/sola-kit/src/app.rs` (`BusSetup::install`)

**Current:** `client.connect_blocking(self.connect_timeout);` result discarded. Menu emit already logs errors.

**Target:**

```rust
pub fn install(self) {
    let mut client = BusClient::new();
    match client.connect_blocking(self.connect_timeout) {
        Ok(()) => {
            tracing::info!(app_id = self.app_id, "bus connected");
        }
        Err(e) => {
            // Still proceed: apps must start even if bus is briefly down
            // (sticky replay / reconnect behavior is bus-client's job).
            // Never silent — "never lose output".
            tracing::warn!(
                app_id = self.app_id,
                error = %e,
                "bus connect_blocking failed; continuing without guaranteed bus"
            );
        }
    }
    // … subscribe + menu as today …
}
```

Confirm `connect_blocking` actual return type in `sola-bus` before coding (Ok unit vs bool vs Result). Match real signature; do not invent.

- [ ] **Step 1:** Read `BusClient::connect_blocking` signature in `crates/sola-bus/src/client.rs`.
- [ ] **Step 2:** Log success/failure without changing control flow unless API already aborts.
- [ ] **Step 3:** Unit tests in `app.rs` unchanged; build kit:

```bash
cargo make build sola-kit
# or: cargo test --manifest-path crates/sola-kit/Cargo.toml apply_theme
```

- [ ] **Step 4:** Commit:

```bash
git add crates/sola-kit/src/app.rs
git commit -m "fix(sola-kit): log BusSetup connect_blocking outcome"
```

---

### Task C2 (stretch): Replace 8 ms poll with notify-fd wake

**Status:** Optional. Higher risk. Ship as its own PR or skip until idle CPU matters.

**Files:**
- Modify: `crates/sola-kit/src/app.rs` (`bus_stream`)
- Read: `crates/sola-bus/src/client.rs` — `notify_fd`, `try_clone_notify`, `drain_notify`

**Intent:** Keep single-poller + poison recovery. Instead of `sleep(8ms)` on empty queue, block on the notify pipe (or register with iced/async).

**Acceptance:**

- One subscription still works.
- Second `bus_subscription` still refuses with warn + empty stream.
- Dropping the subscription stops the thread.
- No message loss vs poll path under bursty traffic.
- No `install`.

**Do not start C2 in the same PR as A/B/C1.**

---

## Phase D — Maintainability (optional, demand-triggered)

Only run these when actively editing the module, or as a dedicated cleanup PR after A–C.

### Task D1: Split `sidebar.rs` without behavior change

**Files:**
- Create: `crates/sola-kit/src/components/sidebar/mod.rs` (re-exports)
- Create: `crates/sola-kit/src/components/sidebar/nav.rs` (plain `sidebar` + items)
- Create: `crates/sola-kit/src/components/sidebar/panel.rs` (`SidebarPanel`, drag/reorder)
- Create: `crates/sola-kit/src/components/sidebar/tabs.rs` (`vertical_tabs*`, `TabSize`)
- Modify: `crates/sola-kit/src/components/mod.rs` — re-exports **unchanged** for consumers

**Rules:**

- Public API surface of `components::{sidebar, SidebarItem, …}` must not break.
- Move tests with the code they cover.
- No visual changes; storybook Sidebar page still builds.

- [ ] **Step 1:** Mechanical move; `mod.rs` re-export.
- [ ] **Step 2:** `cargo make build sola-kit` + `cargo test --manifest-path crates/sola-kit/Cargo.toml sidebar`
- [ ] **Step 3:** Commit: `refactor(sola-kit): split sidebar into nav/panel/tabs modules`

---

### Task D2: fonts.rs hygiene

**Files:**
- Modify: `crates/sola-kit/src/fonts.rs`

- Fix duplicated doc comments (`Build a Fonts table…` twice).
- Fix jammed layout (`}fn medium`, `}/// Family names…`) with normal newlines.
- No behavior change.

- [ ] Commit: `chore(sola-kit): fonts.rs doc/layout cleanup`

---

### Task D3: Launcher icon caching audit (shell only)

**Files:**
- Read: `crates/sola-shell/src/launcher/view.rs` (uses `icon(name, 24)` — disk read per call)

**If** launcher list re-renders every frame with many apps, switch to store `svg::Handle` via `icon_handle` in launcher state and render with `icon_svg`.

- Only change if profiling/reason exists; otherwise leave a one-line comment near the call site pointing at `icon_handle`.
- **Do not** change browser/agent icons "just because."

---

## Phase E — Deferred (document only; no implementation in this plan)

| Item | Notes | Unblock when |
|------|-------|--------------|
| Materials / translucency kit-wide | Design language §2.2; shell already has alpha `shell-*` tokens | Explicit visual redesign pass + screenshots |
| Bus-driven `space-*` / `radius-*` | Seed has tokens; kit uses compile-time `SPACE_*` / `RADIUS_*` | Settings wants editable density, or design says so |
| Storybook page-header redesign | Atom-editing design §5 deferred | UI polish pass |
| Generic `run::<A>()` / font builder combinator | Still YAGNI; helpers cover quit/theme | If main() boilerplate becomes painful again |
| FloatState stronger correlation | Title-based matching fragile | Multi-window same-title bugs reported |
| text_input horizontal scroll TODO | Fork maintenance | Input overflow bugs |
| Clipboard Edit menu free for all kit apps | Browser already has multi-menu | Second app needs same free handlers |

---

## Testing matrix

| Check | Command / method | Phase |
|-------|------------------|-------|
| Kit unit tests | `cargo test --manifest-path crates/sola-kit/Cargo.toml` | A (if kit changed), C, D |
| Monitor build | `cargo make build sola-monitor` | A1 |
| Settings build | `cargo make build sola-settings` | A2 |
| Shell build | `cargo make build sola-shell` | A3 |
| Kit build | `cargo make build sola-kit` | B–D |
| Live selection | User: storybook edit selection → open settings | A (manual) |
| Quit | User: Cmd+Q settings/monitor; CloseApp from shell | A (manual) |

---

## Review checklist (orchestrator / human)

Before calling the work done:

- [ ] No changes under `apps/`
- [ ] No changes under `crates/sola-app` (unless a typo-level comment — prefer leave frozen)
- [ ] Every iced consumer either uses `apply_theme_update` **or** documents why not and still calls `install_selection`
- [ ] Shell still refreshes `ShellStyle`
- [ ] Shell quit behavior unchanged
- [ ] Docs no longer claim live theme is future work
- [ ] No `cargo make install` in any agent log
- [ ] Commits live on worktree branches; merge only with user permission

---

## Subagent handoff notes

**Session entry:** `.grok/rules/active-work.md` is auto-loaded and points here.
Any informal go-ahead without a new task means execute this plan from the
phase listed under Active work's **Next**.

When starting a fresh session:

1. Read `active-work.md` → this plan + `CLAUDE.md` install/worktree rules.
2. Prefer **PR1 = Phase A (all three apps)** in one worktree branch.
3. Prefer **PR2 = Phase B** docs.
4. Prefer **PR3 = C1**; leave C2 unless asked.
5. Skip D/E unless the user expands scope.
6. Canonical good consumers for copy-paste patterns: `crates/sola-agent/src/main.rs` and `crates/sola-browser-core/src/integration.rs` (theme + quit).
7. After each phase, update `.grok/rules/active-work.md` so the next session stays accurate.

### Do not

- Convert `apps/mail`, `apps/agent`, or `sola-app` to iced.
- Re-open the May audit W1 theme table rewrite (already done).
- Add new kit components in this plan.
- Force-push or merge to master without permission.

---

## Success criteria

1. Editing the bus `selection` atom updates sidebar highlights in **monitor**, **settings**, and **shell** without restart.
2. Settings quit uses the same predicate as other kit apps (`is_self_quit`).
3. Crate/vault docs match iced kit reality and clearly separate `sola-app` / `apps/*` as non-kit.
4. Bus connect failures are visible in logs.
5. Optional splits leave public APIs stable and tests green.

---

## Appendix — audit residual mapping

| Audit ID | Disposition in this plan |
|----------|---------------------------|
| A1–A8 theme schema | Done previously |
| B1 poller race / lifetime | Done previously (guard + exit on close) |
| B2 poison | Done previously |
| B3 notify-fd | Task C2 stretch |
| B5 connect log | Task C1 |
| C1 main() hoist / run() | Deferred E |
| C2 theme helper | Task A* (consumers) |
| C3 quit helper | Task A2 (settings residual) |
| D1–D11 components | Mostly done; D1 optional split |
| E1–E5 storybook | Mostly done; page header → E |
| F1–F5 legacy gaps | Done or deferred with product |

---

*End of plan.*
