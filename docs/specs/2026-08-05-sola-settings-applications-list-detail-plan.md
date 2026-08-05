# Applications list + detail panel — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/specs/2026-08-05-sola-settings-applications-list-detail-design.md`

**Goal:** Replace the always-expanded Applications cards with a compact alphabetical list and a fixed-width side detail panel for create/edit, with a dirty lock so unsaved edits cannot be abandoned by switching rows.

**Architecture:** Single-editor UI state (`Detail::{Closed,Edit,Draft}`) in `applications.rs`. Display-only sort for the list. Master–detail `row![list | detail]` at fixed 400px detail width. Bus emit/retract and `ApplicationsConfig` APIs unchanged. External sticky replay reconciled via `on_apps_changed` called from `main.rs`.

**Tech Stack:** Rust, iced 0.14, `sola-kit` components (`card`, `field`, `text_input`, `button`, `badge`, `text`), `sola-bus` / `sola-core` application types.

## Global Constraints

- **UI only** in `crates/sola-settings` (`applications.rs`, light `main.rs`). No bus schema changes.
- **No `cargo make install`.** Verify with `cargo make build` and `cargo test -p sola-settings --bin sola-settings` (or workspace-equivalent). User smokes UI manually.
- **No kit component API work** and no storybook page updates.
- **Worktree rule:** if starting from a clean master checkout, implement in a `.worktrees/` branch — this plan assumes the existing `naturalethic/sola-settings-improvement` branch is fine to continue on.
- Dirty lock, blank-draft rules, sort, and 400px detail width are defined in the spec — copy those rules exactly; do not invent confirm dialogs or copy/duplicate.

---

## File Structure

```
crates/sola-settings/src/
  applications.rs   # REWRITE state, update, view; add pure helpers + unit tests
  main.rs           # AFTER Application topic apply: call on_apps_changed;
                    # optional: Applications body fill-height (no outer scroll fight)
  procfs.rs         # unchanged
  mail.rs           # unchanged
```

No new crates or files unless `applications.rs` exceeds ~700 lines and a
small `applications/detail.rs` split becomes clearer — prefer one file
for this feature.

---

### Task 1: Pure helpers + unit tests

**Files:**
- Modify: `crates/sola-settings/src/applications.rs`
- Test: same file, `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `fn display_title(app: &Application) -> &str` — label if non-empty after trim, else `app_id`
  - `fn sort_key(app: &Application) -> String` — `display_title` lowercased ASCII
  - `fn sorted_apps(apps: &ApplicationsConfig) -> Vec<&Application>` — sorted by `sort_key`, tie-break `app_id`
  - `fn draft_is_dirty(buf: &EditBuffer) -> bool` — any field non-empty after trim
  - `fn edit_is_dirty(buf: &EditBuffer, canonical: &Application) -> bool` — `!buf.matches(canonical)` (keep existing `EditBuffer::matches`)
  - `fn can_leave_detail(detail: &Detail, apps: &ApplicationsConfig) -> bool` — true when Closed, blank draft, or edit buffer clean vs canonical for `orig`

- [ ] **Step 1: Read current `applications.rs` end-to-end** (state, `EditBuffer`, view helpers) so renames do not fight existing names.

- [ ] **Step 2: Write failing unit tests** at the bottom of `applications.rs` (or temporarily under `#[cfg(test)]` before types exist — if compile fails because `Detail` is missing, define a minimal `Detail` enum first in Step 3 then re-run).

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::Application;

    fn app(id: &str, label: &str) -> Application {
        Application {
            app_id: id.into(),
            label: label.into(),
            command: "true".into(),
            icon: String::new(),
        }
    }

    #[test]
    fn display_title_prefers_nonempty_label() {
        assert_eq!(display_title(&app("x", "Chrome")), "Chrome");
        assert_eq!(display_title(&app("x", "  ")), "x");
        assert_eq!(display_title(&app("x", "")), "x");
    }

    #[test]
    fn sorted_apps_orders_case_insensitive_by_label() {
        let mut cfg = ApplicationsConfig::default();
        cfg.apps = vec![
            app("z", "chrome"),
            app("a", "Bitwarden"),
            app("m", "Signal"),
        ];
        let ids: Vec<&str> = sorted_apps(&cfg).iter().map(|a| a.app_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "z", "m"]); // Bitwarden, chrome, Signal
    }

    #[test]
    fn draft_dirty_when_any_field_nonempty() {
        assert!(!draft_is_dirty(&EditBuffer::default()));
        assert!(draft_is_dirty(&EditBuffer {
            label: "x".into(),
            ..Default::default()
        }));
    }

    #[test]
    fn can_leave_blank_draft_but_not_dirty_edit() {
        let mut cfg = ApplicationsConfig::default();
        let a = app("chrome", "Chrome");
        cfg.apps.push(a.clone());

        assert!(can_leave_detail(&Detail::Closed, &cfg));
        assert!(can_leave_detail(
            &Detail::Draft(EditBuffer::default()),
            &cfg
        ));
        assert!(!can_leave_detail(
            &Detail::Draft(EditBuffer {
                command: "x".into(),
                ..Default::default()
            }),
            &cfg
        ));

        let clean = EditBuffer::from_app(&a);
        assert!(can_leave_detail(
            &Detail::Edit {
                orig: "chrome".into(),
                buffer: clean.clone(),
            },
            &cfg
        ));
        let mut dirty = clean;
        dirty.label = "Nope".into();
        assert!(!can_leave_detail(
            &Detail::Edit {
                orig: "chrome".into(),
                buffer: dirty,
            },
            &cfg
        ));
    }
}
```

- [ ] **Step 3: Implement minimal helpers + `Detail` enum** needed for tests (do not rewrite `update`/`view` yet). Keep old `AppsState` compiling by leaving it in place temporarily, **or** switch `AppsState` to the new shape with `Default` so `main.rs` still builds:

```rust
#[derive(Debug, Clone, Default)]
pub enum Detail {
    #[default]
    Closed,
    Edit {
        orig: String,
        buffer: EditBuffer,
    },
    Draft(EditBuffer),
}

