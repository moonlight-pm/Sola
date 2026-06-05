# sola-shell kit-ification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate `sola-shell`'s four UI surfaces (menubar, menu dropdown, launcher, switcher) off hand-rolled `container(...).style()` + `mouse_area` styling and onto shared `sola-kit` components, so shell chrome is consistent with settings/terminal/monitor and the styling lives in one place.

**Architecture:** `sola-shell` is already an `iced::daemon` on `sola-kit` (lifecycle, bus, theme protocol, fonts, icons, text-input). This plan adds a small set of opt-in kit features (chrome theme variants, a selectable-list-item button style, a menubar button style, a horizontal divider, an overlay/backplate card variant), then rewrites each surface's `view()` to use them. The pivotal change is moving overlay/menubar theming into the kit and fixing the overlay palette so kit components that derive colors from the ambient theme render correctly inside the transparent overlay windows — letting us delete the "close over `shell.theme`" closures rather than port them.

**Tech Stack:** Rust, iced 0.14 (`wgpu`/`wayland`), `sola-kit`, `sola-bus`.

**Status:** complete (2026-06-04)

---

## Background: the overlay-theming crux (verified facts)

The shell hands each window a per-window theme via `Shell::theme(window)` (`crates/sola-shell/src/app.rs:418`):

- **Menubar window:** base palette `background = BLACK`, then `Extended::generate`. The bar is permanently black; only fg/icons follow the theme.
- **Overlay windows (menu/launcher/switcher):** base palette `background = TRANSPARENT`, then `Extended::generate`. This makes the OS-transparent window see-through — but because `Extended::generate` derives every background tier from the transparent base, **all** `background.*` tiers come out transparent. That is why each surface paints its card chrome with `container(...).style(|_| …)` closures that capture `shell.theme.extended_palette()` (the real, opaque palette) instead of reading the ambient theme.

Verified against the iced 0.14 source (do not re-verify; these are established):

- The window fill is painted from **`extended_palette().background.base.color`** — `iced_core-0.14.0/src/theme.rs:333` (`default(theme)` returns `Style { background_color: palette.background.base.color, .. }`). It is NOT the base `Palette.background`.
- `iced::Theme::custom_with_fn(name, palette, generate)` takes `generate: impl FnOnce(Palette) -> Extended` — `iced_core-0.14.0/src/theme.rs:100`. A capturing closure is allowed.
- `iced::theme::palette::Extended`, `Background`, and `Pair` all have **public, mutable** fields (`Background { base, weakest, weaker, weak, strong, stronger, strongest }`, `Pair { color, text }`) — `iced_core-0.14.0/src/theme/palette.rs:288,426,450`.

**Consequence:** We can build an overlay theme whose Extended palette is generated from the *real* (opaque) palette, then force only `background.base.color = TRANSPARENT`. Result:

- Window fill = `background.base.color` = transparent → window still see-through.
- `background.weak/weaker/strong/stronger` = real opaque colors → kit `card()`, `popover::style`, `button::*` render opaque and correct inside overlays.
- `primary.*` is derived from the accent (independent of background) and is already correct today — selected-item fills already work in overlays.
- The only other consumer of `background.base.color` is `text_input::style` (field background). A transparent field background sitting inside the opaque card is the **current** behavior and is acceptable (the field blends into the card). No regression.

This is a strict improvement over today (today *every* background tier is transparent; after, only `base.color` is).

---

## File Structure

**New / modified kit files:**

- `crates/sola-kit/src/theme.rs` — add `overlay(&Theme) -> Theme` and `menubar(&Theme) -> Theme` chrome theme builders.
- `crates/sola-kit/src/components/button.rs` — add `list_item(selected: bool)` style-fn factory and `menubar(active: bool)` style-fn factory.
- `crates/sola-kit/src/components/divider.rs` — add `horizontal_divider(...)` + `horizontal_style`.
- `crates/sola-kit/src/components/card.rs` — add `modal(content)` / `modal_style` (deep shadow) and `accent_backplate(content)` / `accent_backplate_style` (primary-tinted translucent).
- `crates/sola-kit/src/components/mod.rs` and `crates/sola-kit/src/lib.rs` — export the new symbols as needed.
- `crates/sola-kit/src/storybook/pages/{button,divider,card}.rs` — extend showcases; add a `theme`/chrome demo if practical.

