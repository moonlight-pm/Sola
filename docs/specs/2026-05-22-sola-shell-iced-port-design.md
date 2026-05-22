# sola-shell → iced kit (sola-kit) Port

**Status:** draft, awaiting review
**Date:** 2026-05-22
**Owner:** Joshua

## Goal

Port `sola-shell` from CEF + Remix v3 (consumed via `sola-kit-legacy`, imported as `sola_kit`) to iced (consumed via the new `sola-kit`). Faithful parity with today's four-window shell — menubar, launcher, switcher, menu — plus zoning and all bus integrations. No UX redesign.

Motivation: the CEF shell spawns ~10 helper processes (multiple zygotes, GPU, utility, renderer-per-browser) to render maybe 200 simple DOM nodes. The desktop nerve center should not need a Chromium runtime. iced rendering is GPU-cheap, single-process, and the shell becomes much easier to debug.

## Scope

Big-bang. All four windows ported in one branch, swapped atomically when the new shell hits parity. The legacy CEF shell is preserved as `sola-shell-legacy` so it remains runnable during the port and as a fallback after.

**In scope:**

- New `crates/sola-shell` built on iced via the new `sola-kit`.
- All four windows: menubar, launcher, switcher, menu — same surfaces, same lifecycles, same compositor-level behavior.
- Zoning (`zoning.rs`) carried over largely intact — its logic is framework-agnostic; only the bus glue around it changes.
- Every bus topic emitted by today's shell, identical payloads and semantics: `Frame`, `Composition`, `Focus`, `LaunchApp`, `CloseApp`, `MenuAction`, `Shutdown`, `Theme`, `Zones`, `RegisteredChords`.
- Every subscription today's shell consumes: `Windows`, `Zones`, `SetAppMenu`, `OutputGeometry`, `MouseEntered/Clicked/Left`, `Chord/ChordReleased`, `LaunchResult`, `UserAppExited`, `Application`.
- Whatever additions the shell discovers it needs from `sola-kit` along the way: an icon-loading helper, a periodic-tick subscription wrapper, an animated-value primitive for the toast fade, possibly an `iced::widget::svg` wrapper that defaults to `currentColor` theming.
- Old crate renamed to `crates/sola-shell-legacy`; its binary becomes `/opt/sola/bin/sola-shell-legacy`. The launcher gets a new builtin entry (`"Shell (Legacy)"`) so the user can run it manually for comparison until it's retired.

**Out of scope:**

- UX redesign or visual changes beyond what falls out of an iced-native rewrite (tighter padding from native widget metrics, etc.).
- Real icon rendering for *external* apps. The placeholder `⬡` situation predates the port and is addressed here only insofar as `sola-*` builtin icons (lucide names already in `Application::icon`) can be wired up via sola-assets; freedesktop icon-theme lookup for external apps is a separate project.
- Promoting shell-specific components into `sola-kit`. Components live in `crates/sola-shell/src/components/` until something proves reusable; promotion is then a one-file move.
- Theme editor UI (the storybook owns that).
- An automated UI test harness. No such thing exists today and adding one isn't part of the port.

## Architecture

### Process & window model

One iced `application` process. Four `iced::window`s created at startup. iced 0.14 supports multi-window via the standard `application` builder + `iced::window::open` for windows beyond the initial one; each window has its own `view(&self, window: window::Id) -> Element` returning a different root based on `Id`. Shared state lives in one `Shell` struct; the four window views are facets onto it.

Window-geometry policy carries over from today's shell: all four windows exist at their final size and position from the first `Topic::OutputGeometry` tick; show/hide is done by including or excluding the window's `CompositionEntry` from `Topic::Composition`. No window resize on open/close — the surface is always at its final geometry, so there's never a resize-then-show lag.

### Bus integration

`sola_kit::app::bus_subscription()` returns a `Subscription<Arc<Message>>` that drains the kit-owned bus client. The shell wires it into `subscription()`, maps to `Msg::Bus(Arc<bus::Message>)`, and `update` dispatches via `Topic::parse(...)`. Same pattern as `sola-monitor`; the shell just consumes many more topic kinds.

