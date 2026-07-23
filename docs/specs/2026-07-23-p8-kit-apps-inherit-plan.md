# P8 — Kit apps inherit

> **For agentic workers:** One signature move per pass; worktrees only;
> `cargo make build` — never install without express user permission.
>
> **Parent roadmap:** `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` §4 P8  
> **North star:** `docs/manual/design-language.md`  
> **P7 closed:** `docs/specs/2026-07-21-p7-kit-controls-plan.md` (primitives ready)

**Goal:** Settings, monitor, terminal chrome, agent, and browser chrome
**consume** kit tokens and helpers — no per-app themes, no local hex/pad
snowflakes for shared chrome, no reinvented button/field density.

**Architecture:** Apps only. Kit primitives already exist (`button::labeled`,
`field` / `form_row`, type roles, `PAD_CONTROL*`, selection atom, quiet
ghost). Promote a constant to kit only if two apps need the same snowflake.

## Global constraints

- Worktrees under `.worktrees/` only; merge to master only with approval.
- Build, do not install, unless the user asks for that install.
- One surface (app) per pass unless the user expands scope.
- Prefer kit helpers over local `button(text(...)).padding(...).size(...)`.
- Prefer `style::{SPACE_*, PAD_CONTROL*}` over raw `12.0` / `Padding::new(6)`.
- Prefer type roles (`text::heading` / `subheading` / `body` / `caption`) over
  bare `.size(N)` for UI chrome (content areas may keep domain sizes).
- Terminal ANSI palette and browser page content stay domain-owned; only
  **chrome** inherits kit.
- Do not re-open P7 kit primitives unless a real density/binding bug surfaces.

## Out of scope

| Item | Why |
|------|-----|
| New kit widgets | Only if an app is blocked; prefer compose |
| Shell layout rewrites | P3–P6 done |
| Blur / vibrancy | Roadmap deferred |
| Per-app theme forks | Explicitly forbidden in P8 |

## Pass overview

| Pass | Signature move | App(s) |
|------|----------------|--------|
| **A** | Settings inherits labeled buttons + type roles + SPACE scale | sola-settings |
| **B** | Monitor inherits selection atom + type roles (drop row hex) | sola-monitor |
| **C** | Agent chrome density (`labeled`, type roles, card/plain where fit) | sola-agent |
| **D** | Terminal chrome only (sidebar/menu density; not ANSI grid) | sola-terminal |
| **E** | Browser chrome inherits kit helpers | sola-browser-core |
| **F** | Docs + roadmap closeout | docs |

Each pass = one worktree branch, one mergeable unit, visual stop if chrome
changed visibly.

---

## Pass A — Settings inherits kit density

**Signature move:** Every settings control uses kit density helpers instead
of hand-rolled pad/size snowflakes.

**Files:**
- `crates/sola-settings/src/main.rs` — page title → `text::heading`
- `crates/sola-settings/src/applications.rs` — `labeled` / type roles / SPACE_*
- `crates/sola-settings/src/mail.rs` — same

**Replace patterns:**

| Before (snowflake) | After (kit) |
|--------------------|-------------|
| `button(text("Save").size(13)).style(primary).padding(Padding::new(6).left(12)…)` | `kit_btn::labeled("Save", kit_btn::primary)` |
| compact ghost / Remove in dense rows | `kit_btn::labeled_sm(...)` when 12px density fits |
| `text(...).size(28)` page title | `kit_text::heading(...)` |
| section `size(16)` + medium font | `kit_text::subheading(...)` |
| body `size(13)` | `kit_text::body(...)` |
| helper `size(12)` muted | `kit_text::caption(...).style(muted)` (11) or body muted if weight matters |
| validation `size(12)` muted | `kit_text::caption(...).style(danger)` |
| `text_input(...).padding(Padding::new(6).left(10)…)` | omit padding → kit `DEFAULT_PADDING` |
| `FIELD_GAP = 12` / `CARD_GAP = 16` | `style::SPACE_LG` / `SPACE_XL` |
| `field(label, input, None, None)` + separate error line | pass `error` into `field` when it's a field-level error; card-level errors stay caption danger |

**Do not:**
- Change bus/save semantics or panel structure
- Force `form_row` on stacked field layouts (stacked `field` is correct)
- Touch other apps

### Acceptance

- [x] No hand-rolled control padding on settings buttons
- [x] Page/section/body/caption use type roles (or documented exception)
- [x] Text inputs use kit default padding
- [x] Gaps use `SPACE_*`
- [x] `cargo make build settings` (or full build) succeeds

### Commit

`feat(settings): inherit kit labeled buttons, type roles, and spacing`

---

## Pass B — Monitor inherits selection + type

**Signature move:** Selected-row chrome uses `theme::selection()` (or kit
list style), not `#1c2129`. Headers/rows use type roles where chrome.

JSON syntax colors in the detail pane may stay domain syntax theme (not
product chrome) — optional later pass to tokenise if desired.

**Files:** `crates/sola-monitor/src/main.rs`

### Commit

`feat(monitor): quiet selection atom and kit type roles`

---

## Pass C — Agent chrome density

**Signature move:** Agent chrome uses `button::labeled` / type roles /
consistent pads; no raw size+pad on shared actions.

**Files:** `crates/sola-agent/src/view/*`

### Commit

`feat(agent): inherit kit button density and type roles`

---

## Pass D — Terminal chrome only

**Signature move:** Sidebar / menu chrome density via kit; leave
`term_view` ANSI grid palette domain-owned (already maps many ANSI slots
from atoms).

**Files:** `crates/sola-terminal/src/sidebar.rs`, `menu.rs`, maybe `main.rs`
chrome — **not** full `term_view` palette rewrite.

### Commit

`feat(terminal): kit density for chrome, leave grid domain palette`

---

## Pass E — Browser chrome

**Signature move:** Browser iced chrome (tabs, URL bar chrome, toolbar)
uses kit helpers; page content stays engine-owned.

**Files:** `crates/sola-browser-core/` (and thin wrappers if any)

### Commit

`feat(browser): kit helpers for chrome surfaces`

---

## Pass F — Docs + handoff

- Roadmap P8 checklist complete
- `active-work.md` → next initiative or `none`
- design-language redesign order mark item 7 done if all app passes landed

### Commit

`docs: close P8 kit apps inherit`

---

## Success criteria for “P8 done”

1. Settings has no control pad/size snowflakes for buttons/fields.
2. Monitor selected rows use selection atom (or kit list style).
3. Agent / terminal chrome / browser chrome use kit labeled + type roles.
4. No app introduces a local theme builder or palette fork for chrome.
5. Roadmap + active-work closed cleanly.

---

## Suggested worktree names

```
.worktrees/p8a-settings-inherit
.worktrees/p8b-monitor-inherit
.worktrees/p8c-agent-inherit
.worktrees/p8d-terminal-chrome
.worktrees/p8e-browser-chrome
.worktrees/p8f-docs-handoff
```
