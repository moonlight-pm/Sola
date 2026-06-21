# Browser chrome density, cmd-click new tab, and edit menu — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land three browser improvements — less-compressed chrome (a `large`
sidebar-tab variant + more nav padding), cmd/middle-click opening a link in a
background tab (WPE + CEF), and an Edit menu (copy/cut/paste/select-all/undo/redo)
routed to whichever surface is focused.

**Architecture:** Shared logic lives in `sola-browser-core` behind the existing
`Cmd<E>` / `Engine` abstraction; engine-specific work is confined to
`sola-browser-wpe` (WebKit C bindings + worker) and `sola-browser-cef` (CEF
handlers). The tab-size variant lives in `sola-kit`. Pure logic (intent mapping,
focus routing, the cmd-click predicate, editing-command names, tab metrics) is
unit-tested; engine FFI and visual density are build-verified + user smoke-tested.

**Tech Stack:** Rust 2024, iced 0.14 (wgpu/wayland), WPEWebKit via bindgen C
bindings, CEF via the `cef` crate `147.1.0+147.0.10`, sola-bus IPC.

Design spec: `docs/specs/2026-06-21-browser-chrome-and-edit-features-design.md`.

## Global Constraints

- **NEVER run `cargo make install` (or any variant).** Build-verify only. This
  applies to every task; if a step says "smoke", that smoke run is the *user's*
  to perform — do not install. (Project CLAUDE.md.)
- **Compile-verify with `cargo make build`** — never raw `cargo build` or `cp`.
  Run unit tests with `cargo test -p <crate>`.
- **Builds must be warning-free.** A new warning is a task failure.
- **Both engines stay at parity.** Every cross-engine feature (cmd-click, edit
  command, web-view-focus publish) lands in *both* `sola-browser-wpe` and
  `sola-browser-cef` in the same task.
- **`sola-browser-core` depends on NO web-engine library** — keep WebKit/CEF
  symbols out of it. Shared logic is engine-agnostic.
- **Prefer Serena symbolic tools** for reading/editing code files.
- Commit trailer on every commit:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`

---

## File Structure

| File | Responsibility | Tasks |
| --- | --- | --- |
| `crates/sola-kit/src/components/sidebar.rs` | `TabSize` enum, `TabMetrics`, `vertical_tabs_sized` | 1 |
| `crates/sola-kit/src/components/mod.rs` | export `TabSize` | 1 |
| `crates/sola-kit/src/storybook/pages/sidebar.rs` | dogfood Normal + Large tabs | 1 |
| `crates/sola-browser-core/src/app.rs` | chrome density consts/views; `url_bar_focused`; `Msg::WebViewFocused`/`UrlPasted`; run_intent clipboard | 2, 5 |
| `crates/sola-browser-core/src/engine.rs` | `EditCmd`, `Cmd::Edit` | 3 |
| `crates/sola-browser-core/src/util.rs` | `editing_command_name`, `is_new_tab_click` | 3, 8 |
| `crates/sola-browser-core/src/integration.rs` | `BrowserIntent::Edit`, edit action ids, `EDIT_MENU_ITEMS`, `intent_for_menu_action` arms, `EditTarget`/`edit_target`, run_intent Edit arm | 3, 4, 5 |
| `crates/sola-browser-core/src/run.rs` | publish the Edit menu | 6 |
| `crates/sola-kit/src/app.rs` | `BusSetup` multi-menu (`app_menu_more`) | 6 |
| `crates/sola-browser-wpe/src/engine.rs` | `Cmd::Edit` arm; decide-policy → background tab; thread `next_id` | 3, 8 |
| `crates/sola-browser-wpe/src/frame.rs` | publish `Msg::WebViewFocused` | 7 |
| `crates/sola-browser-cef/src/engine.rs` | `Cmd::Edit` arm; `on_before_popup` → background tab; thread `next_id` | 3, 9 |
| `crates/sola-browser-cef/src/frame.rs` | publish `Msg::WebViewFocused` | 7 |

---

## Task 1: sola-kit `TabSize` variant for vertical tabs

**Files:**
- Modify: `crates/sola-kit/src/components/sidebar.rs` (the `vertical_tabs` fn at lines 166-231, plus add `TabSize`/`TabMetrics`)
- Modify: `crates/sola-kit/src/components/mod.rs:66-71` (re-export block)
- Modify: `crates/sola-kit/src/storybook/pages/sidebar.rs` (add a density demo)

**Interfaces:**
- Produces: `pub enum TabSize { Normal, Large }` (derives `Debug, Clone, Copy, PartialEq, Eq, Default`; `Normal` is `#[default]`); `pub fn vertical_tabs_sized<'a, Message, FHover>(tabs, hovered, on_hover, size: TabSize) -> Container<'a, Message, Theme>` (same bounds as `vertical_tabs`). `vertical_tabs(..)` keeps its exact current signature and now delegates to `vertical_tabs_sized(.., TabSize::Normal)`.
- Consumes: `SPACE_XS` (=2.0) and `SPACE_SM` (=4.0) from `crate::components::style` (already imported).

- [ ] **Step 1: Write the failing test for `TabSize::metrics`**

Add to the existing `mod tests` in `crates/sola-kit/src/components/sidebar.rs`:

```rust
#[test]
fn tab_size_metrics_are_stable() {
    let n = TabSize::Normal.metrics();
    assert_eq!((n.row_pad_v, n.row_pad_h, n.font, n.close), (6, 10, 13, 15));
    assert_eq!(n.gap, SPACE_XS);

    let l = TabSize::Large.metrics();
    assert_eq!((l.row_pad_v, l.row_pad_h, l.font, l.close), (10, 12, 14, 17));
    assert_eq!(l.gap, SPACE_SM);

    assert_eq!(TabSize::default(), TabSize::Normal);
}
```

- [ ] **Step 2: Run it to confirm it fails to compile**

Run: `cargo test -p sola-kit tab_size_metrics_are_stable`
Expected: FAIL — `cannot find type TabSize`.

- [ ] **Step 3: Add `TabSize` + `TabMetrics`, and `SPACE_SM` to the import**

In `crates/sola-kit/src/components/sidebar.rs`, change the style import at line 29:

```rust
use crate::components::style::{RADIUS_SM, SPACE_SM, SPACE_XS};
```

Insert directly above `vertical_tabs` (before its doc comment at line ~155):