**Modified shell files:**

- `crates/sola-shell/src/app.rs` — `theme(window)` delegates to `sola_kit::theme::{menubar,overlay}`.
- `crates/sola-shell/src/menu/view.rs` — card → `popover::style`; items → kit button styles; divider → `horizontal_divider`.
- `crates/sola-shell/src/launcher/view.rs` — card → `card::modal`; rows → `button::list_item`; divider → `horizontal_divider`.
- `crates/sola-shell/src/switcher/view.rs` — backplate → `card::accent_backplate`; cards → `button::list_item` (or selectable container helper).
- `crates/sola-shell/src/menubar/view.rs` — labels → `button::menubar`; drop `highlight_container`.

**Conventions to follow:**
- Building is verification (`cargo make build`); never `cargo make install` (user-only, per-call).
- Work on `master` directly (single-session; per user preference). Commit per task with the kit/shell prefix style already in the log; do not merge or push without explicit instruction.
- Use Serena symbol tools for code reads/edits where they fit.
- No time estimates.

---

## Phase 0 — Kit foundation

These land first because every surface depends on them. Each kit addition must keep existing APIs untouched (backward-compatible) and update the matching storybook page.

### Task 0.1: Chrome theme builders in the kit

**Files:**
- Modify: `crates/sola-kit/src/theme.rs`
- Test: `crates/sola-kit/src/theme.rs` (`#[cfg(test)]`)

Add two functions that produce per-window chrome themes from a base `iced::Theme`:

```rust
/// Theme for an OS-transparent overlay window (menu / launcher / switcher).
///
/// The window fill is painted from `extended_palette().background.base.color`
/// (iced 0.14), so we generate the Extended palette from the REAL palette —
/// keeping every tier opaque so kit chrome (`card`, `popover`, `button`) reads
/// correct colors — then force only `background.base.color` transparent so the
/// area around the card stays see-through.
pub fn overlay(base: &Theme) -> Theme {
    let palette = base.palette();
    Theme::custom_with_fn(
        "sola-overlay".to_string(),
        palette,
        |p| {
            let mut ext = palette::Extended::generate(p);
            ext.background.base.color = Color::TRANSPARENT;
            ext
        },
    )
}

/// Theme for the permanently-black menubar. Background tiers are generated
/// from a black base so any hover/active fills derive from black; foreground
/// text/icons still follow the real palette via `base.text`.
pub fn menubar(base: &Theme) -> Theme {
    let mut palette = base.palette();
    palette.background = Color::BLACK;
    Theme::custom_with_fn(
        "sola-menubar".to_string(),
        palette,
        palette::Extended::generate,
    )
}
```

- [ ] **Step 1:** Add the two functions (with the imports they need: `iced::theme::palette`, `iced::Color`, `iced::Theme`).
- [ ] **Step 2:** Add a unit test `overlay_keeps_tiers_opaque_but_base_transparent`: build `overlay(&default_theme())`, assert `ext.background.base.color.a == 0.0` and `ext.background.weak.color.a == 1.0` (or `> 0.0`) and `ext.background.strong.color.a > 0.0`. Assert `menubar(&default_theme())` has `ext.background.base.color` equal to a black-derived (opaque) color and `primary.base` unchanged from the input accent.
- [ ] **Step 3:** `cargo make build sola-kit` and `cargo test -p sola-kit` — both pass.
- [ ] **Step 4:** Commit `feat(sola-kit): overlay + menubar chrome theme builders`.

**Notes for implementer:** `default_theme()` / `Atoms` are available in this module. Do not change `default_theme`, `from_bus_theme`, etc. Keep the functions `pub`.

### Task 0.2: `button::list_item(selected)` style factory

**Files:**
- Modify: `crates/sola-kit/src/components/button.rs`
- Modify (showcase): `crates/sola-kit/src/storybook/pages/button.rs`
- Test: `crates/sola-kit/src/components/button.rs` (`#[cfg(test)]` if feasible; otherwise rely on storybook + build)

