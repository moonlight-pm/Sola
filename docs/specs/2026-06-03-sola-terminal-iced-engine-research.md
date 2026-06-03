---
title: sola-terminal iced port — terminal-engine research
date: 2026-06-03
status: research / decision-input
---

> Multi-agent research output (7 candidate engines investigated + adversarially
> verified). Decision: depend on `alacritty_terminal` directly, render the grid
> ourselves in iced, keep Sola's existing tmux/openpty backend. Ghostty parked on
> the watchlist. See the synthesis below; per-candidate findings live in the run
> transcript.

# Sola Terminal Engine — Decision-Grade Synthesis

The brief: keep Sola's existing nix::openpty + tmux backend and raw-PTY-byte reader thread. We need ONLY a terminal-emulator layer (VTE parse → cell grid → readable each frame) plus the reverse path (input events → bytes). Render in iced 0.14 (wgpu/wayland/NVIDIA). No candidate may own our PTY, window, or event loop.

---

## 1. Comparison Matrix

| Candidate | Embeddable as lib (feed bytes / read grid)? | Owns PTY / spawns shell? | Ships renderer / render ourselves | iced 0.14 + wgpu + wayland + NVIDIA fit | Maintenance | License | Verdict (confidence) |
|---|---|---|---|---|---|---|---|
| **alacritty_terminal** | **Yes** — `Term<T: EventListener>` + `vte::ansi::Processor::advance(&mut term, bytes)`; read via `renderable_content()` / `grid()`; `resize`, `scroll_display`, `selection_to_string`, `damage` | **No** — `tty`/`event_loop` modules are optional; core needs no PTY/window/loop | No renderer (headless). We render in iced. | **Strong** — concrete proof: iced_term 0.8.0 pins iced 0.14.0 + canvas/wgpu; cosmic-term runs it on glyphon/wgpu at scale | Excellent (live Alacritty engine; latest **0.26.0**, 2026-04-06). Embedder API breaks across minors — pin exactly | Apache-2.0 (permissive) | **strong-fit (high)** |
| **iced_term** (Harzu) | Partial — clean iced API, but grid only via its own widget; raw bytes only reach its **spawned** PTY (`ProxyToBackend → backend::Command::Write → private notifier`) | **YES** — `tty::new()` + `EventLoop::spawn()`; `from_fd` is NOT an attach hatch (it re-spawns a Child) | Ships renderer — custom `Widget` per-cell `fill_text` (cosmic-text), batched bg rects | **Strongest on paper** (purpose-built for iced 0.14) but no NVIDIA/wayland example; reuse requires a **fork** | Active, single maintainer (Harzu); API unstable pre-iced-1.0 | MIT (+ Apache-2.0 backend) | **viable (high)** — as reference/fork target, not drop-in |
| **wezterm-term** (+ termwiz/vtparse) | **Yes** — maintainer's own recommended path: `Terminal::advance_bytes(&buf)` → `screen()`; `key_down`/`mouse_event` produce input bytes; `seqno`/`changed_since` dirty-line API | **No** — "no GUI, does not manage a PTY"; caller passes a `std::io::Write` | No renderer in the crate. We render in iced. | Good in principle; **zero** iced precedent; all glyph work ours | Excellent code (battle-tested in wezterm) but **not on crates.io** — git-rev only; no semver contract | MIT throughout | **viable (high)** |
| **Ghostty / libghostty-vt** (libghostty-rs) | **Yes** — `Terminal::new`, `vt_write(&bytes)`, `on_pty_write`, `RenderState::update` → row/cell iterators; ships **input encoders** (kitty kbd/mouse) | **No** — pure state, owns nothing | No renderer (RenderState is the data API). We render in iced. | No iced/wgpu integration; lib is CPU-only (no GPU constraint). Precedent: gpui-ghostty | Active (Ghostty maintainers) but **pre-1.0, UNTAGGED on both layers**; needs **Zig toolchain** in build; `!Send+!Sync` | MIT (both lib & bindings; whole Ghostty app is MIT too) | **viable (high)** |
| **Rio ecosystem** (rio-backend / sugarloaf / copa) | **Poor** — `crosswords` grid not separable: hard-deps sugarloaf + teletypewriter + rio-window, `use sugarloaf::…`, generic over `RioEvent`. Only clean piece is `copa` (bare VTE, no grid) | No (PTY isolated in teletypewriter) but grid is wired to Rio's event proxy | Ships sugarloaf (wgpu) but it **self-owns Instance/Device/Surface**, no external-target injection — can't draw into iced | **Poor** — sugarloaf can't share iced's wgpu device/surface | Active, single maintainer; crates are de-facto internal, lockstep 0.4.x | MIT (copa Apache/MIT) | **poor-fit (high)** |
| **DIY parser** (vte / vtparse / vt100) | **Best-in-class** — pure compute. `vte`/`vtparse` = callbacks into your grid; `vt100` = parser+grid (`Parser::process`, `screen().cell()`) | **No** — none touch PTY/window/loop | No renderer. We render in iced. | Zero graphics deps → no wgpu conflict; all glue ours | vte 0.15 (de-facto standard, ~51M dl); vt100 0.16.2 small/active | All permissive (Apache/MIT) | **viable (high)** — fallback baseline |