```rust
/// Size variant for [`vertical_tabs_sized`]. `Normal` reproduces the
/// historical density; `Large` is the roomier browser-chrome variant.
/// This is the kit's canonical size-variant pattern — copy it for other
/// components that grow a size knob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabSize {
    #[default]
    Normal,
    Large,
}

/// Resolved per-size metrics. Values are deliberate, not derived.
struct TabMetrics {
    row_pad_v: u16,
    row_pad_h: u16,
    font: u16,
    close: u16,
    gap: f32,
}

impl TabSize {
    fn metrics(self) -> TabMetrics {
        match self {
            TabSize::Normal => TabMetrics { row_pad_v: 6, row_pad_h: 10, font: 13, close: 15, gap: SPACE_XS },
            TabSize::Large => TabMetrics { row_pad_v: 10, row_pad_h: 12, font: 14, close: 17, gap: SPACE_SM },
        }
    }
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cargo test -p sola-kit tab_size_metrics_are_stable`
Expected: PASS.

- [ ] **Step 5: Refactor `vertical_tabs` to delegate, add `vertical_tabs_sized`**

Replace the body of `vertical_tabs` (lines 166-231) so it delegates, and add the sized form that threads the metrics into the hard-coded sites. Keep the doc comment on `vertical_tabs`. The new `vertical_tabs_sized` is the old body with the four literals replaced by metrics:

```rust
pub fn vertical_tabs<'a, Message, FHover>(
    tabs: Vec<TabDescriptor<Message>>,
    hovered: Option<usize>,
    on_hover: FHover,
) -> Container<'a, Message, Theme>
where
    Message: Clone + 'a,
    FHover: Fn(Option<usize>) -> Message + 'a,
{
    vertical_tabs_sized(tabs, hovered, on_hover, TabSize::Normal)
}

/// Size-parameterized [`vertical_tabs`]. `TabSize::Large` is the roomier
/// browser-chrome density.
pub fn vertical_tabs_sized<'a, Message, FHover>(
    tabs: Vec<TabDescriptor<Message>>,
    hovered: Option<usize>,
    on_hover: FHover,
    size: TabSize,
) -> Container<'a, Message, Theme>
where
    Message: Clone + 'a,
    FHover: Fn(Option<usize>) -> Message + 'a,
{
    let m = size.metrics();
    let mut col = column![].spacing(m.gap).padding(Padding::from([8, 6]));
    for (i, tab) in tabs.into_iter().enumerate() {
        let TabDescriptor { label, active, on_activate, on_close } = tab;

        let activate = button(
            text(label)
                .font(fonts::ui())
                .size(m.font)
                .wrapping(Wrapping::None),
        )
        .style(move |t, status| item_style(t, status, active))
        .padding(Padding::from([m.row_pad_v, m.row_pad_h]))
        .width(Length::Fill)
        .on_press(on_activate);

        let row_el: Element<'a, Message> = if hovered == Some(i) {
            let close = button(text("×").font(fonts::ui()).size(m.close))
                .style(|t, status| item_style(t, status, false))
                .padding(Padding::from([0, 7]))
                .on_press(on_close);
            stack![
                activate,
                container(close)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Center)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(Padding::from([0, 4])),
            ]
            .into()
        } else {
            activate.into()
        };

        col = col.push(
            mouse_area(row_el)
                .on_enter(on_hover(Some(i)))
                .on_exit(on_hover(None)),
        );
    }

    container(col).style(style).height(Length::Fill).width(Length::Fill)
}
```

Note `Padding::from([m.row_pad_v, m.row_pad_h])` takes `[u16; 2]`; `m.font`/`m.close` are `u16` and feed `.size(..)` which accepts `impl Into<Pixels>` (u16 works).

- [ ] **Step 6: Export `TabSize`**

In `crates/sola-kit/src/components/mod.rs`, add `TabSize` and `vertical_tabs_sized` to the `sidebar::{...}` re-export (lines 66-71):

```rust
pub use sidebar::{
    PANEL_HEADER_H, PANEL_REORDER_THRESHOLD, PANEL_ROW_H, PANEL_W_DEFAULT, PANEL_W_MAX,
    PANEL_W_MIN, ReorderCfg, SIDEBAR_WIDTH, SidebarItem, SidebarPanel, SidebarSection,
    TabDescriptor, TabSize, panel_dragged_width, panel_drop_index, panel_drop_index_relative,
    panel_renumber_changed, panel_reordered, sidebar, vertical_tabs, vertical_tabs_sized,
};
```

- [ ] **Step 7: Dogfood both sizes in the storybook Sidebar page**

In `crates/sola-kit/src/storybook/pages/sidebar.rs`, add a density demo below the existing panel demo. Add the import (extend line 15-17's `components::{..}` use with `TabDescriptor, TabSize, vertical_tabs_sized`), then add this helper and splice its result into the `column![...]` in `view`:

```rust
fn density_demo<'a>() -> Element<'a, Msg> {
    let mk = |active_i: usize| -> Vec<TabDescriptor<Msg>> {
        ["Inbox", "A long tab title that truncates", "Sent"]
            .into_iter()
            .enumerate()
            .map(|(i, l)| TabDescriptor::new(l, i == active_i, Msg::ItemPress(i), Msg::Noop))
            .collect()
    };
    row![
        column![
            body("Normal").style(muted),
            container(vertical_tabs_sized(mk(0), None, |_| Msg::Noop, TabSize::Normal))
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(160.0)),
        ]
        .spacing(8),
        column![
            body("Large").style(muted),
            container(vertical_tabs_sized(mk(0), None, |_| Msg::Noop, TabSize::Large))
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(160.0)),
        ]
        .spacing(8),
    ]
    .spacing(24)
    .into()
}
```

Then add `density_demo()` to the page's top-level `column![...]` (after the `demo` element, before the trailing `code(...)`), and add a one-line `body("vertical_tabs density: Normal vs Large").style(muted)` heading above it. `row`, `column`, `container`, `body`, `muted`, `Length` are already imported in this file.

- [ ] **Step 8: Build the whole workspace and run kit tests**

Run: `cargo make build`
Expected: builds clean, no warnings.
Run: `cargo test -p sola-kit`
Expected: all pass (including `tab_size_metrics_are_stable`).

- [ ] **Step 9: Commit**

```bash
git add crates/sola-kit/src/components/sidebar.rs crates/sola-kit/src/components/mod.rs crates/sola-kit/src/storybook/pages/sidebar.rs
git commit -m "feat(sola-kit): TabSize Normal/Large variant for vertical_tabs

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Browser chrome density

**Files:**
- Modify: `crates/sola-browser-core/src/app.rs` (const at line 25; `view_nav_bar` lines 335-353; `view_tab_sidebar` line 330)

**Interfaces:**
- Consumes: `vertical_tabs_sized` + `TabSize` from Task 1.

No unit test — this is pure layout (constants + a call swap); verified by build
+ the user's visual smoke. Values are deliberate starting points, tunable later.

- [ ] **Step 1: Raise `CHROME_HEIGHT`**

In `crates/sola-browser-core/src/app.rs` line 25:

```rust
pub const CHROME_HEIGHT: f32 = 46.0;
```

- [ ] **Step 2: Loosen the nav bar**

In `view_nav_bar` (lines 335-353): the `text_input` gets `.padding(9)` (was 6); the row gets `.spacing(8)` (was 6) and `.padding(10)` (was 6). Resulting tail of the function:

```rust
            text_input("Search or enter URL", &self.url_field)
                .id(crate::integration::url_input_id())
                .on_input(Msg::UrlInput)
                .on_submit(Msg::UrlSubmit)
                .padding(9)
                .width(Length::Fill)
                .style(sola_kit::components::text_input::style),
        ]
        .spacing(8)
        .padding(10)
        .align_y(iced::Alignment::Center)
        .height(Length::Fixed(CHROME_HEIGHT))
        .into()