A selectable list row: when `selected`, it shows a filled accent pill regardless of hover; when not selected, it is transparent at rest and lifts on hover. This matches launcher rows, switcher cards, and menu items.

```rust
/// Style for a selectable list-row button.
///
/// `selected` is owned by the app (keyboard/MRU selection), independent of the
/// pointer `Status`. Selected → filled `primary` pill. Unselected → transparent,
/// lifting to `background.strong` on hover/press. Radius is `RADIUS_MD`.
pub fn list_item(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        if selected {
            return button::Style {
                background: Some(Background::Color(p.primary.base.color)),
                text_color: p.primary.base.text,
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_MD.into() },
                shadow: Default::default(),
                snap: false,
            };
        }
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => p.background.strong.color,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.background.base.text,
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_MD.into() },
            shadow: Default::default(),
            snap: false,
        }
    }
}
```

- [ ] **Step 1:** Add `list_item`. Reuse existing imports (`Background`, `Border`, `Color`, `RADIUS_MD`, `button`, `Theme`). Confirm the radius const name actually exported in this crate (`RADIUS_MD`); if the file uses a different name, match it.
- [ ] **Step 2:** Call site shape is `button(content).style(kit_btn::list_item(is_selected))`. Confirm iced accepts an `impl Fn(&Theme, Status) -> Style` as a `.style(...)` argument (it does — `button::StyleFn`). If the borrow checker complains about the closure capturing `selected`, the `move` + `Copy` bool handles it.
- [ ] **Step 3:** Storybook: add a row to `button.rs` page showing `list_item(true)` and `list_item(false)` side by side (selected vs unselected).
- [ ] **Step 4:** `cargo make build sola-kit` passes; `cargo make build sola-kit` storybook binary compiles.
- [ ] **Step 5:** Commit `feat(sola-kit): list_item selectable button style`.

### Task 0.3: `button::menubar(active)` style factory

**Files:**
- Modify: `crates/sola-kit/src/components/button.rs`
- Modify (showcase): `crates/sola-kit/src/storybook/pages/button.rs`

Menubar labels: transparent at rest; a translucent foreground-tinted highlight on hover and when the menu is open (`active`). Derives from `background.base.text` so it reads as a light highlight on the black bar (and adapts if the bar color ever changes).

```rust
/// Style for a menubar label button. `active` = its menu is open.
/// Transparent at rest; a translucent fg-tinted highlight on hover or when
/// active. `RADIUS_SM`.
pub fn menubar(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        let fg = p.background.base.text;
        let highlight = |a: f32| Color { a, ..fg };
        let bg = if active {
            highlight(0.18)
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => highlight(0.12),
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_SM.into() },
            shadow: Default::default(),
            snap: false,
        }
    }
}
```

- [ ] **Step 1:** Add `menubar`. Confirm `RADIUS_SM` const name in-crate; match the actual name.
- [ ] **Step 2:** Storybook: add a small demo (on a dark container) showing rest / hover / active. A static `active` example is fine.
- [ ] **Step 3:** `cargo make build sola-kit` passes.
- [ ] **Step 4:** Commit `feat(sola-kit): menubar label button style`.

### Task 0.4: `horizontal_divider` in the kit

**Files:**
- Modify: `crates/sola-kit/src/components/divider.rs`
- Modify (export): `crates/sola-kit/src/components/mod.rs` / `lib.rs` if `vertical_divider` is re-exported there (mirror it).
- Modify (showcase): `crates/sola-kit/src/storybook/pages/divider.rs`

The menu and launcher both use `rule::horizontal(1)`. Give the kit a 1px horizontal divider that reads like the hairline borders (`background.stronger`), matching the existing `vertical_divider` color.

```rust
/// A 1px horizontal divider line, same color as the hairline borders.
pub fn horizontal_divider<'a, Message: 'a>() -> Element<'a, Message, Theme> {
    container(Space::new(Length::Fill, Length::Fixed(1.0)))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(horizontal_style)
        .into()
}

pub fn horizontal_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(p.background.stronger.color.into()),
        ..container::Style::default()
    }
}
```