#[derive(Default)]
pub struct AppsState {
    pub detail: Detail,
    pub error: Option<String>,
}

pub fn display_title(app: &Application) -> &str {
    if app.label.trim().is_empty() {
        app.app_id.as_str()
    } else {
        app.label.as_str()
    }
}

pub fn sort_key(app: &Application) -> String {
    display_title(app).to_ascii_lowercase()
}

pub fn sorted_apps(apps: &ApplicationsConfig) -> Vec<&Application> {
    let mut v: Vec<&Application> = apps.apps.iter().collect();
    v.sort_by(|a, b| {
        sort_key(a)
            .cmp(&sort_key(b))
            .then_with(|| a.app_id.cmp(&b.app_id))
    });
    v
}

pub fn draft_is_dirty(buf: &EditBuffer) -> bool {
    !buf.app_id.trim().is_empty()
        || !buf.label.trim().is_empty()
        || !buf.command.trim().is_empty()
        || !buf.icon.trim().is_empty()
}

pub fn edit_is_dirty(buf: &EditBuffer, canonical: &Application) -> bool {
    !buf.matches(canonical)
}

pub fn can_leave_detail(detail: &Detail, apps: &ApplicationsConfig) -> bool {
    match detail {
        Detail::Closed => true,
        Detail::Draft(buf) => !draft_is_dirty(buf),
        Detail::Edit { orig, buffer } => match apps.get(orig) {
            Some(canonical) => !edit_is_dirty(buffer, canonical),
            // Canonical gone — treat as free to leave (on_apps_changed will close).
            None => true,
        },
    }
}
```

If this breaks existing `update`/`view` that still use `drafts`/`edits`/`errors`, either:
- **Option A (preferred for this task):** keep old fields **and** new fields until Task 2 removes old ones; tests only exercise new helpers; or
- **Option B:** stub `update`/`view` to compile with new state only (empty list + empty detail) for one commit — only if Option A is messier.

Prefer **Option A**: add new types/helpers without deleting old state yet; tests compile against new helpers.

- [ ] **Step 4: Run unit tests**

```bash
cargo test -p sola-settings --bin sola-settings -- helpers -- --nocapture
# or full binary tests:
cargo test -p sola-settings --bin sola-settings
```

Expected: all new tests PASS. Fix sort expected order if `Application` normalize mutates fields.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-settings/src/applications.rs
git commit -m "test(settings): helpers for app list sort and dirty lock"
```

---

### Task 2: Rewrite `update` for single-detail editor

**Files:**
- Modify: `crates/sola-settings/src/applications.rs` (`AppsMsg`, `update`, remove multi-draft/edit maps)

**Interfaces:**
- Consumes: `Detail`, `AppsState`, `can_leave_detail`, `draft_is_dirty`, `EditBuffer`
- Produces: message enum and `update` behavior matching the table below

**`AppsMsg` (final):**

```rust
#[derive(Debug, Clone)]
pub enum AppsMsg {
    Select(String),
    StartBlank,
    StartFromCandidate {
        app_id: String,
        command: Option<String>,
    },
    Field {
        field: AppField,
        value: String,
    },
    Save,       // edit: commit; draft: add
    Discard,    // edit: reset buffer; draft: close
    CloseDetail,
    Remove(String),
}
```

