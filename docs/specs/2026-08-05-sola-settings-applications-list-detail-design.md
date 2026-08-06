# sola-settings Applications — compact list + detail panel

**Status:** approved. Implementation plan:
`docs/specs/2026-08-05-sola-settings-applications-list-detail-plan.md`.  
**Date:** 2026-08-05  
**Scope:** UI only inside `crates/sola-settings` (primarily `applications.rs`,
light touch in `main.rs` if page scroll must change). No bus schema changes.

## Problem

The Applications panel renders every configured app as a full multi-field
card (`app_id`, `label`, `icon`, `command` always visible). With many
entries the page is hard to scan, ordering is insertion/bus-replay order
(not alphabetical), and edit chrome dominates the list.

## Goals

1. **Compact list** of apps: name + status + Remove; open details elsewhere.
2. **Alphabetical display order** by label (fallback `app_id`).
3. **Side detail panel** for create/edit of one app at a time.
4. **No silent data loss:** dirty detail blocks selection changes until
   Save or Discard.
5. Preserve existing bus persistence, validation, candidates, and
   command-exists ("not found") behavior.

## Non-goals

- Manual reorder / persisted list order.
- Copy / duplicate actions.
- Confirm dialog on Remove (keep today's one-click Remove).
- Draggable split, modal dialog, or kit component changes.
- Changes to `Application` / `ApplicationsConfig` wire format or shell
  launcher consumption order.

## Decisions (brainstorm)

| Topic | Choice |
|---|---|
| Sort | Display-only: label A→Z, empty label falls back to `app_id`; ASCII case-insensitive (`to_ascii_lowercase` then `cmp`) |
| Layout | Fixed master–detail row (list left, detail right) — not kit `split`, not modal |
| Row actions | Open edit (row click) + Remove. No Copy |
| Dirty | Block switch: ignore other row select, Add, Configure, and Close until Save or Discard |
| Add | Opens blank draft in the detail panel (no inline draft cards above the list) |
| Candidates | Stay under the list (left column); Configure opens the detail panel prefilled |
| Config order | Unchanged on disk/bus; sort only in the view |

## UX

### List (left)

- Header area unchanged at page level ("Applications").
- **+ Add application** at the top of the list column (or immediately
  above the list card).
- One row per configured app, compact:
  - **Title:** `label` if non-empty, else `app_id`
  - **Badge:** `not found` (warning tone) when
    `!command_exists(command)` — same rule as today
  - **Remove** (danger) on the row
- Rows sorted for display only (see Sort).
- Selected row (when detail is open for that app) uses a clear active
  state (kit sidebar/list active styling or equivalent raised/hover
  treatment already used in settings).
- Empty state: short muted copy when no apps and no open draft —
  point at Add and candidates.

### Detail panel (right)

- **Always present** on the right: either the editor or an empty-state
  hint ("Select an app or add one").
- **Fixed width:** `400` px (constant; not user-resizable).
- **Edit existing:** title = current label/`app_id`; fields:
  `app_id`, `label`, `icon`, `command` (same labels/placeholders
  semantics as today). Footer: **Save** + **Discard** when dirty;
  **Close** always visible — no-op while dirty, closes when clean.
- **New draft:** title "New application"; fields blank or prefilled from
  candidate; **Add** + **Discard** (same validation as today). No
  separate Close — Discard closes a draft.
- Validation errors stay inline in the panel (caption under fields).
- On successful Save/Add: stay open on that app in clean edit mode so
  the user can tweak further without re-selecting.
- On Discard of a draft: close to empty hint. On Discard of an edit:
  reset buffer from canonical, stay open clean.

### Dirty lock

**Dirty means:**

| Mode | Dirty when |
|---|---|
| `Edit` | buffer ≠ canonical fields for `orig` |
| `Draft` | any of app_id / label / command / icon is non-empty after trim |

While dirty:

- Selecting another app: **no-op**
- **+ Add**: **no-op**
- Candidate **Configure**: **no-op**
- **Close** (edit mode only): **no-op**
- **Remove** on a *different* row: still allowed (does not change
  selection); **Remove** on the app currently open for edit: remove,
  then close panel and clear detail state

A **blank** draft (all fields empty) is not dirty: **+ Add** /
Configure / Select may replace it, and Discard still closes it.

Blocked actions are silent no-ops (no toast / dialog in v1).

### Candidates

Keep "Running, not configured" card under the list in the **left**
column (not full page width under the split). Behavior unchanged except
Configure opens the detail draft instead of inserting an inline draft
card.

## Layout structure

```
Applications (page title)
┌─────────────────────────────┬──────────────────────┐
│ [+ Add application]         │ Detail panel         │
│                             │ (or empty hint)      │
│ ┌ list rows ─────────────┐  │ app_id / label / …   │
│ │ Chrome          Remove │  │ Save Discard Close   │
│ │ Rocket.Chat     Remove │  │                      │
│ │ …                      │  │                      │
│ └────────────────────────┘  │                      │
│                             │                      │
│ ┌ Running, not configured ┐ │                      │
│ │ …              Configure│ │                      │
│ └────────────────────────┘  │                      │
└─────────────────────────────┴──────────────────────┘
```

Page scroll: prefer the **list column** scrolls independently if the list
is long; detail panel content can scroll if needed. Avoid the current
single outer scroll forcing the whole split to move as one tall stack of
cards — adjust `main.rs` only if the outer `scrollable` fights this
(e.g. Applications body fills height with an inner scroll on the list).

## State model

Replace multi-row inline edit maps with a single open editor:

```rust
enum Detail {
    Closed,
    /// Editing an existing app; `orig` is the canonical app_id at open.
    Edit { orig: String, buffer: EditBuffer },
    /// Creating a new app (blank or from candidate).
    Draft(EditBuffer),
}

struct AppsState {
    detail: Detail,
    /// Single error string for the open detail (not multi-key map).
    error: Option<String>,
}
```

Migration from current `AppsState`:

| Current | New |
|---|---|
| `drafts: Vec<DraftRow>` | at most one `Detail::Draft(EditBuffer)` |
| `edits: BTreeMap<String, EditBuffer>` | at most one `Detail::Edit` |
| `errors: BTreeMap<String, String>` | `error: Option<String>` |

Messages (illustrative):

- `Select(app_id)` — open Edit if allowed
- `StartBlank` / `StartFromCandidate { … }` — open Draft if allowed
- `CloseDetail` — only when clean
- `Field { field, value }` — mutate open buffer
- `Save` / `Discard` — commit or reset open detail
- `Remove(app_id)` — unchanged bus retract path

Bus emit/retract, `apps.add` / `apps.update` / `apps.remove`, required
field checks, and `app_id` rename retract-old semantics stay as today.

## Sorting

```rust
fn sort_key(app: &Application) -> String {
    let raw = if app.label.trim().is_empty() {
        app.app_id.as_str()
    } else {
        app.label.as_str()
    };
    raw.to_ascii_lowercase()
}
// display: sort by sort_key, then stable by app_id for ties
```

Do **not** reorder `ApplicationsConfig.apps` on save or load. Shell and
bus consumers keep insertion/replay order.

When bus replay mutates the list while an Edit is open (handled where
`Topic::Application` is applied — today `main.rs` — either by calling
into `applications::on_apps_changed` or equivalent):

- If the open `orig` was retracted externally: close detail, clear error.
- If the open app's canonical fields change under us and buffer is clean:
  refresh buffer from canonical. If dirty: keep local buffer (user owns
  the lock).

## Visual / kit

- Reuse `card`, `field`, `text_input`, `button::labeled` / danger / ghost /
  primary, `badge`, type roles, `SPACE_*` — no new pad snowflakes.
- List rows: single-line height, similar density to candidates rows today.
- Detail: same field stack as current cards (column of labeled inputs;
  command full width).
- No new kit components; no storybook page required (settings-only layout).

## Testing

- Unit-test pure helpers if extracted: `sort_key` / display order,
  dirty detection, "can switch?" guard.
- Manual smoke: open settings Applications — list alphabetical; edit;
  dirty blocks switch; Save/Discard; Add; candidate Configure; Remove
  selected and non-selected; "not found" badge; rename `app_id` still
  retracts old sticky.

## Implementation sketch

1. Introduce `Detail` + slim `AppsState`; rewrite `update` for single
   editor + dirty guard.
2. Rewrite `view`: master–detail row; compact list rows; detail form
   moved from `configured_card` / `draft_card`.
3. Drop multi-draft / multi-edit maps.
4. Tweak page scroll only if needed for fill-height list.
5. Build (`cargo make build settings` or workspace equivalent); manual
   smoke.

## Out of scope follow-ups

- Copy / duplicate
- Drag reorder + persisted order
- Remove confirmation
- Search/filter on long lists
- Icon preview in the list row