- [ ] **Step 1:** Add `horizontal_divider` + `horizontal_style`. Match the file's existing imports/patterns (the vertical version is the reference). It takes no `on_press` (non-interactive).
- [ ] **Step 2:** If `vertical_divider` is re-exported from `components/mod.rs` or `lib.rs`, add `horizontal_divider` alongside.
- [ ] **Step 3:** Storybook: add a horizontal divider between two stacked rows on the divider page.
- [ ] **Step 4:** `cargo make build sola-kit` passes.
- [ ] **Step 5:** Commit `feat(sola-kit): horizontal_divider`.

### Task 0.5: Overlay/backplate card variants

**Files:**
- Modify: `crates/sola-kit/src/components/card.rs`
- Modify (showcase): `crates/sola-kit/src/storybook/pages/card.rs`

Two new chrome variants. The menu reuses the existing `popover::style` (card + shadow) — no new code needed there. The launcher needs a deeper modal shadow; the switcher needs a primary-tinted translucent backplate.

```rust
/// Deep-shadow modal card (launcher panel). Larger radius, heavier shadow than
/// the default card.
pub fn modal_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        // `weaker`, not `base`: overlay windows zero base's alpha for the
        // see-through window fill; the modal card must stay opaque.
        background: Some(Background::Color(p.background.weaker.color)),
        border: Border { color: p.background.strong.color, width: 1.0, radius: RADIUS_XL.into() },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
            offset: Vector::new(0.0, 16.0),
            blur_radius: 48.0,
        },
        ..container::Style::default()
    }
}

pub fn modal<'a, Message: 'a>(content: impl Into<Element<'a, Message, Theme>>)
    -> Container<'a, Message, Theme>
{ container(content).style(modal_style) }

/// Primary-tinted translucent backplate (switcher). Background and border are
/// the accent at low alpha; deep shadow.
pub fn accent_backplate_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let a = p.primary.base.color;
    container::Style {
        background: Some(Background::Color(Color { a: 0.18, ..a })),
        border: Border { color: Color { a: 0.35, ..a }, width: 1.0, radius: RADIUS_XL.into() },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..container::Style::default()
    }
}

pub fn accent_backplate<'a, Message: 'a>(content: impl Into<Element<'a, Message, Theme>>)
    -> Container<'a, Message, Theme>
{ container(content).style(accent_backplate_style) }
```

- [ ] **Step 1:** Add the four functions. Confirm a `RADIUS_XL` (or similar large radius) const exists; if not, add one to the kit's radius constants module (e.g. `pub const RADIUS_XL: f32 = 14.0;`) or use a literal `14.0`/`16.0` consistent with the originals (launcher 14px, switcher 16px — if one const can't serve both, use literals and document them). Bring in `Shadow`, `Vector` imports.
- [ ] **Step 2:** Storybook: add `modal` and `accent_backplate` demos to the card page.
- [ ] **Step 3:** `cargo make build sola-kit` passes.
- [ ] **Step 4:** Commit `feat(sola-kit): modal + accent_backplate card variants`.

### Task 0.6: Shell delegates per-window theming to the kit

**Files:**
- Modify: `crates/sola-shell/src/app.rs` (`Shell::theme`)

Replace the inline menubar/overlay theme construction with calls to the kit builders. This is the switch that makes the surface migrations possible.

- [ ] **Step 1:** Rewrite `theme(window)`:
  ```rust
  pub fn theme(&self, window: iced::window::Id) -> iced::Theme {
      if Some(window) == self.menubar_window_id {
          return sola_kit::theme::menubar(&self.theme);
      }
      sola_kit::theme::overlay(&self.theme)
  }
  ```
- [ ] **Step 2:** `cargo make build sola-shell` passes.
- [ ] **Step 3:** Commit `refactor(sola-shell): per-window theming via kit theme::{menubar,overlay}`.

**Important:** After this task the overlay windows' ambient theme has opaque background tiers. The surfaces still use their own closures-over-`shell.theme` at this point, so they keep working unchanged. Phases 1–4 then swap those closures for ambient-theme kit components one surface at a time. Do NOT migrate surfaces before 0.6 lands.