**Behavior table:**

| Msg | When dirty | Action |
|---|---|---|
| `Select(id)` | blocked if `!can_leave` | `Detail::Edit { orig: id, buffer: from apps.get }` if present |
| `StartBlank` | blocked if `!can_leave` | `Detail::Draft(EditBuffer::default())`, clear error |
| `StartFromCandidate` | blocked if `!can_leave` | `Detail::Draft` with app_id=label=app_id, command=suggested or `""`, icon `""` |
| `Field` | n/a | mutate open buffer; clear `error` |
| `Save` on Edit | n/a | same validation + `apps.update` + rename retract + emit as today; on success set `Detail::Edit { orig: new_id, buffer: from saved }` clean; clear error |
| `Save` on Draft | n/a | same as today's `DraftCommit`; on success open `Edit` on new app clean |
| `Discard` Edit | n/a | reset buffer from canonical; clear error; stay open |
| `Discard` Draft | n/a | `Detail::Closed`; clear error |
| `CloseDetail` | no-op if dirty | if edit clean → Closed; if draft blank → Closed |
| `Remove(id)` | always | if open Edit for `id`, close detail; `apps.remove` + retract as today |

Remove: `DraftRow`, `DRAFT_SEQ`, multi-key error maps, `EditField { orig, … }` / `EditSave(orig)` / per-draft keys.

- [ ] **Step 1: Replace `AppsMsg` and rewrite `update`** to the behavior table. Keep `emit`/`retract` helpers. Required-field error string remains: `"app_id, label, and command are required"`.

Example skeleton for the dirty guard:

```rust
pub fn update(
    msg: AppsMsg,
    apps: &mut ApplicationsConfig,
    ui: &mut AppsState,
) -> Task<AppsMsg> {
    match msg {
        AppsMsg::Select(id) => {
            if !can_leave_detail(&ui.detail, apps) {
                return Task::none();
            }
            if let Some(a) = apps.get(&id) {
                ui.detail = Detail::Edit {
                    orig: id,
                    buffer: EditBuffer::from_app(a),
                };
                ui.error = None;
            }
        }
        AppsMsg::StartBlank => {
            if !can_leave_detail(&ui.detail, apps) {
                return Task::none();
            }
            ui.detail = Detail::Draft(EditBuffer::default());
            ui.error = None;
        }
        // ... StartFromCandidate, Field, Save, Discard, CloseDetail, Remove
        _ => {}
    }
    Task::none()
}
```

For `Save` on Edit, reuse the existing rename logic (retract old sticky when `app_id` changes, then emit new). On success:

```rust
let new_app = buf.to_application();
// ... apps.update + emit ...
ui.detail = Detail::Edit {
    orig: new_app.app_id.clone(),
    buffer: EditBuffer::from_app(&new_app),
};
ui.error = None;
```

- [ ] **Step 2: Temporary view stub** so the crate still compiles if old view helpers referenced removed msgs — either delete `configured_card`/`draft_card` now and return a one-line placeholder body, or finish Task 3 in the same working tree before committing. **Do not leave a non-compiling tree.**

Preferred: if Task 2 and Task 3 will be one session, implement both then one commit; if separate commits, Task 2 must end with a compiling stub:

```rust
pub fn view<'a>(
    _apps: &'a ApplicationsConfig,
    _running: &'a [BusWindow],
    _ui: &'a AppsState,
) -> Element<'a, AppsMsg> {
    kit_text::body("Applications UI rebuild in progress").into()
}
```

- [ ] **Step 3: Build**

```bash
cargo make build
# or at least:
cargo build -p sola-settings
```

Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-settings/src/applications.rs
git commit -m "refactor(settings): single-detail Applications editor state"
```

---

### Task 3: Master–detail view

**Files:**
- Modify: `crates/sola-settings/src/applications.rs` (`view` and helpers)

**Interfaces:**
- Consumes: `sorted_apps`, `display_title`, `Detail`, `AppsMsg`
- Produces: full master–detail UI per spec

**Constants:**

```rust
const DETAIL_WIDTH: f32 = 400.0;
```

**Layout:**

```rust
pub fn view<'a>(
    apps: &'a ApplicationsConfig,
    running: &'a [BusWindow],
    ui: &'a AppsState,
) -> Element<'a, AppsMsg> {
    let list = list_column(apps, running, ui);
    let detail = detail_panel(apps, ui);
    row![list, detail]
        .spacing(SPACE_XL)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