Emit side: `sola_kit::app::bus().lock().emit(Topic::*)`. Today's `AppCtx::emit` calls translate one-to-one.

The shell *seeds* `Topic::Theme` with a default at startup. The legacy kit storybook does this today as a TODO-flagged hack; in the new world the shell is the right owner because the shell runs always-on while the kit storybook is a dev tool that may not be running.

### Shared state (`Shell` struct)

The categories from today's `ShellApp` carry over almost verbatim — they're framework-agnostic:

1. **Focus tracking** — `focused_app_id: Option<String>`, `focused_window_id: Option<u32>`.
2. **MRU / ordering** — `mru_apps: Vec<String>`, `mru_window_by_app: HashMap<String, u32>`.
3. **Window registry** — `known_windows: Vec<Window>`, `window_id_by_key: HashMap<(String, String), u32>`.
4. **Application catalog** — `applications: ApplicationsConfig`.
5. **Menu cache** — `menus: MenuCache`.
6. **Focus-hover timer** — `pending_focus_source: Option<u64>`, `pending_focus_generation: u64`. The `AppRuntimeHandle` indirection used today exists because CEF re-enters the app via posted tasks; iced's update loop is the natural re-entry point and a plain `iced::time::every` subscription replaces the handle.

Per-window state, kept as sub-structs (one file per window mirroring today's layout): `zoning: ZoningState`, `switcher: SwitcherState`, `launcher: LauncherState`, `menu: MenuState`.

### Crate shape

```
crates/sola-shell/                  (new — iced)
  Cargo.toml                        path-depends on sola-kit (iced)
  build.rs                          RUNPATH bake for libwayland — same as sola-monitor / sola-kit
  src/
    main.rs                         iced::application boilerplate, BusSetup, fonts, window opens
    app.rs                          Shell struct + update + subscription + view-by-window dispatch
    bus.rs                          Topic dispatch table (parse → update method)
    keys.rs                         keymap install + chord dispatch (carried over, iced-agnostic)
    zoning.rs                       carried over verbatim minus bus glue
    components/                     shell-local iced widgets (clock, toast, menu-item, app-row, switcher-card, system-menu)
    menubar/{mod.rs, view.rs}       window state + view fn
    launcher/{mod.rs, view.rs, state.rs}
    switcher/{mod.rs, view.rs, state.rs}
    menu/{mod.rs, view.rs, state.rs}
    theme.rs                        seed-theme + Topic::Theme observer wiring

crates/sola-shell-legacy/           (renamed from current crates/sola-shell — no code changes other than [package] name → "sola-shell-legacy" and [[bin]] name → "sola-shell-legacy")
  ... existing CEF code unchanged ...
```

`crates/sola-shell` joins the workspace-exclude list (alongside `sola-kit`, `sola-monitor`) so iced's `wayland-sys` dlopen feature flip doesn't unify across the workspace and break `sola-river`. Builds via the existing `sola-make` isolated-crate path.

`/opt/sola/bin/sola-shell` remains the canonical shell path the process manager launches. `sola-shell-legacy` installs alongside.

### Kit additions

These are the items the shell will need that `sola-kit` doesn't have yet. We add them as the shell forces the need — not speculatively.

- **Icon primitive** — `sola_kit::components::icon(name)`. Resolves a `lucide/<name>` (or future `freedesktop/<name>`) string to a static SVG via `sola-assets` and returns an `iced::widget::Svg` themed with `currentColor`. The launcher, switcher, and menubar system-menu all need this.
- **Periodic tick subscription** — thin wrapper around `iced::time::every(Duration)` that emits a typed message. Used by the menubar clock (10 s) and toast auto-hide (5 s, one-shot, generation-counter cancellable).
- **Animated value primitive** — small helper for the toast 200 ms opacity fade. `iced::widget::container` style is static per render; we either re-render at ~60 Hz during the fade window (cheap, captured by a transient subscription) or accept an instantaneous show/hide if the cost feels not worth it. Spec says: ship the latter first, add the fade only if it looks janky.
- **`window_settings` extension** — today's `sola_kit::app::window_settings(app_id)` takes one app id for one window. The shell needs four windows with the same app_id but different geometries and transparency. Either accept a config struct or expose a builder. Simple: extend `window_settings` to take optional overrides.

### Theme

Same architecture as monitor + storybook. `Shell` stores `iced::Theme`; subscribes to `Topic::Theme`; `update` swaps via `sola_kit::theme::from_bus_theme(...)`. Shell-specific component styling reads from `theme.extended_palette()` — no shell-side palette atom additions are expected to be needed because the kit's atoms (bg-primary, bg-secondary, bg-tertiary, border, text-primary, text-tertiary, accent, success, warning, danger) already cover what today's `theme/mod.rs` binds.

## Per-window design

### Menubar

- **Window config:** size `(output_width, 28)`, pos `(0, 0)`, decorated false, transparent true, `keyboard_target: true`, `zoned: false`.
- **Layout:** `iced::widget::row` — left cluster (system-menu logo button → app-title bold → menu labels row starting at index 1), `Length::Fill` spacer, right cluster (toast overlay + clock).
- **Menu labels:** rendered by mapping over the focused app's `AppMenuPayload.menus[1..]`. Each label is a `mouse_area` wrapping a styled `text` — `on_press` emits `Msg::OpenMenu { index }`; `on_enter` emits `Msg::HoverMenu { index }` which only opens if a *different* menu is already open (parity with today's hover-sweep guard).
- **Anchor positioning:** the open-menu position is what made this hard in CEF (anchor_x came from `getBoundingClientRect` in JS). In iced we control layout, so we compute anchor X in `update` from the menu labels' positions. Two implementations possible: (a) widget that reports its laid-out position via a message (`iced::widget::responsive` + `Container::id`), or (b) compute from font metrics + label widths in Rust. Start with (a); fall back to (b) if iced's positioning hooks prove insufficient. Resolution chosen during impl, not in this spec.
- **Clock:** subscription with `iced::time::every(Duration::from_secs(10))` emitting `Msg::ClockTick`; format via `chrono` (already in tree).
- **Toast:** state holds `Option<ToastState { message, generation, until_instant }>`. On `Topic::LaunchResult(err)` or `Topic::UserAppExited`, set toast with `generation += 1`, schedule a delayed `Msg::ToastExpire(gen)` via `iced::Command::perform`. View renders an overlay container above the menubar if toast is active.
- **System-menu logo:** lucide pillars SVG via the new `sola_kit::components::icon("sola/pillars")` (added to sola-assets, sola-shell-specific).