---

## Phase 1 — Menu dropdown → kit

**Files:** `crates/sola-shell/src/menu/view.rs`

Target mapping:
- Dropdown card container (currently inline `container(...).style(|_| { bg/border/shadow over shell.theme })`) → `sola_kit::components::popover(items).width(Fixed(220.0)).padding(4.0)` (popover::style provides bg + hairline + shadow). Keep the 220px width and the anchor-x positioning padding exactly as today.
- Section divider (`rule::horizontal(1)`) → `sola_kit::components::horizontal_divider()`.
- Enabled action item (inline `button(...).style(|theme,status| hover=primary)`) → `iced::widget::button(item_row).style(kit_btn::list_item(false)).on_press(...)`. (Menu items have no external selection; `list_item(false)` gives transparent→hover-fill, matching today's hover=primary intent. If you want the hover fill to be the accent specifically, that is `list_item`'s unselected-hover = `background.strong`; if the team prefers accent-on-hover for menus, add a `menu_item` variant instead — DECISION POINT, default to `list_item(false)` for consistency unless it looks wrong in the storybook/build.)
- Disabled item (`container(item_row)` gray text) → keep a `container` but color the text via `kit_text::muted` rather than a hardcoded gray.
- Label / shortcut text → `kit_text::body`/`caption` sizes; shortcut via `kit_text::muted`.

- [ ] **Step 1:** Read `menu/view.rs` fully (Serena overview + the `view` symbol). Note the anchor-x padding logic and the `MenuItem::{Divider,Action}` match.
- [ ] **Step 2:** Replace the card container with `popover(...)`, preserving width/padding/anchor. Remove the inline style closure and any now-unused `card_bg`/`card_border` locals.
- [ ] **Step 3:** Replace the divider with `horizontal_divider()`.
- [ ] **Step 4:** Replace the enabled-item button style with `kit_btn::list_item(false)`; keep `.on_press(Msg::MenuAction { .. })`, `.width(Fill)`, padding, and the `[label | spacer | shortcut]` row. Replace disabled-item gray with `kit_text::muted`.
- [ ] **Step 5:** `cargo make build sola-shell` passes. Verify no leftover `shell.theme.extended_palette()` closures remain in this file.
- [ ] **Step 6:** Commit `refactor(sola-shell): menu dropdown on kit popover/list_item/divider`.

**Acceptance:** Menu renders an opaque rounded card with shadow, items highlight on hover, shortcuts are muted, disabled items are muted, dividers are hairlines — all driven by the ambient (now-opaque) overlay theme, no captured `shell.theme`.

---

## Phase 2 — Launcher → kit

**Files:** `crates/sola-shell/src/launcher/view.rs`