```

**List column:**
1. `+ Add application` → `AppsMsg::StartBlank`
2. Empty muted text if no apps
3. For each `sorted_apps(apps)`: compact row
4. Candidates card (existing content, but under list)

**Compact row:**
- Title: `display_title(app)`
- Badge `not found` when `!sola_core::applications::command_exists(&app.command)`
- `Remove` danger → `AppsMsg::Remove`
- Clicking the row body (not Remove) → `AppsMsg::Select(app_id)`
- Selected when `matches!(ui.detail, Detail::Edit { orig, .. } if orig == app.app_id)`
- Style: prefer `kit_btn::list_item(selected)` for the title hit target so selection reads clearly; Remove remains a separate danger button so it does not also fire Select.

Pattern (avoid Select when pressing Remove by not nesting Remove inside the Select button):

```rust
row![
    kit_btn::labeled(title, kit_btn::list_item(selected))
        .on_press(AppsMsg::Select(app.app_id.clone()))
        .width(Length::Fill),
    // badge if missing — place before Remove or after title inside a row with Space::Fill
    kit_btn::labeled("Remove", kit_btn::danger)
        .on_press(AppsMsg::Remove(app.app_id.clone())),
]
```

If `list_item` + `labeled` needs a custom label row (title + badge), use `button(content).style(kit_btn::list_item(selected)).on_press(Select)` with a `row![text, optional badge]` as content — follow existing kit patterns in shell/agent list rows if present.

**Detail panel:**
- `container(...).width(Length::Fixed(DETAIL_WIDTH)).height(Length::Fill)`
- `Detail::Closed` → card or plain column with muted "Select an app or add one"
- `Detail::Edit` → title, four fields via `Field` msgs, error caption, footer: if dirty show Save + Discard; always show Close → `CloseDetail`
- `Detail::Draft` → "New application", four fields (placeholders like today: firefox / Firefox / simpleicons/firefox / firefox), error, Add (`Save`) + Discard

Field wiring:

```rust
text_input(placeholder, value)
    .on_input(move |v| AppsMsg::Field { field: f, value: v })
    .size(13)
    .style(kit_input::style)
    .width(Length::Fill);
```

Use `field(label, input, None, None)` as today.

- [ ] **Step 1: Implement `list_column`, `app_row`, `detail_panel`, `candidates_card` (adapt Configure to `StartFromCandidate`).** Delete obsolete `configured_card` / `draft_card` / per-orig `edit_text_input`.

- [ ] **Step 2: Build**

```bash
cargo build -p sola-settings
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-settings/src/applications.rs
git commit -m "feat(settings): compact Applications list with side detail panel"
```

---

### Task 4: Bus replay reconciliation

**Files:**
- Modify: `crates/sola-settings/src/applications.rs` — add `pub fn on_apps_changed(apps: &ApplicationsConfig, ui: &mut AppsState)`
- Modify: `crates/sola-settings/src/main.rs` — call it after applying `Topic::Application`

**Behavior (`on_apps_changed`):**

```rust
pub fn on_apps_changed(apps: &ApplicationsConfig, ui: &mut AppsState) {
    match &ui.detail {
        Detail::Closed | Detail::Draft(_) => {}
        Detail::Edit { orig, buffer } => {
            match apps.get(orig) {
                None => {
                    ui.detail = Detail::Closed;
                    ui.error = None;
                }
                Some(canonical) if !edit_is_dirty(buffer, canonical) => {
                    // Refresh clean buffer from canonical (external update).
                    let refreshed = EditBuffer::from_app(canonical);
                    ui.detail = Detail::Edit {
                        orig: orig.clone(),
                        buffer: refreshed,
                    };
                }
                Some(_) => {
                    // Dirty — keep local buffer.
                }
            }
        }
    }
}
```

Note: after `remove` + conditional `push` in `main.rs`, call `on_apps_changed` so retract of the open app closes the panel.

```rust
Some(Topic::Application(app)) => {
    self.applications.remove(&app.app_id);
    if message.sticky {
        self.applications.apps.push(app);
    }
    applications::on_apps_changed(&self.applications, &mut self.apps_ui);
}
```

- [ ] **Step 1: Add unit test** for retract-while-edit-open closes detail:

```rust
#[test]
fn on_apps_changed_closes_edit_when_app_removed() {
    let mut cfg = ApplicationsConfig::default();
    let a = app("chrome", "Chrome");
    cfg.apps.push(a.clone());
    let mut ui = AppsState {
        detail: Detail::Edit {
            orig: "chrome".into(),
            buffer: EditBuffer::from_app(&a),
        },
        error: None,
    };
    cfg.remove("chrome");
    on_apps_changed(&cfg, &mut ui);
    assert!(matches!(ui.detail, Detail::Closed));
}
```

- [ ] **Step 2: Implement `on_apps_changed` + wire `main.rs`**

- [ ] **Step 3: Run tests + build**

```bash
cargo test -p sola-settings --bin sola-settings
cargo build -p sola-settings
```

Expected: PASS / success.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-settings/src/applications.rs crates/sola-settings/src/main.rs
git commit -m "fix(settings): reconcile Applications detail on bus replay"
```