```

- [ ] **Step 3: Use the Large tab variant**

Change the import at lines 13-15 to pull in `TabSize` and `vertical_tabs_sized`:

```rust
use sola_kit::components::{
    TabDescriptor, TabSize, horizontal_divider, toolbar_button, vertical_divider,
    vertical_tabs_sized,
};
```

Change the final line of `view_tab_sidebar` (line 330) from `vertical_tabs(...)` to:

```rust
        vertical_tabs_sized(tabs, self.hovered_tab, Msg::TabHover, TabSize::Large).into()
```

(Remove `vertical_tabs` from the import since it's no longer used — otherwise an unused-import warning.)

- [ ] **Step 4: Build**

Run: `cargo make build`
Expected: builds clean, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-browser-core/src/app.rs
git commit -m "feat(sola-browser): roomier chrome — taller bar, more padding, Large tabs

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: `EditCmd` plumbed through both engines

Adding `Cmd::Edit` makes both engines' `process_cmd` matches non-exhaustive, so
the enum variant and *both* engine arms must land in one commit to keep the
workspace building. The pure, testable piece is the WebKit command-name map.

**Files:**
- Modify: `crates/sola-browser-core/src/engine.rs` (`Cmd` enum lines 32-42)
- Modify: `crates/sola-browser-core/src/util.rs` (add `editing_command_name`)
- Modify: `crates/sola-browser-wpe/src/engine.rs` (`process_cmd` lines 440-512)
- Modify: `crates/sola-browser-cef/src/engine.rs` (`process_cmd` lines 653-718)

**Interfaces:**
- Produces: `pub enum EditCmd { Copy, Cut, Paste, SelectAll, Undo, Redo }` (derives `Debug, Clone, Copy, PartialEq, Eq`) in `engine.rs`; `Cmd::Edit(EditCmd)` variant; `pub fn editing_command_name(cmd: EditCmd) -> &'static str` in `util.rs`.
- Consumes: WPE `sys::webkit_web_view_execute_editing_command`; CEF `cef::Frame` methods `copy/cut/paste/select_all/undo/redo` (each `fn(&self)`), reached via `tab.browser.main_frame()`.

- [ ] **Step 1: Write the failing test for `editing_command_name`**

Add to the `#[cfg(test)] mod tests` in `crates/sola-browser-core/src/util.rs`:

```rust
#[test]
fn editing_command_names_match_webkit() {
    use crate::engine::EditCmd;
    assert_eq!(editing_command_name(EditCmd::Copy), "Copy");
    assert_eq!(editing_command_name(EditCmd::Cut), "Cut");
    assert_eq!(editing_command_name(EditCmd::Paste), "Paste");
    assert_eq!(editing_command_name(EditCmd::SelectAll), "SelectAll");
    assert_eq!(editing_command_name(EditCmd::Undo), "Undo");
    assert_eq!(editing_command_name(EditCmd::Redo), "Redo");
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p sola-browser-core editing_command_names_match_webkit`
Expected: FAIL — `EditCmd` / `editing_command_name` not found.

- [ ] **Step 3: Add `EditCmd` and `Cmd::Edit`**

In `crates/sola-browser-core/src/engine.rs`, add above the `Cmd` enum (before line 32):

```rust
/// Editing commands routed to the focused web content (or, in the chrome,
/// to the URL bar). Names map to WebKit editing-command strings via
/// [`crate::util::editing_command_name`]; CEF maps them to `cef::Frame`
/// methods directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCmd {
    Copy,
    Cut,
    Paste,
    SelectAll,
    Undo,
    Redo,
}
```

Add the variant to `Cmd` (inside `pub enum Cmd<E: Engine>`), after `SetActiveTab(TabId)`:

```rust
    /// Run an editing command against the active tab's web content.
    Edit(EditCmd),
```

- [ ] **Step 4: Add `editing_command_name`**

In `crates/sola-browser-core/src/util.rs`, add (outside the test module):

```rust
use crate::engine::EditCmd;

/// The WebKit editing-command string for an [`EditCmd`]. WebKit command
/// names are case-sensitive.
pub fn editing_command_name(cmd: EditCmd) -> &'static str {
    match cmd {
        EditCmd::Copy => "Copy",
        EditCmd::Cut => "Cut",
        EditCmd::Paste => "Paste",
        EditCmd::SelectAll => "SelectAll",
        EditCmd::Undo => "Undo",
        EditCmd::Redo => "Redo",
    }
}
```

(If `util.rs` already imports from `crate::engine`, merge the `use` rather than duplicating it.)

- [ ] **Step 5: Run the test to confirm it passes**

Run: `cargo test -p sola-browser-core editing_command_names_match_webkit`
Expected: PASS.

- [ ] **Step 6: Handle `Cmd::Edit` in the WPE worker**

In `crates/sola-browser-wpe/src/engine.rs` `process_cmd` (lines 440-512), add an arm after `Cmd::Nav`:

```rust
        Cmd::Edit(edit) => {
            if let Some(tab) = active_tab(ctx) {
                if !tab.webview.is_null() {
                    let name = sola_browser_core::util::editing_command_name(edit);
                    let name_c = std::ffi::CString::new(name).unwrap();
                    sys::webkit_web_view_execute_editing_command(
                        tab.webview as *mut _,
                        name_c.as_ptr(),
                    );
                }
            }
        }
```

`active_tab(ctx)` and `sys::webkit_web_view_execute_editing_command` already
exist (the latter is generated by the `webkit_.*` bindgen allowlist). Confirm
the imported `EditCmd` path — `Cmd::Edit(EditCmd)` resolves through the existing
`use crate::engine::...` (engine.rs re-exports from `sola-browser-core`); if
`EditCmd` isn't in scope, add it to the WPE `use sola_browser_core::engine::{...}`.

- [ ] **Step 7: Handle `Cmd::Edit` in the CEF worker**

In `crates/sola-browser-cef/src/engine.rs` `process_cmd` (lines 653-718), add an arm after `Cmd::Nav`:

```rust
        Cmd::Edit(edit) => {
            if let Some(tab) = active_tab(state) {
                if let Some(frame) = tab.browser.main_frame() {
                    use sola_browser_core::engine::EditCmd;
                    match edit {
                        EditCmd::Copy => frame.copy(),
                        EditCmd::Cut => frame.cut(),
                        EditCmd::Paste => frame.paste(),
                        EditCmd::SelectAll => frame.select_all(),
                        EditCmd::Undo => frame.undo(),
                        EditCmd::Redo => frame.redo(),
                    }
                }
            }
        }
```

`active_tab(state)` and `tab.browser.main_frame()` follow the existing
`dispatch_nav` pattern (engine.rs lines 889-904). The `cef::Frame` edit methods
are `fn(&self)` with no return.

- [ ] **Step 8: Build everything**

Run: `cargo make build`
Expected: builds clean (core + both engines), no warnings.

- [ ] **Step 9: Commit**

```bash
git add crates/sola-browser-core/src/engine.rs crates/sola-browser-core/src/util.rs crates/sola-browser-wpe/src/engine.rs crates/sola-browser-cef/src/engine.rs
git commit -m "feat(sola-browser): EditCmd + Cmd::Edit dispatched in both engines

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Edit-intent mapping + focus-routing logic (pure)

All pure, fully unit-tested. No App or engine changes here.

**Files:**
- Modify: `crates/sola-browser-core/src/integration.rs`

**Interfaces:**
- Produces:
  - `pub const ACTION_EDIT_UNDO/REDO/CUT/COPY/PASTE/SELECT_ALL: &str` = `"edit-undo"`, `"edit-redo"`, `"edit-cut"`, `"edit-copy"`, `"edit-paste"`, `"edit-select-all"`.
  - `pub const EDIT_MENU_ITEMS: [(&str, &str, KeyChord); 6]`.
  - `BrowserIntent::Edit(EditCmd)` variant.
  - `pub enum EditTarget { Engine, UrlBar }` and `pub fn edit_target(url_bar_focused: bool) -> EditTarget`.
  - `intent_for_menu_action` mapping the six edit ids → `BrowserIntent::Edit(_)`.
- Consumes: `EditCmd` from `crate::engine`; `KeyCode`/`KeyChord` (already imported).

- [ ] **Step 1: Write failing tests**

Add to `mod tests` in `crates/sola-browser-core/src/integration.rs`:

```rust
#[test]
fn edit_actions_map_to_edit_intents() {
    use crate::engine::EditCmd;
    assert_eq!(intent_for_menu_action(ACTION_EDIT_COPY), BrowserIntent::Edit(EditCmd::Copy));
    assert_eq!(intent_for_menu_action(ACTION_EDIT_CUT), BrowserIntent::Edit(EditCmd::Cut));
    assert_eq!(intent_for_menu_action(ACTION_EDIT_PASTE), BrowserIntent::Edit(EditCmd::Paste));
    assert_eq!(intent_for_menu_action(ACTION_EDIT_SELECT_ALL), BrowserIntent::Edit(EditCmd::SelectAll));
    assert_eq!(intent_for_menu_action(ACTION_EDIT_UNDO), BrowserIntent::Edit(EditCmd::Undo));
    assert_eq!(intent_for_menu_action(ACTION_EDIT_REDO), BrowserIntent::Edit(EditCmd::Redo));
}

#[test]
fn edit_target_routes_by_focus() {
    assert_eq!(edit_target(true), EditTarget::UrlBar);
    assert_eq!(edit_target(false), EditTarget::Engine);
}

#[test]
fn edit_menu_items_cover_all_actions() {
    let ids: Vec<&str> = EDIT_MENU_ITEMS.iter().map(|(id, _, _)| *id).collect();
    assert_eq!(
        ids,
        vec![
            ACTION_EDIT_UNDO, ACTION_EDIT_REDO, ACTION_EDIT_CUT,
            ACTION_EDIT_COPY, ACTION_EDIT_PASTE, ACTION_EDIT_SELECT_ALL,
        ]
    );
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p sola-browser-core --lib integration`
Expected: FAIL — unresolved `ACTION_EDIT_*`, `EditTarget`, `edit_target`.

- [ ] **Step 3: Add edit action ids + the Edit menu items**

In `crates/sola-browser-core/src/integration.rs`, after the existing
`ACTION_QUIT` const (line 36) add:

```rust
pub const ACTION_EDIT_UNDO: &str = "edit-undo";
pub const ACTION_EDIT_REDO: &str = "edit-redo";
pub const ACTION_EDIT_CUT: &str = "edit-cut";
pub const ACTION_EDIT_COPY: &str = "edit-copy";
pub const ACTION_EDIT_PASTE: &str = "edit-paste";
pub const ACTION_EDIT_SELECT_ALL: &str = "edit-select-all";
```

After the existing `MENU_ITEMS` const (line 58) add the Edit menu. `KeyCode`
exposes `.meta()` and `.meta_shift()` (verified in `sola-core::keys`):

```rust
/// The "Edit" app-menu published alongside "Browser". Meta-bound so the
/// shell grabs them globally and routes `Topic::MenuAction` back; the
/// browser then routes each to the focused surface (web content or URL bar).
pub const EDIT_MENU_ITEMS: [(&str, &str, KeyChord); 6] = [
    (ACTION_EDIT_UNDO, "Undo", KeyCode::Z.meta()),
    (ACTION_EDIT_REDO, "Redo", KeyCode::Z.meta_shift()),
    (ACTION_EDIT_CUT, "Cut", KeyCode::X.meta()),
    (ACTION_EDIT_COPY, "Copy", KeyCode::C.meta()),
    (ACTION_EDIT_PASTE, "Paste", KeyCode::V.meta()),
    (ACTION_EDIT_SELECT_ALL, "Select All", KeyCode::A.meta()),
];
```

- [ ] **Step 4: Add `BrowserIntent::Edit`, `EditTarget`, `edit_target`**

Add `Edit(EditCmd)` to the `BrowserIntent` enum (after `FocusUrl`, before
`Quit`). Update the `use` at line 25 to bring in `EditCmd`:

```rust
use crate::engine::{Engine, EditCmd};
```

(`Engine` is already imported lower; keep a single `use crate::engine::...`.)

The variant:

```rust
    /// Run an editing command, routed to the focused surface.
    Edit(EditCmd),
```

Add the focus-routing type + helper near the other pure mapping fns:

```rust
/// Which surface an `Edit` intent acts on, chosen by `url_bar_focused`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditTarget {
    /// The web content (full fidelity — honors the page's text selection).
    Engine,
    /// The chrome URL bar (best-effort: whole-field, no partial selection).
    UrlBar,
}