Target mapping:
- Outer card (inline 640px, 14px radius, deep shadow over shell.theme) → `sola_kit::components::card::modal(card_body).width(Fixed(640.0))`.
- Query input → already `text_input(...).style(input_style)`; leave as-is (kit input). Confirm it still reads correctly now that `background.base.color` is transparent inside the card (expected: field blends into card, same as today).
- Separator (`rule::horizontal(1)`) → `horizontal_divider()`.
- App row button (inline selected=primary / hover=bg.weak / default=transparent) → `iced::widget::button(row_content).style(kit_btn::list_item(is_selected)).on_press(Msg::Launch)`, preserving padding `[12,16]`, `width(Fill)`, the `[icon(24) | label(16)]` row, and 8px radius (note: `list_item` uses `RADIUS_MD`; if 8px must be exact, confirm `RADIUS_MD == 8.0` in the kit — it should; otherwise accept the kit radius for consistency).
- Backdrop (black @ 40%) → keep as a local `mouse_area`+`container` (it's window-orchestration, not a reusable widget); optionally factor a tiny local helper. Not a kit component.

- [ ] **Step 1:** Read `launcher/view.rs` fully. Identify the row-builder, the card, the backdrop, the scrollable.
- [ ] **Step 2:** Replace the outer card with `card::modal(...)`. Drop `card_bg`/`card_border` locals and the inline style.
- [ ] **Step 3:** Replace each app row's inline style with `kit_btn::list_item(is_selected)`. Keep `.on_press(Msg::Launch)`, padding, width, and the icon+label row. Remove `primary_base`/`bg_weak`/`bg_text` locals if now unused.
- [ ] **Step 4:** Replace the separator with `horizontal_divider()`.
- [ ] **Step 5:** `cargo make build sola-shell` passes; no captured-`shell.theme` closures remain except the backdrop (which uses a literal alpha, fine).
- [ ] **Step 6:** Commit `refactor(sola-shell): launcher on kit card::modal/list_item/divider`.

**Acceptance:** Launcher shows the deep-shadow modal card; selected row is the accent pill, hover lifts non-selected rows, keyboard nav still drives `is_selected`; query input unchanged behaviorally.

---

## Phase 3 — Switcher → kit

**Files:** `crates/sola-shell/src/switcher/view.rs`

Target mapping:
- Backplate (inline primary-tint translucent, 16px radius, deep shadow, derived from `shell.theme` `real` palette) → `sola_kit::components::card::accent_backplate(row_of_cards).padding(36.0)`. Because the ambient overlay theme now has opaque tiers and a correct `primary`, `accent_backplate_style` reading the ambient theme produces the same tint — so the `let real = shell.theme.extended_palette()` capture (line ~100) can be deleted.
- App card (inline selected=primary fill / default transparent, 8px radius) → wrap the `[icon(52) | label(13)]` column in a pressable/selectable element. Two options:
  - (a) `iced::widget::button(card_content).style(kit_btn::list_item(is_selected))` wrapped so `on_enter` hover still emits `SwitcherHover`. But `button` doesn't expose `on_enter`. The current code uses `mouse_area(container).on_enter(...)`. To preserve hover-select, keep the `mouse_area(...).on_enter(Msg::SwitcherHover { index })` and put a **styled container** inside whose style is a thin wrapper that mirrors `list_item`. Since `list_item` is a button style fn, add a sibling `card::selectable_style(selected)` (container style) OR reuse `accent`/primary logic inline-but-from-kit. DECISION POINT: add `card::list_tile_style(selected: bool) -> container::Style` to the kit (selected→primary fill, else transparent, `RADIUS_MD`) in this task if needed, with a storybook note — keep it a `container` style because the switcher needs `mouse_area.on_enter`, not button press.
- Full-screen dismiss `mouse_area.on_press(SwitcherCancel)` → unchanged (orchestration).

- [ ] **Step 1:** Read `switcher/view.rs` fully. Note the `real` palette capture, the per-card `is_selected`, the `mouse_area.on_enter` hover, and the backplate.
- [ ] **Step 2:** If needed, add `card::list_tile_style(selected: bool) -> container::Style` to `crates/sola-kit/src/components/card.rs` (+ storybook + commit `feat(sola-kit): list_tile container style`) BEFORE editing the switcher. (This is the switcher's analog of `button::list_item` for the `mouse_area`-wrapped, non-button case.)
- [ ] **Step 3:** Replace the backplate with `card::accent_backplate(...)`; delete the `real` capture.
- [ ] **Step 4:** Replace the per-card container style with `card::list_tile_style(is_selected)`, keeping the `mouse_area(...).on_enter(Msg::SwitcherHover { index })` wrapper, padding `[16,20]`, and the icon/label column.
- [ ] **Step 5:** `cargo make build sola-shell` passes; no captured-`shell.theme` closures remain.
- [ ] **Step 6:** Commit `refactor(sola-shell): switcher on kit accent_backplate/list_tile`.

**Acceptance:** Switcher shows the primary-tinted backplate; the selected app card is an accent pill; hover moves selection; dismiss on outside click — all from the ambient theme.

---

## Phase 4 — Menubar → kit

**Files:** `crates/sola-shell/src/menubar/view.rs`

Target mapping:
- `highlight_container` helper (white @ 15% when active) → delete. Replace each label/title/system-button with `iced::widget::button(inner).style(kit_btn::menubar(is_active)).on_press(...).padding([2,8])`. Keep `.on_press(Msg::OpenMenu{..})`; the current `on_enter(Msg::HoverMenu{..})` hover-to-switch behavior must be preserved — `button` has no `on_enter`, so wrap the button in a `mouse_area(...).on_enter(Msg::HoverMenu{..})` (the button handles press + hover-fill; the mouse_area adds the hover-switch signal). Verify mouse_area+button composition delivers both (the terminal-sidebar work established that a `button` captures the press; here press is wanted on the button and only `on_enter` is needed from the mouse_area, which fires on a different event, so there is no capture conflict — confirm in build/behavior).
- System flower icon button → same treatment; keep `icon_colored("sola/flower", 16, WHITE)` as the button content.
- Clock + toast → unchanged.

- [ ] **Step 1:** Read `menubar/view.rs` fully (the `view`, `highlight_container`, and `app_menu_labels` helper).
- [ ] **Step 2:** Convert the system button, app title, and each menu label to `button(...).style(kit_btn::menubar(active))` wrapped in `mouse_area(...).on_enter(Msg::HoverMenu{..})`. Preserve `is_active` (menu-open) per element and the `has_menu` gating on the title.
- [ ] **Step 3:** Delete `highlight_container`.
- [ ] **Step 4:** `cargo make build sola-shell` passes.
- [ ] **Step 5:** Commit `refactor(sola-shell): menubar labels on kit button::menubar`.

**Acceptance:** Menubar still black; labels show a translucent highlight on hover and when their menu is open; clicking opens, hovering while a menu is open switches menus; clock/toast unchanged.

---

## Phase 5 — Cleanup & verification

**Files:** all touched.

- [ ] **Step 1:** Grep `crates/sola-shell/src` for residual inline chrome that should be kit: `extended_palette()` closures, `Color::from_rgba(... 0.15 ...)`, hardcoded card bg/border/shadow. Anything left should be deliberate orchestration (backdrops, anchor padding) — note each in the commit body.
- [ ] **Step 2:** Confirm no unused imports / dead helpers remain (`cargo make build` warnings clean for the touched files).
- [ ] **Step 3:** `cargo make build` (whole workspace incl. `sola-kit` storybook binary) passes. `cargo test -p sola-kit` passes.
- [ ] **Step 4:** Final review pass (dispatch a code-reviewer over the full diff vs the phase-0 base). Address issues.
- [ ] **Step 5:** Update `docs/specs/2026-05-22-sola-shell-iced-port-design.md` status note (or add a pointer) indicating the kit-ification follow-up is complete, and mark this plan done.
- [ ] **Step 6:** Report to the user for smoke-testing (the user runs `cargo make install` themselves; do not install).

---

## Self-review notes (risks & decisions)

- **Overlay theme correctness is load-bearing** and is verified above; Task 0.6 must land before any surface migration, and each surface task asserts "no captured `shell.theme` closures remain."
- **`text_input` background goes transparent** inside overlays (only consumer of `background.base.color`). This is current behavior; acceptable. If the launcher field needs an explicit fill later, add a `text_input::style_filled` variant — out of scope here.
- **Exact radii/paddings:** the kit `RADIUS_*` consts may not match every original literal (menu 6px, launcher 14px, switcher 16px, items 4/8px). Prefer the kit's scale for consistency; only preserve an exact literal when a build/visual check shows the kit value is wrong. Flag any deviation in the task's commit body.
- **Menu hover color:** `list_item(false)` hover = `background.strong`; the original menu used accent on hover. If accent-on-hover is wanted, add a `button::menu_item` variant. Defaulting to `list_item` for cross-surface consistency; revisit if it reads worse.
- **`button` vs `mouse_area`:** switcher and menubar need `on_enter` (hover) which `button` lacks; those keep a `mouse_area` wrapper (switcher uses a container+style for the tile; menubar uses button-for-press + mouse_area-for-hover). The terminal-sidebar work documented the press-capture interaction between `button` and `mouse_area`; apply the same understanding.
- **No installs.** Building verifies. The user smoke-tests.
