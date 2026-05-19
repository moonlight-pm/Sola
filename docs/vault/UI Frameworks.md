# UI Frameworks

Living record of the toolkit choices we've evaluated for Sola apps and
the shell. The original kit (`sola-kit`) hosts CEF + Remix v3 — great
for prototyping, bloated and not snappy for primary apps. This page
charts our progression to a kit that *is* snappy, top-rendering, and
ours.

## Constraints we filter by

- **Single toolkit** for shell + all primary apps. Custom work fine for
  missing components.
- **Top-tier font/text rendering** — the most important single
  criterion. We render a lot of text and we read it constantly.
- **Snappy.** No bloat, no cold-start hesitation, no jank.
- **Linux only.** We will never target macOS or Windows. Anything that
  needs cross-platform abstraction overhead is paying a cost we don't
  want.
- **Rendering control.** Going low-level when we need to (custom
  shaders, custom glyph atlasing, custom hit testing) must be
  achievable, not blocked by the framework's opinions.

## What's still up

CEF / sola-kit stays for prototypes and web-stack experiments. Won't
disappear — it's the right tool for "I want a webview that talks to
the bus." But primary apps and the shell should move off it.

## Frameworks evaluated

### Iced — *current frontrunner, in evaluation*

- Pure Rust, [iced.rs](https://iced.rs/). Elm-style architecture
  (`Message`, `update`, `view`).
- Text via [`cosmic-text`](https://github.com/pop-os/cosmic-text) +
  [swash](https://github.com/dfrg/swash) — **font hinting + RGB
  subpixel rendering both shipped today** on Linux. This was the
  decisive filter result.
- wgpu-backed renderer. GPU-accelerated.
- Wayland-native via winit; supports layer-shell when needed.
- COSMIC desktop (System76) is built on Iced — strongest production
  proof point for a desktop UI on Linux.

**Rationale.** Only mature framework that passes our strict text
filter today. Architecture is ceremonial (Elm-style) but predictable;
worst case we wrap it. We're starting the evaluation with a port of
`sola-monitor` (see `sola-monitor-iced` crate).

**What we'll learn from the port.** Whether Elm-style scales for
real interactive apps without becoming tedious; whether the bus →
`Message` subscription bridge is clean; whether the theme protocol
maps cleanly to iced's `Palette`/style types; whether wayland surface
behavior under river matches expectations.

### gpui (Zed's) — *deferred, watching*

- [gpui.rs](https://www.gpui.rs/). Apache/MIT, hand-rolled GPU-driven UI
  framework that powers Zed.
- Best-in-class text on macOS (Zed's signature feel).
- **On Linux, text rendering is a known weak spot.** Multiple
  long-running issues about blurry/unhinted fonts, ignoring system
  font settings, fractional-scaling artifacts on Sway. The macOS feel
  has not transferred to Linux yet.
- API is pre-1.0, breaks between Zed releases.

**Why deferred.** The thing we love about Zed — text rendering — is
the thing gpui-on-Linux fails on. Until that gap closes, the
appeal-by-association doesn't hold up under scrutiny.

### Floem — *deferred, watching the text path*

- Lapce editor's framework, [lapce/floem](https://github.com/lapce/floem).
  Fine-grained reactivity (signals, no VDOM).
- Text via [parley](https://github.com/linebender/parley),
  rendering via [vello](https://github.com/linebender/vello).
- **No font hinting, no subpixel RGB rendering today.** Linebender's
  position is that hi-DPI eats the world. Tracked in
  [vello#204](https://github.com/linebender/vello/issues/204).
- Architecture is the most modern of the bunch — signals are how
  reactive UIs ought to work.

**Why deferred.** The architecture is what we'd choose, but the
rendering stack hasn't shipped what we need. If parley+vello close the
hinting/subpixel gap, Floem moves to frontrunner.

### Makepad — *deferred, novel paradigm*

- [makepad/makepad](https://github.com/makepad/makepad). Hit 1.0 in
  2025. GPU-shader-driven rendering — styling compiles to shaders, no
  CSS.
- Used by [Project Robius](https://project-robius.github.io/book/) for
  cross-platform Rust apps (Robrix Matrix client).
- Custom DSL for UI; live-coding capabilities.
- Glyph rasterization via SDF — likely **no subpixel RGB rendering**,
  hinting story unclear.

**Why deferred.** Novel paradigm with smaller community. The shader-
driven style system is interesting but probably hostile to dynamic
theming. Worth revisiting if Iced doesn't pan out.

### Roll-your-own on the Linebender stack — *long-term aspiration*

- [vello](https://github.com/linebender/vello) +
  [parley](https://github.com/linebender/parley) + sctk + custom event
  dispatch + custom layout.
- This is what gpui essentially is, but we'd own every layer.
- Best long-term fit for a hand-crafted desktop, no framework opinions
  in our way.
- Same text-rendering gaps as Floem (parley+vello today).

**Why deferred.** 6-12 months of framework engineering before we get
to write apps. Right answer eventually if no off-the-shelf framework
fits, but Iced is worth a real try first.

### Bevy — *not pursuing*

- [bevy_ui](https://bevyengine.org/). Text via cosmic-text 0.16+
  (hinting), 0.18 added OpenType features. Passes the text filter.
- Game-engine-shaped: ECS architecture for UI is unusual; mature
  desktop apps in the wild are rare. Wrong shape for our usage.

### Excluded

| | Why excluded |
|---|---|
| **GTK4** | Legacy stack we just moved off. Theming on Wayland is a known quagmire. |
| **egui** | Immediate-mode is great for tools and debug UIs, wrong for polished apps. |
| **Slint** | Styling DSL has a fixed property bag, not CSS-flexible. Joshua hit this limit in prior use. |
| **Dioxus / Tauri** | Webview-based — defeats the move off CEF. |
| **Druid** | Superseded by Xilem. |
| **Xilem** | Linebender's evolving framework — too alpha for production apps. Same parley+vello text gap as Floem. |
| **Vizia** | Dormant. |
| **fltk-rs** | C++ wrapper, dated look. Not Wayland-first. |

## Decision matrix snapshot

| Toolkit | Single tk? | Hinting | Subpixel RGB | Snappy | Versatility | Maturity (Linux) |
|---|---|---|---|---|---|---|
| **Iced** | ✓ | ✓ | ✓ | ✓ | ✓ (custom widgets) | ✓ (COSMIC ships it) |
| gpui | ✓ | ✗ on Linux | ✗ on Linux | ✓ | ✓ | ▲ (Zed pre-1.0 churn) |
| Floem | ✓ | ✗ | ✗ | ✓ | ✓ | ▲ (Lapce only) |
| Makepad | ✓ | ? | ✗ (SDF) | ✓ | ✓ | ▲ (small community) |
| Bevy | ✓ | ✓ | ✓ | ✓ | ▲ (game-shaped) | ▲ for UI apps |
| Roll-your-own | ✓ | ✗ (today) | ✗ (today) | ✓ | ✓✓ | ✗ (need to build) |

## Action items (rolling)

- [x] Survey the field, document tradeoffs.
- [ ] Port `sola-monitor` to Iced as `sola-monitor-iced`. Standalone
      crate, no `sola-kit` dependency. Pull `sola-bus`, `sola-core`,
      `sola-assets`.
- [ ] Build a `sola_core::theme::Theme → iced::Theme` converter so the
      iced app participates in our theme protocol.
- [ ] Decide on Iced after monitor port lands and we live with it for
      a while.

## Notes for future-us

If the iced port lands well: port sola-settings, then sola-terminal
(xterm.js → native via wezterm-term or similar), then evaluate the
shell. Shell is the hardest case — transparent overlays, layer-shell,
custom click-to-dismiss — so leave it until the toolkit choice has
been proven on simpler apps.

If iced doesn't pan out: try Floem with whatever the parley/vello text
gap looks like at that point. If still missing, fall back to roll-
your-own on the Linebender stack.

Watch for: parley+vello shipping hinting + subpixel; gpui's Linux text
parity work; Makepad community growth.