---

### Task 5: Page layout / scroll

**Files:**
- Modify: `crates/sola-settings/src/main.rs` (`view`) if the outer `scrollable` prevents fill-height master–detail

**Intent:** List and detail should fill the main pane height; list column scrolls internally when long.

- [ ] **Step 1: Adjust Applications layout**

Option A — panel-specific: only wrap Mail (or non-Applications) in the outer scrollable; Applications body uses `height(Fill)`:

```rust
let main_inner: Element<'_, Msg> = match self.panel {
    Panel::Applications => {
        column![
            kit_text::heading(title_text),
            applications::view(...).map(Msg::Apps),
        ]
        .spacing(page_pad)
        .padding(Padding::new(page_pad))
        .height(Length::Fill)
        .into()
    }
    Panel::Mail => scrollable(
        column![
            kit_text::heading(title_text),
            mail::view(...).map(Msg::Mail),
        ]
        .spacing(page_pad)
        .padding(Padding::new(page_pad)),
    )
    .height(Length::Fill)
    .width(Length::Fill)
    .into(),
};
```

Option B — keep outer scroll if Option A is awkward; put `scrollable` only around the list of app rows inside `list_column`. Prefer Option A if height Fill works.

Inside `list_column`, wrap the sorted rows in `scrollable(...).height(Length::Fill)`.

- [ ] **Step 2: Build**

```bash
cargo build -p sola-settings
```

- [ ] **Step 3: Commit**

```bash
git add crates/sola-settings/src/main.rs crates/sola-settings/src/applications.rs
git commit -m "fix(settings): fill-height Applications master-detail scroll"
```

(Skip this commit if Step 1 determines no `main.rs` change is needed — then only commit applications scroll if any.)

---

### Task 6: Final verification

**Files:** none new

- [ ] **Step 1: Full build**

```bash
cargo make build
```

Expected: success. **Do not install.**

- [ ] **Step 2: Unit tests**

```bash
cargo test -p sola-settings --bin sola-settings
```

Expected: all PASS.

- [ ] **Step 3: Manual smoke checklist** (user or agent with desktop access)

1. Open Settings → Applications: list is alphabetical by label.
2. Click a row: detail opens with fields; list shows selection.
3. Edit a field: Save/Discard appear; selecting another row no-ops.
4. Discard: fields reset; Close works when clean.
5. Save with rename of `app_id`: entry updates; no duplicate stickies.
6. + Add: blank draft; empty draft can be replaced by Select; typed draft blocks switch; Add commits and stays open as Edit.
7. Candidate Configure: prefilled draft in detail.
8. Remove selected: row gone, detail closes; Remove other: row gone, selection unchanged.
9. Missing binary: `not found` badge on row.
10. Mail panel still scrolls/works.

- [ ] **Step 4: Update design doc status** (optional one-liner)

In `docs/specs/2026-08-05-sola-settings-applications-list-detail-design.md`, set status to `implemented` when smoke passes.

```bash
git add docs/specs/2026-08-05-sola-settings-applications-list-detail-design.md
git commit -m "docs: mark applications list-detail design implemented"
```

---

## Spec coverage checklist

| Spec requirement | Task |
|---|---|
| Compact list (name, badge, Remove) | 3 |
| Sort display-only A→Z label / app_id | 1, 3 |
| Side detail 400px | 3 |
| Dirty lock / blank draft rules | 1, 2 |
| Add → draft in panel | 2, 3 |
| Candidates under list, Configure → draft | 3 |
| Save stays open clean Edit | 2 |
| Discard draft closes; discard edit resets | 2 |
| Bus schema unchanged | all |
| on_apps_changed for external retract/update | 4 |
| Independent list scroll / fill height | 5 |
| Unit tests for sort + dirty | 1, 4 |
| No copy / no reorder / no kit changes | non-goals |

---

## Self-review notes

- No TBD placeholders; widths and dirty rules are concrete.
- `AppsMsg::Save` serves both draft commit and edit save — footer labels differ in the view only ("Add" vs "Save").
- `EditBuffer::from_app` / `matches` / `to_application` retained from current code.
- `main.rs` Application handler must call `on_apps_changed` after every sticky apply/retract of that topic.
