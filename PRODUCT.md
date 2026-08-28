# Product

<!-- impeccable:product-schema 1 -->

## Platform

linux-desktop

Sola is a native Wayland desktop (Iced clients + River), not a website and
not an iOS/Android app. Impeccable’s `web` / `ios` / `android` / `adaptive`
enum does not have a slot for this; treat the value above as the product
truth. Do not run web-only live/detect loops against Rust/Iced sources.

## Users

**Primary:** Joshua — daily driver of Sola on a physical TTY. Designs, dogs,
and ships by using the desktop, not by browsing a mock.

**Secondary (when it exists):** colleagues on the Shape 1 NixOS module /
tarball path. Not a public consumer audience yet.

Job at the kit surface: see whether sola-kit chrome is fit to put in a real
app (settings, mail, browser, workspaces, shell), and edit theme seeds without
leaving the storybook.

## Product Purpose

Sola is a full Wayland desktop environment: process supervisor, bus, call
plane, River compositor bridge, Iced shell, and first-party apps (browser,
terminal, mail, workspaces, arcade, settings, …).

**sola-kit** is the shared Iced app kit and the storybook that dogfoods it.
Every kit visual change is meant to show up in the storybook first.

Success: Joshua can live on Sola; kit chrome is dense, quiet, and consistent
enough that new apps inherit it instead of inventing hex and pads.

## Positioning

Cool graphite tool UI on a multi-process Wayland desk — Iced only, no
WebView for shell or kit apps. macOS Dark Mode is the *craft reference*
(density, hierarchy, materials), not a clone. Intentional departures:
graphite palette, default-float windows, no title bars when zoned, client
decorations when floating, sparse neon cyan accent, graphite selection.

A neighboring Linux desktop (GNOME, Cosmic, niri-as-DE) cannot truthfully
claim this stack + this visual law.

## Operating Context

- Launch: physical TTY → `/opt/sola/bin/sola`. No display manager.
- Agents never `cargo make install` without explicit permission that time.
- Feature code lives in git worktrees; this workspace is already one
  (`naturalethic/kit-design`).
- Theme travels on the bus (`Topic::Theme`); font roles and shell tokens
  ride with it.
- Storybook (`sola-kit` binary) is the kit regression surface. Overview is
  the composition north star; other pages still lag it.
- Visual law: `docs/manual/design-language.md`. Seed hex:
  `sola-kit::theme::hex` synced with `sola-core` `Palette::seed`.
- Dogfood facts and locks: root `CURRENT.md`. Capability maturity:
  `docs/capabilities.md`. Ask-human forks: `docs/open-questions.md` (D1–D3).

## Capabilities and Constraints

- **Stack:** Rust, Iced 0.14 (wgpu, wayland, svg). No HTML/CSS/JS in kit
  or shell. CEF only inside `sola-browser`.
- **IPC:** Sola Bus (fan-out) + sola-call (request/reply) + Wayland.
- **Theme:** 11 seed atoms (including selection). Components read
  `extended_palette()`, never snowflake hex in views.
- **Fonts:** system only (SF Pro Text / Iosevka Term Slab preferred).
  Semantic roles: `ui`, `ui_medium`, `display`, `chrome`, `mono`.
- **Iced limits:** linear gradients only; no backdrop-filter; opaque
  white-mix hairlines instead of alpha borders.
- **Undecided (do not invent):** D1 multi-agent permission fan-out; D2
  kvm permanent input ACL; D3 which call-plane methods need a confirm.
- **Not a claim:** public “install now” readiness. Dist ISO e2e is pending.

## Brand Commitments

- Name: **Sola**. Kit: **sola-kit**.
- Binding visual law: `docs/manual/design-language.md` — cool graphite,
  sparse cyan, quiet selection, status density over hierarchy theater.
  Do not invent a second brand look.
- Voice: labels name user concepts, not internal IDs. Active verbs on
  actions. Errors say what failed and what to do next.

## Evidence on Hand

- Live storybook and installed `/opt/sola/bin` desktop (user installs).
- Seed atoms and materials in `crates/sola-kit/src/theme.rs` and
  `crates/sola-kit/src/components/style.rs`.
- Historical Open Design comps under the Sola OD project (`sola-kit-ds.html`,
  2026-07-24) — **reference only**, not authority. Kit code + design-language
  win.
- Paper file **Sola** (`01KTZ1RMPAVH2AYCTCQ48Q8HE9`): Overview direction
  **A · Desk** approved 2026-08-14.
- Do not fabricate testimonials, user counts, or “ready to install”
  claims.

## Product Principles

1. **Tokens first** — chrome resolves through kit/bus atoms.
2. **Compose, don’t catalog** — storybook Overview proves the system by
   being a desk, not a style-guide poster.
3. **One primary** — at most one filled accent control per group.
4. **Dogfood in the product** — verify kit in the storybook and on the
   live desk, not in a browser.
5. **Ask on Decision points** — D1/D2/D3 stay human; do not invent policy.
