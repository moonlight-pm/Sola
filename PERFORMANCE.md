# PERFORMANCE — GPU / idle track

**Living program log** for compositor and iced present cost. Not a session
handoff (that stays [`CURRENT.md`](CURRENT.md)) and not a freeze.

**As-built tables:** [`docs/architecture.md` § Iced present / GPU idle](docs/architecture.md#iced-present--gpu-idle-as-built)  
**Capability:** [`docs/capabilities.md`](docs/capabilities.md) row `gpu-idle`

Update this file when a mitigation lands, a smoke result changes, or the
next perf slice is chosen. Keep CURRENT as a one-line pointer.

---

## Law

Iced 0.14 **GPU-presents every window in the process after any `Message`**.
A timer or `window::frames()` is a full-window present loop, not a cheap
poll. **Do not re-introduce always-on vsync pumps** to fix a gesture or
helper drain.

If a widget needs motion, subscribe or `request_redraw` **only while that
motion is live**. Helper threads must `chrome_wake::wake()` (or equivalent).

River GLES **cannot scan out ARGB** unless the client sets a full
`wl_surface.set_opaque_region`. Tiled kit windows must look opaque to the
compositor; float CSD still needs an alpha swapchain.

---

## Dogfood box

River 0.4.5, NVIDIA **RTX 3090 Ti**, output **5120×2160**. Binaries under
`/opt/sola/bin/`. NVML util is **relative to current clocks** — 20% at
P8 / 210 MHz is not the same work as 20% at boost.

**Baseline (this track, before mitigations):** ~**30–40%** GPU at all
times with several kit apps open.

---

## Latest measurement (2026-08-25)

Live desk (two browser profiles, workspaces, terminal, mail, arcade,
preview, kit storybook, `sola-kit-spike`). **15 s** `nvidia-smi`:

| | |
|--|--|
| GPU util | **12–26%**, mean **17.7%** |
| P-state / clocks | **P8**, 210 MHz graphics / 405 MHz mem |
| Power / temp | **~21 W**, 37–38 °C |
| VRAM | **2.7 / 24 GB** |

When pmon reported SM: **river ~14%**, **workspaces ~11–12%**. Shell VRAM
~313 MB (five iced windows of wgpu overhead, not four leftover 5K
swapchains).

**This sample did not fully exercise opaque-region.** After `install` of
all kit apps, only **shell** and **browser `--engine` helpers** re-exec’d.
Workspaces, terminal, mail, browser chrome, kit, arcade, and preview
kept 12-hour / morning PIDs (self-watch miss). Re-measure after those
windows are actually restarted.

---

## Shipped

| Mitigation | Where | Status |
|------------|--------|--------|
| No 16 ms chrome timer | `sola-browser` `subscription` | **Dogfooded.** Copy / context menu wake via `chrome_wake::wake`. |
| Chrome `Tick` is not a 250 ms pump | `sola-browser` `Msg::Tick` | **Dogfooded.** 250 ms `time::every` only for copy-URL flash, vault TOTP on the open item, vault fill-wait. Helpers wake Tick. |
| Working ring `At` ~20 Hz | `sola-kit` `status_mark` | **In code.** `RedrawRequest::At(50ms)`, not `NextFrame`. |
| Workspaces pointer gated | `sola-workspaces` | **In code.** No `window::frames()`; `CursorMoved` only while split-dragging. |
| Morph2 drag-only vsync | `sola-kit` `sidebar/strip.rs` | **Dogfooded** for tab reorder (stutter fixed by re-enabling vsync *during* drag). Idle must not leave `request_redraw` on. |
| Overlay first-show at live output | `sola-shell` `overlay_open_rect` | **Dogfooded.** No 1920-wide placeholder / default-center jump. |
| River hide until Composition | `sola-river` `last_composition` | **Dogfooded** with overlay park (no center flash of hidden surfaces). |
| Shell overlays parked 2×2 off-output | `sola-shell` `zoning::overlay_frame` | **Dogfooded** (position, then flash, then iced `Resized` gate). Show is Frame while hidden, Composition after live `Resized` + one tick. **Never park at 1×1** (winit min 2×1 + `resizable=false` → `xdg_toplevel` invalid_size panic-loop). |
| Menu overlay is card-sized | `sola-shell` `zoning::menu_overlay_frame` | **In code (2026-08-31).** Dropdown / calendar / stat / BT / volume / pile Frame to the card, not the full usable area. Full-output wgpu on software GL (llvmpipe) pegged a core on first click. Launcher / switcher / selection still full-output. |
| Super+K overlay is card-sized | `sola-shell` `zoning::card_overlay_frame` | **In code (2026-09-02).** Cheatsheet Frames to the card + shadow pad (~584×575 at 1920×1080, ~0.3M px vs ~2.0M usable). Same leftover-pad dismiss as menus; desk clicks pass through. Park stays 2×2 off-output. Launcher / switcher still full-output (their dim / click-outside *is* the usable area). |
| Tiled kit opaque-region | patched `iced_winit` `State::synchronize` | **In code, not fully smoked.** ARGB kept for float CSD. Tiled `theme_for(false)` → full opaque-region. Float / overlay theme → region cleared. |

Install that landed this track (2026-08-25): `shell` `agent` `arcade`
`browser` `install` `kit` `mail` `monitor` `paint` `preview` `settings`
`terminal` `workspaces`. (`agent` was in that bulk install; crate retired
2026-08-28.)

---

## Not fully smoked

| Item | Why it is still open |
|------|----------------------|
| Opaque-region / GLES scanout | Session-spawned kit apps did not re-exec on the 2026-08-25 install. Restart workspaces, terminal, mail, browser chrome, kit, arcade, preview, then watch **river SM %** at idle. |
| Self-watch on `cargo make install` | Those same PIDs stayed up for hours after the binary was replaced. Shell and CEF engines did restart. Need to know why session children missed the inode swap. |
| First overlay after a **fresh shell** | Parked 2×2 surfaces still have to map once before Super+Space can skip create/map. Later opens are Frame + stack. |
| Workspaces blink | `BlinkTick` every **530 ms** is still a full-window present (~2 Hz) while a cursor is blinking. Visible in the 2026-08-25 sample (workspaces ~11% SM, ~16% CPU). |
| Browser click | `LeftPressed` / `CursorReleased` still present chrome once per click. Marked acceptable. |
| Working ring at desk | Storybook Sidebar / a Working Grok row can still pin GPU if `RedrawRequested` is left chained. |
| Overlay `Resized` hang | If iced never reports ≥64×64 after Frame, Super+Space would not join Composition. Not seen after the iced-`Resized` slice. |

---

## Next / later

**Next (as of 2026-08-25):** River **NVIDIA knobs**, only after clients
stop presenting storms and after opaque-region is actually live on tiled
kit windows.

**Later / park**

- Workspaces (and terminal) **cursor blink** without a process-wide
  present — `request_redraw` only on the cell that blinks, or a cheaper
  tick that does not redraw the whole 5K window.
- Find and fix **session self-watch** so `cargo make install <app>`
  always re-execs user apps, not only managed `sola-*` daemons.
- Confirm **direct scanout** (or at least no ARGB blend) for tiled
  opaque-region windows — `WAYLAND_DEBUG` / river logs, not NVML alone.
- Opaque **XRGB** swapchains while tiled (today: ARGB + opaque-region
  hint). Only if region is not enough for scanout.
- Browser chrome present on every click (only if it shows up in idle
  traces).
- Extra iced windows left open (storybook, preview, arcade, kit-spike)
  still cost a present each; that is operator hygiene, not a code bug.

---

## How to re-measure

```bash
# Whole GPU, 15 s
for i in $(seq 1 15); do
  nvidia-smi --query-gpu=utilization.gpu,power.draw,clocks.gr,pstate --format=csv,noheader,nounits
  sleep 1
done

# Who is on the GPU
nvidia-smi pmon -c 5 -s um

# Did this process pick up the last install?
stat -c '%y' /opt/sola/bin/sola-workspaces
ps -o lstart,etime,args -C sola-workspaces
```

Compare against the baseline (~30–40%) and the 2026-08-25 row above.
If p-state leaves P8 at a quiet desk, something is presenting again —
check the architecture regression column before adding a timer.