/// Route an `Edit` intent: the URL bar when it holds focus, else the page.
pub fn edit_target(url_bar_focused: bool) -> EditTarget {
    if url_bar_focused {
        EditTarget::UrlBar
    } else {
        EditTarget::Engine
    }
}
```

- [ ] **Step 5: Map the edit ids in `intent_for_menu_action`**

In `intent_for_menu_action` (lines 91-102), add arms before the `_ =>` fallback:

```rust
        ACTION_EDIT_UNDO => BrowserIntent::Edit(EditCmd::Undo),
        ACTION_EDIT_REDO => BrowserIntent::Edit(EditCmd::Redo),
        ACTION_EDIT_CUT => BrowserIntent::Edit(EditCmd::Cut),
        ACTION_EDIT_COPY => BrowserIntent::Edit(EditCmd::Copy),
        ACTION_EDIT_PASTE => BrowserIntent::Edit(EditCmd::Paste),
        ACTION_EDIT_SELECT_ALL => BrowserIntent::Edit(EditCmd::SelectAll),
```

`run_intent` (lines 124-151) now has a non-exhaustive match on `BrowserIntent::Edit`.
To keep the crate building this task, add a temporary arm that compiles but does
nothing yet (Task 5 replaces it):

```rust
        BrowserIntent::Edit(_) => Task::none(),
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p sola-browser-core --lib integration`
Expected: PASS (new + existing). Then `cargo make build` — clean, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-browser-core/src/integration.rs
git commit -m "feat(sola-browser): Edit intents, edit menu items, focus-route helper

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: App focus tracking + run_intent Edit dispatch

Wires the pure logic into `App`. App is not unit-testable (needs a live engine),
so this is build-verified + smoke.

**Files:**
- Modify: `crates/sola-browser-core/src/app.rs` (`Msg` enum lines 32-56; `App` struct lines 62-103; `new` lines 111-139; `update` lines 145-233)
- Modify: `crates/sola-browser-core/src/integration.rs` (`run_intent` Edit arm; `NewBlankTab`/`FocusUrl` set the focus flag)

**Interfaces:**
- Produces: `App.url_bar_focused: bool`; `Msg::WebViewFocused`; `Msg::UrlPasted(Option<String>)`.
- Consumes: `edit_target`/`EditTarget` from Task 4; `editing_command_name` (Task 3); `iced::clipboard::{read, write}`; `iced::advanced::widget::operation::select_all`.

- [ ] **Step 1: Add the two messages**

In `crates/sola-browser-core/src/app.rs`, add to the `Msg` enum (after `TabHover`):

```rust
    /// A left button press landed inside the web view — the page took
    /// keyboard focus, so edit commands route to the engine (not the URL bar).
    WebViewFocused,
    /// Result of an `iced::clipboard::read` kicked off by a URL-bar paste.
    UrlPasted(Option<String>),
```

- [ ] **Step 2: Add the `url_bar_focused` field**

Add to the `App<E>` struct (after `hovered_tab`, before `app_id`):

```rust
    /// True when the chrome URL bar holds keyboard focus, so `Edit`
    /// commands target it instead of the web content. Set by ⌘L / ⌘T /
    /// typing in the bar; cleared when a press lands in the web view.
    /// Best-effort heuristic — see the design spec's documented edge case.
    pub url_bar_focused: bool,
```

Initialize it in `new` (in the `Self { ... }` literal, after `hovered_tab: None,`):

```rust
            url_bar_focused: false,
```

- [ ] **Step 3: Maintain the flag + handle the new messages in `update`**

In `App::update` (lines 145-233):

- In the `Msg::UrlInput(s)` arm (line 157), also set the flag:

```rust
            Msg::UrlInput(s) => {
                self.url_field = s;
                self.url_bar_focused = true;
            }
```

- Add two new arms (e.g. after `Msg::TabHover`):

```rust
            Msg::WebViewFocused => self.url_bar_focused = false,
            Msg::UrlPasted(text) => {
                if let Some(s) = text {
                    // Best-effort: iced exposes no caret/selection, so append
                    // at the end (cursor-at-end assumption).
                    self.url_field.push_str(&s);
                }
            }
```

- [ ] **Step 4: Set the flag on `FocusUrl` and `NewBlankTab`**

In `crates/sola-browser-core/src/integration.rs` `run_intent`:

- In the `BrowserIntent::NewBlankTab` arm (lines 130-139), add `app.url_bar_focused = true;` before `focus_url_bar()`.
- Change the `BrowserIntent::FocusUrl` arm (line 147) so it sets the flag too:

```rust
        BrowserIntent::FocusUrl => {
            app.url_bar_focused = true;
            focus_url_bar()
        }
```

- [ ] **Step 5: Implement the `Edit` dispatch arm**

Replace the temporary `BrowserIntent::Edit(_) => Task::none(),` arm (added in
Task 4) with focus-routed dispatch:

```rust
        BrowserIntent::Edit(cmd) => match edit_target(app.url_bar_focused) {
            EditTarget::Engine => {
                let _ = app.releaser.send(Cmd::Edit(cmd));
                Task::none()
            }
            EditTarget::UrlBar => match cmd {
                EditCmd::Copy => iced::clipboard::write(app.url_field.clone()),
                EditCmd::Cut => {
                    let task = iced::clipboard::write(app.url_field.clone());
                    app.url_field.clear();
                    task
                }
                EditCmd::Paste => iced::clipboard::read().map(Msg::UrlPasted),
                EditCmd::SelectAll => {
                    iced::advanced::widget::operation::select_all(url_input_id())
                }
                // The URL bar has no app-level undo/redo stack.
                EditCmd::Undo | EditCmd::Redo => Task::none(),
            },
        },
```

Update imports at the top of `integration.rs`:
- Add `Cmd` to `use crate::engine::{...}` → `use crate::engine::{Cmd, EditCmd, Engine};`
- The `Msg` import (`use crate::app::{App, BLANK_URL, Msg};`) already covers `Msg::UrlPasted`.

`iced::clipboard::read()` returns `Task<Option<String>>` (mapped via
`.map(Msg::UrlPasted)`); `iced::clipboard::write::<Msg>(String)` returns
`Task<Msg>`; `iced::advanced::widget::operation::select_all::<Msg>(id)` returns
`Task<Msg>` (the `Id` type matches `url_input_id()`'s return).

- [ ] **Step 6: Build**

Run: `cargo make build`
Expected: clean, no warnings. (If `select_all`'s path doesn't resolve, confirm
it against `iced_runtime::widget::operation::select_all` — re-exported under
`iced::advanced::widget::operation`. It is a `Task`-returning free fn, unlike
`focusable::focus` which is wrapped in `operate(..)`.)

Run: `cargo test -p sola-browser-core`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-browser-core/src/app.rs crates/sola-browser-core/src/integration.rs
git commit -m "feat(sola-browser): focus-routed Edit dispatch + url_bar_focused tracking

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Publish the Edit menu (kit multi-menu + browser)

**Files:**
- Modify: `crates/sola-kit/src/app.rs` (`BusSetup` struct lines 56-61; `impl BusSetup` lines 63-142)
- Modify: `crates/sola-browser-core/src/run.rs` (BusSetup call site lines 145-148)

**Interfaces:**
- Produces: `BusSetup::app_menu_more(self, menu_label, items) -> Self` — declares an *additional* menu; the internal store becomes `Vec<MenuDefinition>` and `install` publishes them all in order.
- Consumes: `EDIT_MENU_ITEMS` from Task 4.

The bus payload already supports multiple menus
(`AppMenuPayload { app_id, menus: Vec<MenuDefinition> }`). Today `app_menu`
stores a single `Option<MenuDefinition>`; switch to a `Vec` and append.

- [ ] **Step 1: Change the store to a `Vec`**

In `crates/sola-kit/src/app.rs`, change the `BusSetup` field (line ~59):

```rust
    app_menus: Vec<MenuDefinition>,