### Launcher

- **Window config:** size `(output_width, output_height - 28)`, pos `(0, 28)`, decorated false, transparent true, `keyboard_target: true`, `zoned: false`. Same composition-only visibility as today.
- **Layout:** full-window transparent backdrop (`mouse_area` capturing background clicks → `Msg::CloseLauncher`). Centered card via `container` + `align_x(Horizontal::Center)`. Inside the card: `text_input` for query (kit-styled; `on_input` → `Msg::LauncherQuery`, focus on open via `text_input::focus(Id)`), `vertical_space(1)` divider, `scrollable` containing filtered rows. Each row is a styled `button` whose body is `row![icon, text(label)]`.
- **State:** `LauncherState { active, prior_focus, query, filtered_ids, selected }` (verbatim from today).
- **Filter:** same case-insensitive substring on `label`, preserving config order.
- **Keys:** while active, arrow keys (`Msg::LauncherNav { up/down }`), Enter (`Msg::Launch(selected)`), Escape (close + restore prior_focus). Driven by `Topic::Chord` from sola-river, plus iced's `keyboard::Event` for arrow keys when focus is on the text input.
- **Outside-click dismiss:** backdrop `mouse_area::on_press` covers our own window; clicks on other windows are caught by `Topic::MouseClicked` on a non-shell surface (already handled in today's app.rs:385 for the menu and trivially extends to the launcher).

### Switcher

- **Window config:** size `(output_width, output_height - 28)`, pos `(0, 28)`, transparent, `keyboard_target: false`. (Same as launcher dims — full overlay; the card centers inside.)
- **Layout:** transparent full-overlay; card auto-centered via `container::center_x().center_y()`. Card body is a `row` of `switcher_card` components — each card column with icon + label, selected card highlighted with accent background.
- **Driving:** `Meta+Tab/Right/Left` from `Topic::Chord` step `selected`. `Super_L` release (`Topic::ChordReleased`) confirms (emit `Topic::Focus`). Escape cancels.
- **Mouse:** `mouse_area::on_enter` on each card sets `selected` (parity with today's `select` IPC).

### Menu

- **Window config:** size `(output_width, output_height - 28)`, pos `(0, 28)`, transparent, `keyboard_target: false`. Dropdown floats on a full-overlay surface, anchored.
- **Layout:** dropdown card positioned by absolute placement inside the overlay. iced doesn't have absolute positioning out of the box for arbitrary x/y inside a window, but `iced::widget::stack` + `padding::Padding { top: 0, left: anchor_x, ... }` on the dropdown wrapper achieves it. Alternative: `iced_layershell` style absolute positioning via a custom widget. Start with padding-based approach.
- **Items:** `column` of menu-item rows. Each row is `button` with `on_press` → `Msg::MenuAction { app_id, action_id }`. Disabled items render with muted style and no `on_press`. Dividers are 1 px `Rule::horizontal`.
- **Dismiss:** Escape (chord), action click, focus change (`set_focus` calls close), `Topic::MouseClicked` on a non-shell window (existing logic).
- **Anchor x:** flows from menubar's reported label position (see Menubar section), stored in `MenuState { anchor_x }`.

## Cross-cutting concerns

### Menu anchor positioning (the hard one)

Today the menubar JS reports `getBoundingClientRect().left` of the clicked label, passes it back to Rust, and Rust re-injects it as inline CSS `left:` on the menu div. Both surfaces share the same Wayland output coordinate space — that's why this works.

In iced there is no shared DOM coordinate space, but the menubar window has a known on-screen origin (0, 0) and the labels are laid out by us. Approach: when computing the menubar view, track label positions deterministically — render labels into a `row![]` with known padding/font metrics, walk the layout, store cumulative X per label index. When the user opens a menu, the `OpenMenu { index }` message carries the precomputed X. Padding-based dropdown positioning in the menu window uses that X.

Risk: iced's text layout uses cosmic-text under the hood and exact pixel widths depend on the resolved font face. Mitigation: use a fixed-width measurement via `cosmic_text` (which iced already pulls in) at the point we compute label widths. Fall-back: use iced's `responsive` widget to capture actual laid-out widths post-layout, then post a message with positions back to state.

### Icons

Today: `⬡` Unicode placeholder. The `icon` field on `Application` is a string like `lucide/settings` but the legacy CEF shell never resolves it.

This port: implement basic resolution for `lucide/*` and `sola/*` via `sola-assets` lookups. `sola_kit::components::icon(name)` returns an `iced::widget::Svg` themed with the kit's text color. External apps (Firefox, etc.) still get the placeholder until freedesktop icon-theme lookup is built (out of scope).

### Outside-click dismiss

Three sources:

1. **Click on a non-shell window** — already handled via `Topic::MouseClicked` from sola-river (today's `app.rs:385`). Carries over.
2. **Click on shell-window backdrop** (launcher, menu) — `mouse_area` covers the transparent background, `on_press` emits dismiss.
3. **Click on the menubar while a menu is open** — closes the current menu (and conditionally opens a different one via the hover-sweep guard).

### Periodic timers and one-shot delays

- **Clock (every 10 s):** `iced::time::every(Duration::from_secs(10))` subscription → `Msg::ClockTick`.
- **Toast auto-hide (one-shot 5 s):** state holds `toast_generation: u64`; show emits `Msg::ToastExpire(gen)` via `iced::Command::perform(tokio::time::sleep(...), ...)`. On receipt, compare against current `toast_generation` and ignore if stale (cancel-by-generation pattern, same as today's focus-hover).
- **Focus-hover (one-shot 500 ms):** same generation-counter pattern; replaces the legacy `AppRuntimeHandle`-based scheduling.

### Transparency

iced 0.14 supports transparent windows on Wayland via `iced::window::Settings { transparent: true, ... }`. Confirmed via storybook playground; details to verify during impl.

### `keyboard_target` vs iced focus

`keyboard_target: true` is a sola-river concept (route keyboard input to this surface). It carries over: the shell still emits `Topic::Focus { window_id }` to direct compositor-level focus. iced widget focus (which text input has the caret) is separate and managed by iced — `text_input::focus(Id)` on launcher open is the analog of today's JS `el.focus()`.

## Migration mechanics

1. Rename `crates/sola-shell` → `crates/sola-shell-legacy`. Update its `[package]` name and `[[bin]]` name to `sola-shell-legacy`. No code changes. Stays in the workspace — no `exclude` entry needed, since it has no iced deps (same status as `sola-kit-legacy` today).
2. Add a new builtin in `applications.rs`: `app_id: "sola-shell-legacy"`, label `"Shell (Legacy)"`. Useful for the user to launch and visually compare, even though only one shell can hold output focus at a time.
3. Create new `crates/sola-shell` (iced). Add to workspace `exclude` list. Add `build.rs` for RUNPATH (same as monitor/kit).
4. Implement window-by-window inside the new crate. Order suggestion (not binding): menubar → launcher → switcher → menu. Each ships in working form before the next begins; the new shell becomes runnable end-to-end at the last step.
5. When the new shell hits parity, `cargo make install sola-shell` swaps the binary in place at `/opt/sola/bin/sola-shell`. The process manager picks up the new binary on next sola launch without changes.
6. After ~one release of soak, retire `sola-shell-legacy` (delete crate, remove from exclude, remove builtin).

## Risks

- **Menu anchor positioning is the hardest single thing in the port.** If iced's layout introspection turns out to be too thin, the fallback (Rust-side font-metric measurement) is more code than ideal. Acceptable because the alternative — keeping a CEF process alive just for the menubar — is worse.
- **Multi-window transparency** is well-supported by iced on Wayland but not heavily exercised. If transparency hits issues, fallback is opaque windows with the existing background fill — visual regression but functional.
- **`keyboard_target` interaction with iced's own focus model** might surprise us. River-level focus and iced-level focus are independent, but the launcher specifically needs both (river routes keys to the launcher window; iced routes the keys to the search input). Test early.
- **`Topic::Composition` ordering** during the swap-in. If the new shell emits composition in subtly different order than the legacy, MRU display order regresses. Carry today's exact ordering algorithm verbatim.

## What this port also fixes (incidental)

- The runaway zygote problem (currently ~9 CEF zygotes for a shell rendering ~200 DOM nodes).
- The `eval_js(format!("..."))` string-IPC pattern (replaced by typed messages in iced).
- The split between Rust state and JS state (everything in one Rust struct now).
- Outside-click handling moves from JS `document.addEventListener` patterns to compositor-level pointer events, which is the right abstraction layer.

## What this port does not fix

- External-app icon rendering still uses the placeholder until freedesktop icon-theme lookup is built.
- The synthesized one-item "Quit App" menu for apps that don't publish `SetAppMenu` carries over unchanged.
- Zoning's coupling to shell-level focus state is unchanged. Whether zoning should move to sola-river is a separate question for later.