---

## 2. Recommendation

**Primary: depend on `alacritty_terminal` directly (pin `=0.26.x`), drive its `Term` from our existing tmux/openpty reader via `vte::ansi::Processor::advance`, and render the grid ourselves in iced. Use `iced_term` 0.8.0 as a worked code reference (its `view.rs` and input bindings), NOT as a dependency.**

Why it wins for *our* constraints:

- It is genuinely a headless parse+grid library. The parse+grid path (`Term` + `vte::ansi::Processor`) has **no PTY, window, or event-loop dependency** — verified against `event_loop.rs`, which itself just calls `parser.advance(&mut **terminal, &buf)`. We do exactly that from our reader thread. The shell-spawning `tty`/`event_loop` modules are optional and we ignore them.
- The iced 0.14 fit is **proven, not theoretical**: `iced_term` 0.8.0 pins `iced = 0.14.0` (features canvas/lazy/advanced) and renders an alacritty grid through iced's wgpu canvas. Separately, `cosmic-term` drives the *same* alacritty grid onto glyphon/wgpu — the exact text stack iced uses — at production scale. No NVIDIA/Vulkan/wayland blocker exists; it's ordinary wgpu.
- Permissive **Apache-2.0**. (Read `cosmic-term` for technique, but it's GPL-3.0 — do not copy its code.)
- Everything we must preserve is supported at the grid layer: scrollback (`Grid` history + `display_offset`/`scroll_display`), selection + `selection_to_string`, truecolor/256/named per `Cell`, resize/reflow, wide-char flags, OSC-8 hyperlinks, bracketed-paste via `term.mode()`.

### The two anchors, answered plainly

**alacritty_terminal (anchor #1): YES — this is the pick.** It is the only candidate that is simultaneously (a) a true feed-bytes/read-grid library, (b) Apache-2.0 permissive, and (c) backed by a *current, concrete* iced-0.14 reference plus a production wgpu user (cosmic-term) on iced's own glyphon text stack. Note the version skew the findings flag: `iced_term` pins `0.25.1`, but the current release is **0.26.0** — depend on 0.26.x directly and re-derive `renderable_content`/`EventListener` against it. Its embedder API is not contractually stable (breaks across minors), so pin exactly and budget migration.

**ghostty / libghostty-vt (anchor #2): NOT a realistic primary today — keep it on the watchlist, do not build on it now.** It is technically the *cleanest* embedding model (pure state, owns nothing, and uniquely it **ships input encoders** — kitty keyboard + mouse — that everyone else makes us hand-roll). But three things disqualify it as the primary for a 2026 iced port:
1. **Pre-1.0 and UNTAGGED on *both* layers** — the upstream C API explicitly expects breaking changes, and the Rust binding is `0.1.1` moving weekly (selection API landed only 2026-05-28). No pin gives you a stable contract.
2. **Zig toolchain injected into the build** — `build.rs` fetches Ghostty source at a pinned commit and runs `zig build`. There's a documented Nix contract (`GHOSTTY_SOURCE_DIR` + `GHOSTTY_ZIG_SYSTEM_DIR`, a flake), so it's tractable on NixOS, but it's real friction against our pure Rust+cargo+nix build.
3. **No iced integration of any kind** and a community-published (not ghostty-org) binding (bus-factor, though it's Ghostty *maintainers*, not a lone hobbyist).

The license fear in the raw research is **wrong and should be discarded**: Ghostty is MIT (not GPL) end-to-end — the verification corrected this. So if libghostty-vt tags a stable release and someone ships an iced bridge, it becomes the most attractive engine (notably for its built-in input encoders). Today it's a viable *future* option, not the one to start on.

---

## 3. Recommended Rendering Strategy (iced 0.14)

Two-phase, ship the simple thing first:

**Phase 1 — MVP via iced `canvas` (crib `iced_term`'s `view.rs`):**
- Iterate `term.renderable_content()` each render. Behind an iced geometry `Cache`:
  - batch contiguous same-bg-color runs into `frame.fill()` rects,
  - draw underlines via `frame.stroke()`,
  - emit one `frame.fill_text()` per cell with `Shaping::Advanced` and per-cell `FontWeight`/`FontStyle`.
- Glyphs ride iced's built-in cosmic-text/glyphon stack — **no second font stack**, and we get unicode/wide-char shaping for free. This is the proven path (`iced_term` does exactly this on iced 0.14).
- **Ligatures: intentionally out of scope** — a fixed cell grid neither supports nor wants them. Per-cell shaping is correct terminal behavior.

**Phase 2 — only if Phase 1 underperforms:** custom `iced::advanced::Widget` (or `iced::shader`/wgpu pipeline) emitting **batched instanced quads + a glyph atlas** (glyphon, the cosmic-term technique). Driven by alacritty's `damage()`/`reset_damage()` so only dirty cells redraw.

**Why a benchmark gate matters:** the `Cache` helps when content is static, but a busy `tmux` pane (scrolling build log) invalidates it **every frame**, so per-glyph `fill_text` is the real risk. **Benchmark a full-screen scrolling redraw on the NVIDIA box early** and let that result decide whether Phase 2 is needed.

---

## 4. Integration Sketch — alacritty_terminal in Sola

**Construction.** On the emulator thread, build `Term::new(config, &dimensions, event_proxy)` + a `vte::ansi::Processor`. `event_proxy` is our `EventListener` impl (single method `send_event`) that forwards Term events onto Sola's bus / channels.

**PTY bytes IN.** Our existing reader thread already produces raw bytes (today base64'd to xterm.js — drop the base64). Hand them to the emulator thread; call `processor.advance(&mut term, &bytes)`. That drives the ANSI state machine and mutates the grid. No I/O happens inside the library.

**Grid OUT each frame.** iced `view`/`draw` reads `term.renderable_content() -> RenderableContent { display_iter, cursor, selection, display_offset, colors, mode }` and iterates `Cell`s (char + fg/bg + flags). Use `damage()`/`reset_damage()` to cache unchanged rows.

**Input OUT (the part the library does NOT give us).** `Term` does *not* encode key/mouse → bytes; that lives in alacritty's app layer. We **port iced_term's `bindings` module** (key → CSI-u/kitty, mouse → SGR per `term.mode()`, bracketed paste) to emit bytes, then write them to **our** PTY fd (the same write side we already own).

**The PtyWrite back-channel (don't drop this).** Some Term events (DSR/DA cursor-position & device-attribute replies, OSC-52 clipboard) arrive as `Event::PtyWrite`. Our `EventListener` must forward `PtyWrite` into our PTY writer, or some TUIs misbehave. Title/Bell/ClipboardStore route to the bus/tab state.

**Scrollback / selection / resize mapping:**
- **Scrollback ownership — settle this in the spike.** tmux is already the persistence/history source of truth; alacritty's `Grid` *also* keeps history. Recommended model: **alacritty `Grid` = live viewport (+ modest local history); tmux = session persistence.** Avoid two competing scrollback authorities.
- **Selection:** drive `term.selection` geometry from cell hit-testing; copy via `term.selection_to_string()` → wire to wayland clipboard through our bus.
- **Resize:** on pane resize, call `term.resize(dims)` **and** drive the resize into tmux (our existing path) so both agree on size; let tmux own wrapping authority.

**Risks/unknowns to resolve in a spike (ranked):**
1. **Canvas perf on full-screen scrolling redraws** on NVIDIA — gates the Phase-1/Phase-2 decision. Benchmark first.
2. **Scrollback ownership** (tmux vs alacritty Grid) — design decision; pick "Grid = viewport, tmux = persistence."
3. **Input-encoding port** correctness (mouse SGR modes, kitty keyboard, bracketed paste) against real TUIs (vim, htop).
4. **PtyWrite forwarding** wired (else DA/DSR-dependent apps break).
5. **API churn**: build against **0.26.x**, not iced_term's 0.25.1 pin; expect to re-derive types on upgrades.
6. cfg-gated `rustix-openpty` pulled in unused — confirm no link conflict with `nix::openpty` (different layer; should be harmless).

---

## 5. Ranked Fallback Order

1. **alacritty_terminal (direct dep) + render-ourselves** — *primary*. Strong-fit, Apache-2.0, proven iced-0.14 reference (iced_term) and production wgpu user (cosmic-term).
2. **Fork `iced_term`'s backend** — same engine, but reuse its `view.rs` renderer and bindings wholesale; replace its `Backend` so `processor.advance()` is fed by *our* bytes instead of a spawned shell. Faster to a working pixel; cost is carrying a fork on an explicitly-unstable widget. Choose this over #1 only if the canvas renderer + bindings port turns out to be the bulk of the work and the fork is cheaper to maintain than re-authoring.
3. **wezterm-term** — battle-tested model, maintainer-endorsed embedding path, and it **gives us input encoding** (`key_down`/`mouse_event`) which alacritty does not. Drops to this if alacritty's reflow/scrollback ergonomics or API churn become painful. Cost: **git-rev-only** dependency (no crates.io, no semver) — friction for reproducible NixOS/vendored builds — plus zero iced precedent.
4. **DIY parser (`vt100`, else `vte`/`vtparse` + own grid)** — lowest coupling, highest control, all-permissive, no graphics deps to conflict with wgpu. Use `vt100` for a fast feed-bytes/read-grid MVP; fall to `vte`/`vtparse` if `vt100`'s scrollback/reflow/OSC-8 gaps bite. The parser is the easy 10%; reflow + selection + clipboard are the real cost.
5. **libghostty-vt (Ghostty)** — *future watchlist*, not now. Cleanest embedding model and unique built-in input encoders, MIT throughout. Revisit when it tags a stable release and an iced bridge exists; today the pre-1.0/untagged-on-both-layers churn + Zig-in-the-build cost outweigh the elegance.
6. **Rio ecosystem** — *not recommended*. The grid (`crosswords`) is not separable (hard-deps sugarloaf/teletypewriter/rio-window + Rio's event proxy), and sugarloaf can't render into iced's wgpu surface. The only clean piece (`copa`) is a bare VTE with no grid — i.e., no advantage over upstream `vte`.

---

### Honest remaining unknowns
- **No candidate has a tested iced 0.14 + wgpu + wayland + NVIDIA terminal example.** The path is "ordinary wgpu, should work" plus strong adjacent proof (iced_term on iced 0.14; cosmic-term on glyphon/wgpu) — but the specific NVIDIA-wayland-scrolling-perf result is unproven and is the #1 spike item.
- **alacritty_terminal's "no stable embedder API"** is an *inference* (lockstep releases, CHANGELOG breaks, iced_term pinning an exact version), not a documented upstream policy — but the practical conclusion (pin exactly, budget churn) holds regardless.
- **Two-emulators concern** (tmux + our emulator) is not a regression — it's exactly what xterm.js does today — but the scrollback-authority decision must be made deliberately, not by default.