```

and in `BusSetup::new` (lines 64-70) initialize `app_menus: Vec::new(),`
(replacing `app_menu: None,`).

- [ ] **Step 2: Append in `app_menu_definition`, add `app_menu_more`**

`app_menu(...)` stays as-is (it calls `app_menu_definition`). Change
`app_menu_definition` to push instead of replace:

```rust
    /// Add a fully-built `MenuDefinition`. Multiple calls publish multiple
    /// top-level menus, in call order.
    pub fn app_menu_definition(mut self, def: MenuDefinition) -> Self {
        self.app_menus.push(def);
        self
    }
```

Add a sibling to `app_menu` for the (id, label, shortcut) shorthand on an
additional menu — it shares `app_menu`'s body, so factor the item-building into
the existing `app_menu` and have both call it. Concretely, rename the current
`app_menu` body's tail so `app_menu` and a new `app_menu_more` both build items
then call `app_menu_definition`:

```rust
    /// Declare an additional top-level menu (same (id, label, shortcut)
    /// shorthand as [`Self::app_menu`]). Call once per extra menu.
    pub fn app_menu_more<I>(self, menu_label: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = (&'static str, &'static str, KeyChord)>,
    {
        self.app_menu(menu_label, items)
    }
```

Since `app_menu` now *appends* (via `app_menu_definition`), `app_menu_more` can
simply delegate to `app_menu` — both append a menu. (Keep `app_menu` as the
"first/primary" name for readability at call sites.)

- [ ] **Step 3: Publish all menus in `install`**

In `install` (lines 121-142), replace the `if let Some(menu) = self.app_menu`
block with:

```rust
        if !self.app_menus.is_empty() {
            if let Err(e) = client.emit(Topic::SetAppMenu(AppMenuPayload {
                app_id: self.app_id.into(),
                menus: self.app_menus,
            })) {
                tracing::warn!(app_id = self.app_id, "publish app menu failed: {e}");
            }
        }
```

- [ ] **Step 4: Publish the Edit menu from the browser**

In `crates/sola-browser-core/src/run.rs` lines 145-148:

```rust
    sola_kit::app::BusSetup::new(app_id)
        .subscribe(crate::integration::SUBSCRIBE)
        .app_menu("Browser", crate::integration::MENU_ITEMS)
        .app_menu_more("Edit", crate::integration::EDIT_MENU_ITEMS)
        .install();
```

- [ ] **Step 5: Build + kit tests**

Run: `cargo make build`
Expected: clean, no warnings (verify no other `BusSetup` consumer relied on the
old `app_menu: Option<..>` field — it's private, so only `app.rs` touches it).
Run: `cargo test -p sola-kit`
Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-kit/src/app.rs crates/sola-browser-core/src/run.rs
git commit -m "feat(sola-kit): BusSetup multi-menu; browser publishes Edit menu

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Publish `Msg::WebViewFocused` on web-view click (both engines)

**Files:**
- Modify: `crates/sola-browser-wpe/src/frame.rs` (`update` mouse `ButtonPressed`, lines ~165-177)
- Modify: `crates/sola-browser-cef/src/frame.rs` (`update` mouse `ButtonPressed`, lines ~115-130)

**Interfaces:**
- Consumes: `Msg::WebViewFocused` (Task 5); `iced::widget::shader::Action::publish(..).and_capture()`.

Both `update` impls forward input over the `self.slot.releaser` side-channel and
return `Some(Action::capture())`. For a **left** button press we additionally
publish `Msg::WebViewFocused` — the input is already sent, so the returned
`Action` just carries the message plus capture: `Action::publish(msg).and_capture()`.

- [ ] **Step 1: WPE — publish on left press**

In `crates/sola-browser-wpe/src/frame.rs`, the mouse branch builds `ev` then does
`if let Some(e) = ev { self.slot.releaser.send(Cmd::Input(e)); return Some(Action::capture()); }`.
Replace that tail so a left press also publishes. The iced left button is
`mouse::Button::Left`; capture whether this event was a left press before `ev` is
moved:

```rust
            let is_left_press = matches!(
                m,
                mouse::Event::ButtonPressed(mouse::Button::Left)
            );
            if let Some(e) = ev {
                let _ = self.slot.releaser.send(Cmd::Input(e));
                if is_left_press {
                    return Some(
                        iced::widget::shader::Action::publish(
                            sola_browser_core::app::Msg::WebViewFocused,
                        )
                        .and_capture(),
                    );
                }
                return Some(iced::widget::shader::Action::capture());
            }
```

- [ ] **Step 2: CEF — publish on left press**

In `crates/sola-browser-cef/src/frame.rs`, apply the identical change to the
mouse branch's `if let Some(e) = ev { ... }` tail. The `mouse` import and
`sola_browser_core::app::Msg` path are already available in this file (it
constructs `Cmd::Input` and references `sola_browser_core::app::Msg` in the
`Program<Msg>` impl).

- [ ] **Step 3: Build**

Run: `cargo make build`
Expected: clean (both engines), no warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-browser-wpe/src/frame.rs crates/sola-browser-cef/src/frame.rs
git commit -m "feat(sola-browser): publish WebViewFocused on web-view left click

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: WPE cmd/middle-click → background tab

**Files:**
- Modify: `crates/sola-browser-core/src/util.rs` (`is_new_tab_click` predicate)
- Modify: `crates/sola-browser-wpe/src/engine.rs` (`WorkerCtx` struct lines 220-236; `worker_main` signature lines 254-264; the spawn call lines 195-202; `open_tab` lines 551-624; add `on_decide_policy` + mask consts)

**Interfaces:**
- Produces: `pub fn is_new_tab_click(mouse_button: u32, ctrl: bool, super_key: bool) -> bool` in `util.rs` (`true` for middle button, or left + ctrl/super).
- Consumes: the shared `next_id: Arc<AtomicU64>` (threaded into the worker); WebKit policy bindings (`webkit_navigation_policy_decision_get_navigation_action`, `webkit_navigation_action_get_mouse_button`/`_modifiers`/`_request`, `webkit_uri_request_get_uri`, `webkit_policy_decision_ignore`); WPE modifier consts `sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_CONTROL`/`_META` (used by `input.rs`).

- [ ] **Step 1: Write the failing predicate test**

Add to `mod tests` in `crates/sola-browser-core/src/util.rs`:

```rust
#[test]
fn new_tab_click_rules() {
    // Middle-click always opens a background tab, regardless of modifiers.
    assert!(is_new_tab_click(2, false, false));
    // Left-click with ctrl or super opens a background tab.
    assert!(is_new_tab_click(1, true, false));
    assert!(is_new_tab_click(1, false, true));
    // Plain left-click navigates in place.
    assert!(!is_new_tab_click(1, false, false));
    // Right-click is the context menu, not a new tab.
    assert!(!is_new_tab_click(3, true, true));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p sola-browser-core new_tab_click_rules`
Expected: FAIL — `is_new_tab_click` not found.

- [ ] **Step 3: Implement the predicate**

In `crates/sola-browser-core/src/util.rs`:

```rust
/// Whether a link click should open a background tab instead of navigating
/// in place: middle button (2), or left button (1) with Ctrl or Super held.
pub fn is_new_tab_click(mouse_button: u32, ctrl: bool, super_key: bool) -> bool {
    mouse_button == 2 || (mouse_button == 1 && (ctrl || super_key))
}
```

Run: `cargo test -p sola-browser-core new_tab_click_rules` → PASS.

- [ ] **Step 4: Thread `next_id` into the WPE worker**

In `crates/sola-browser-wpe/src/engine.rs`:

- Add to `WorkerCtx` (lines 220-236):

```rust
    /// Shared monotonic tab-id counter (also held chrome-side). The
    /// decide-policy callback mints a background-tab id from this without
    /// involving the chrome.
    next_id: Arc<std::sync::atomic::AtomicU64>,
```

- Add a `next_id` param to `worker_main` (lines 254-264) and pass it through to
  the `WorkerCtx { .. }` construction (find where `WorkerCtx` is built inside
  `worker_main` and add `next_id,`).
- At the spawn site (lines 192-202), clone before the move and pass it:

```rust
        let next_id_w = next_id.clone();
        let worker = thread::Builder::new()
            .name("wpe-engine".into())
            .spawn(move || unsafe {
                worker_main(
                    width, height, frame_tx, cmd_rx, ready_tx, cursor_w, snapshot_w,
                    active_w, next_id_w,
                )
            })
            .expect("spawn wpe-engine thread");
```

(`next_id` is still moved into `Self { .. next_id }` afterward — clone for the
worker as shown.)

- [ ] **Step 5: Connect `decide-policy` in `open_tab`**

In `open_tab` (lines 551-624), after the `notify::title` signal connect and
before `webkit_web_view_load_uri`, connect `decide-policy` with the worker ctx
pointer as user-data (mirrors the global buffer callback's `ctx as *mut c_void`
— the same ctx serves every tab on this single-threaded worker):

```rust
    let policy_signal = CString::new("decide-policy").unwrap();
    sys::g_signal_connect_data(
        webview as *mut c_void,
        policy_signal.as_ptr(),
        Some(std::mem::transmute::<
            unsafe extern "C" fn(
                *mut sys::WebKitWebView,
                *mut sys::WebKitPolicyDecision,
                sys::WebKitPolicyDecisionType,
                *mut c_void,
            ) -> sys::gboolean,
            unsafe extern "C" fn(),
        >(on_decide_policy)),
        ctx as *mut WorkerCtx as *mut c_void,
        None,
        0,
    );
```

- [ ] **Step 6: Add the `on_decide_policy` callback + modifier consts**

Add near the other worker FFI callbacks in `crates/sola-browser-wpe/src/engine.rs`:

```rust
unsafe extern "C" fn on_decide_policy(
    _web_view: *mut sys::WebKitWebView,
    decision: *mut sys::WebKitPolicyDecision,
    decision_type: sys::WebKitPolicyDecisionType,
    user_data: *mut c_void,
) -> sys::gboolean {
    // Only ordinary navigations (link clicks) are interesting; let WebKit
    // apply default policy to everything else (new-window, response, …).
    if decision_type
        != sys::WebKitPolicyDecisionType_WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION
    {
        return 0; // FALSE
    }
    let nav = decision as *mut sys::WebKitNavigationPolicyDecision;
    let action = sys::webkit_navigation_policy_decision_get_navigation_action(nav);
    if action.is_null() {
        return 0;
    }
    let button = sys::webkit_navigation_action_get_mouse_button(action);
    let mods = sys::webkit_navigation_action_get_modifiers(action);
    let ctrl = (mods & sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_CONTROL) != 0;
    let super_key = (mods & sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_META) != 0;
    if !sola_browser_core::util::is_new_tab_click(button, ctrl, super_key) {
        return 0; // ordinary click — navigate in place.
    }
    let request = sys::webkit_navigation_action_get_request(action);
    if request.is_null() {
        return 0;
    }
    let uri_ptr = sys::webkit_uri_request_get_uri(request);
    if uri_ptr.is_null() {
        return 0;
    }
    let uri = std::ffi::CStr::from_ptr(uri_ptr).to_string_lossy().into_owned();
    // Suppress the in-place navigation; open a background tab instead.
    sys::webkit_policy_decision_ignore(decision);
    let ctx = &mut *(user_data as *mut WorkerCtx);
    let id = TabId(ctx.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    open_tab(ctx, id, uri); // no SetActiveTab → background tab
    1 // TRUE — handled.
}
```

**Verify against generated bindings before relying on names:** confirm the
exact bindgen identifiers in the generated `wpe_bindings.rs` (the `sys` module):
`WebKitPolicyDecisionType_WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION`,
`WebKitNavigationPolicyDecision`, and the four `webkit_navigation_*` /
`webkit_uri_request_get_uri` / `webkit_policy_decision_ignore` functions (all
generated by the `webkit_.*` allowlist). The two modifier consts are reused from
the same `WPEModifiers_*` set `input.rs` uses.

- [ ] **Step 7: Build**

Run: `cargo make build`
Expected: clean, no warnings. Fix any binding-name mismatches surfaced here.

- [ ] **Step 8: Manual smoke (USER)**

> **For the user to run** — do not install. With a locally-built `sola-browser`,
> open a page with links; middle-click and ⌘-click (and Ctrl-click) a link.
> Each should open a new tab in the background (current tab stays focused). If
> ⌘/Ctrl don't register, temporarily log `button`/`mods` in `on_decide_policy`
> to confirm the actual modifier mask values for this WPE build, then adjust the
> two mask consts. Middle-click is the modifier-independent fallback and should
> always work.

- [ ] **Step 9: Commit**

```bash
git add crates/sola-browser-core/src/util.rs crates/sola-browser-wpe/src/engine.rs
git commit -m "feat(sola-browser-wpe): cmd/middle-click opens link in background tab

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 9: CEF cmd/middle-click → background tab

**Files:**
- Modify: `crates/sola-browser-cef/src/engine.rs` (`CefThreadState` lines 218-241; state construction in spawn; `BrowserLifeSpanHandler` lines 476-507; reuse `open_tab` lines 744-790)

**Interfaces:**
- Consumes: `is_new_tab_click` is **not** needed here — CEF already encodes
  "this click wants a new window/tab" by invoking `on_before_popup`; we cancel
  the native popup and open the target URL as a background tab. The shared
  `next_id` must be reachable from `CefThreadState`.

- [ ] **Step 1: Add `next_id` to `CefThreadState`**

In `crates/sola-browser-cef/src/engine.rs`, add to `CefThreadState` (lines 218-241):

```rust
    /// Shared monotonic tab-id counter (also held chrome-side). `on_before_popup`
    /// mints a background-tab id from this on the CEF UI thread.
    next_id: Arc<std::sync::atomic::AtomicU64>,
```

Populate it where `CefThreadState` is constructed in `spawn` — clone the engine's
`next_id` into it, mirroring how `active_atomic` is threaded in. (Find the
`CefThreadState { .. }` literal in the spawn path and add `next_id: next_id.clone(),`;
ensure the engine's `next_id` is in scope there, as it is for the chrome-side
`Self { .. next_id }`.)

- [ ] **Step 2: Add `on_before_popup` to the life-span handler**

In the `cef::wrap_life_span_handler! { ... impl LifeSpanHandler { ... } }` block
(lines 476-507), add a second method next to `on_before_close`. Return `1` to
cancel the native popup, and open the target URL as a background tab:

```rust
        #[allow(clippy::too_many_arguments)]
        fn on_before_popup(
            &self,
            _browser: Option<&mut cef::Browser>,
            _frame: Option<&mut cef::Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&cef::CefString>,
            _target_frame_name: Option<&cef::CefString>,
            _target_disposition: cef::WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&cef::PopupFeatures>,
            _window_info: Option<&mut cef::WindowInfo>,
            _client: Option<&mut Option<cef::Client>>,
            _settings: Option<&mut cef::BrowserSettings>,
            _extra_info: Option<&mut Option<cef::DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let url = target_url.map(|u| u.to_string()).unwrap_or_default();
            if url.is_empty() {
                return 1; // cancel the popup; nothing to open
            }
            let state = cef_state();
            let id = TabId(state.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            open_tab(&state, id, url); // no SetActiveTab → background tab
            1 // cancel the native popup — we've handled it as a tab
        }
```

Confirm the exact CEF type spellings against the trait declaration in the `cef`
crate's generated bindings (the signature mirrors `ImplLifeSpanHandler::on_before_popup`:
`target_url: Option<&CefString>`, `target_disposition: WindowOpenDisposition`,
`client: Option<&mut Option<Client>>`, …). `cef_state()`, `TabId`, and `open_tab`
are already in scope in this module.

- [ ] **Step 3: Build**

Run: `cargo make build`
Expected: clean, no warnings. Resolve any `cef` type-path mismatches surfaced
here against the crate's bindings.

- [ ] **Step 4: Manual smoke (USER)**

> **For the user to run** — do not install. With a locally-built CEF browser
> (`sola-browser --engine cef`), middle-click / ⌘-click / Ctrl-click a link and
> confirm it opens a background tab (current tab stays focused). CEF maps all of
> these — plus `target=_blank` — through `on_before_popup`, so no per-modifier
> tuning is needed.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-browser-cef/src/engine.rs
git commit -m "feat(sola-browser-cef): cmd/middle-click opens link in background tab

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Feature 1 (chrome density): kit `TabSize`/`vertical_tabs_sized` + storybook
  (Task 1); browser `CHROME_HEIGHT`/nav padding/`TabSize::Large` (Task 2). ✓
- Feature 2 (cmd-click → background tab): predicate + WPE decide-policy (Task 8);
  CEF `on_before_popup` (Task 9). Both open background tabs via engine self-open
  with shared `next_id`, no `SetActiveTab`. ✓
- Feature 3 (Edit menu + clipboard): `EditCmd`/`Cmd::Edit` + engine arms (Task 3);
  intents/menu items/focus-route helper (Task 4); App focus tracking + run_intent
  dispatch (Task 5); multi-menu publish (Task 6); `WebViewFocused` publish (Task 7).
  Redo uses `⌘⇧Z` (`KeyCode::Z.meta_shift()` — confirmed to exist, no `⌘Y`
  fallback needed). URL-bar copy is whole-field; SelectAll uses the real iced
  operation; Undo/Redo are URL-bar no-ops — all per spec. ✓
- Deferred (DevTools, Bitwarden): correctly absent. ✓

**Type consistency:** `EditCmd` (engine.rs) used uniformly across `Cmd::Edit`,
`BrowserIntent::Edit`, `editing_command_name`, `intent_for_menu_action`, both
engine arms, and the run_intent arm. `Msg::WebViewFocused`/`UrlPasted(Option<String>)`
defined in Task 5, consumed in Task 7 (publish) and Task 5 (handle). `EditTarget`/
`edit_target(bool)` defined in Task 4, consumed in Task 5. `is_new_tab_click(u32,
bool, bool)` defined + tested in Task 8, consumed by the WPE callback only.
`vertical_tabs_sized(.., TabSize)` defined in Task 1, consumed in Task 2.
`app_menu_more` defined in Task 6, consumed in Task 6's run.rs edit.

**Build-greenness:** Each task ends building clean. The risky non-exhaustive-match
moments are contained: `Cmd::Edit` lands with both engine arms in one commit
(Task 3); `BrowserIntent::Edit` gets a temporary `=> Task::none()` arm in Task 4,
replaced with real dispatch in Task 5. Pub enum variants added before their
constructors exist (`Msg::WebViewFocused`, `Cmd::Edit`) don't trip dead-code in a
lib crate.

**Placeholder scan:** No TBD/TODO. Engine FFI tasks carry exact code plus an
explicit "verify the generated binding name" step (a real verification action,
not a placeholder) because bindgen/CEF identifier spellings can only be confirmed
against generated output — the structure, calls, and integration points are all
concrete.
