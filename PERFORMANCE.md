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
`wl_surface.set_opaque_region`. A Cinema kit window must look opaque to
the compositor. Since the background picture (2026-10-24) tiles clear
their rounded corners like floats, so they no longer set an opaque region:
River blends them and draws the ground under them where they are damaged
(nothing at idle; a gapped tile never scanned out anyway).

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

## VRAM (2026-11-05)

Live desk: 10.7 GiB of 24 in use, about 80 windows (33 Browser pages, 28
Projects sessions), 54 Design boards open. What holds it:

| What | GiB | Basis |
|------|-----|-------|
| Design board textures (2× raster, with mips) | ~2.7 | calculated from `status` |
| Window buffers (2 per window, hidden or not) | 2.3 | dma-bufs held by rivulet |
| Image atlases (16 MiB per window, empty or not) | 1.3 | `create_texture` log |
| CEF GPU helpers (Browser 0.85, Design 0.04) | 0.9 | `nvidia-smi` |
| 4× MSAA target (largest window the app has had; never shrinks) | ~0.7 | residual after the rest |
| Browser page textures | ~0.5 | residual after the rest |
| rivulet, Steam, Xwayland | 0.5 | `nvidia-smi` |
| Small per-app overhead | ~0.6 | estimate |
| Not attributed to any process | 1.0 | `nvidia-smi` |

**CEF is not where it goes.** Design's GPU helper is 44 MiB, as its boards
are CPU frames. The memory is on the iced side of both apps, and Projects,
with no CEF, was third at 1.5 GiB for the same per-window reasons.

Four mitigations (under [Shipped](#shipped)) should bring that down by about
4.3 GiB: 2 from Design's hidden canvases, 1.2 from the atlases, 0.6 from
MSAA, 0.5 from Browser's page textures. That is an estimate until they run
at the desk. Measured on a private headless desk (windows about 1024×768, so
the buffers and targets are far smaller than at 5120×2160), MiB per process:

| | Before | After |
|--|--------|-------|
| Shell, 7 windows | 134 | 30 |
| Terminal, 1 tab | 63 | 27 |
| Terminal, 6 tabs | 207 | 99 |
| Editor, 1 window | 64 | 27 |
| Projects, 2 windows | 87 | 43 |
| Browser, 12 pages, none looked at | 363 | 179 |
| Browser, 12 pages, all looked at | 403 | 179 |
| Design, a hidden canvas of 2 boards after its 10 minutes | 131 | 107 |

Pictures against the old binaries: Editor and Preview (images of 400, 600,
1200 and 1500 px) are the same pixels. Terminal differs only in underlines,
one sharp row where MSAA blended two. A page returning from hidden shows its
last frame in the first present, then live ones, with the exact CSS colour
in its window's own buffer. A hidden canvas's window came back at its zoom.

Left, cheapest first:

- **A hidden window holds both its buffers.** Reconfiguring its surface to
  1×1 while hidden, without presenting, should leave only the one the
  compositor holds: up to ~1 GiB. Needs a spike: it touches patched
  `iced_winit`, and how the NVIDIA driver behaves is unmeasured.
- **Shell's selection overlay** is a full-output canvas, so the first
  screenshot selection makes an output-sized MSAA target that stays: seen on
  the private desk, about 220 MiB at 5120×2160 by its size, and live Shell
  went from 154 to 414 MiB. Its meshes would need the same audit first.
- **Design's 2× raster** is the largest lever left (about 4× in VRAM and
  in CEF's RAM), but it changes what agent screenshots show: a product call
  ([idea](docs/ideas/2026-10-13-design-memory-budget.md)).

---

## Shipped

| Mitigation | Where | Status |
|------------|--------|--------|
| No 16 ms chrome timer | `sola-browser` `subscription` | **Dogfooded.** Copy / context menu wake via `chrome_wake::wake`. |
| Chrome `Tick` is not a 250 ms pump | `sola-browser` `Msg::Tick` | **Dogfooded.** 250 ms `time::every` only for copy-URL flash, vault TOTP on the open item, vault fill-wait. Helpers wake Tick. |
| Working ring `At` ~20 Hz | `sola-kit` `status_mark` | **In code.** `RedrawRequest::At(50ms)`, not `NextFrame`. |
| Volume spectrum `At` ~20 Hz | `sola-shell` audio meter | **Installed** `shell` release 2026-09-02. `pw-cat` tap → FFT → 12 warped-log bands + pink weight; canvas `RedrawRequest::At(50ms)` only while bands are live. No iced `time::every`. |
| Projects status tick off the UI thread | `sola-projects` `remote.rs` `status_tick`, `presence.rs` | **Applied on novus 2026-11-04 from master@74cd95ae, not published.** Was a 530 ms UI-thread freeze each second with 29 sessions: no `CONFIG_PROC_CHILDREN` on Oath's kernel made the walk read every process's `status` per process walked. Now one `tmux ls` and one read of `/proc/*/stat` per tick, on a worker. Private desk: key to pane 104 → 11 ms median, 620 → 30 ms worst. Live on novus: longest UI-thread stretch 590 → 27 ms, the process 50% → about 4% of a core. |
| Projects redraws what a message changed | `iced_winit` `scope`, `sola-projects` `App::touch_for` | **Applied on novus 2026-11-04 from master@e0cd88d4, not published.** Private desk, 27 windows all on screen: at rest 103 → 2 terminal draws/s, streaming 674 → 118. Live, 26 sessions, at rest: UI thread 0.1% of a core, the process 1.6%. Opt-in: an app that declares nothing takes the old path. |
| Projects pointer gated | `sola-projects` | **In code.** No `window::frames()`; `CursorMoved` only while split-dragging. |
| Morph2 drag-only vsync | `sola-kit` `sidebar/strip.rs` | **Dogfooded** for tab reorder (stutter fixed by re-enabling vsync *during* drag). Idle must not leave `request_redraw` on. |
| Overlay first-show at live output | `sola-shell` `overlay_open_rect` | **Dogfooded.** No 1920-wide placeholder / default-center jump. |
| River hide until Composition | `sola-river` `last_composition` | **Dogfooded** with overlay park (no center flash of hidden surfaces). |
| Shell overlays parked 2×2 off-output | `sola-shell` `zoning::overlay_frame` | **Dogfooded** (position, then flash, then iced `Resized` gate). Show is Frame while hidden, Composition after live `Resized` + one tick. **Never park at 1×1** (winit min 2×1 + `resizable=false` → `xdg_toplevel` invalid_size panic-loop). |
| Menu overlay is card-sized | `sola-shell` `zoning::menu_overlay_frame` | **In code (2026-08-31).** Dropdown / calendar / stat / BT / volume / pile Frame to the card, not the full usable area. Full-output wgpu on software GL (llvmpipe) pegged a core on first click. Launcher / switcher / selection still full-output. |
| Super+K overlay is card-sized | `sola-shell` `zoning::card_overlay_frame` | **Installed** `shell` release 2026-09-02. Cheatsheet Frames to the card + shadow pad (~584×575 at 1920×1080, ~0.3M px vs ~2.0M usable). Same leftover-pad dismiss as menus; desk clicks pass through. Park stays 2×2 off-output. Launcher / switcher still full-output (their dim / click-outside *is* the usable area). |
| Kit opaque-region | patched `iced_winit` `State::synchronize` | **In code, not fully smoked.** ARGB swapchain. Cinema `theme_for(Chrome::Bare)` → full opaque-region. Tiled and floating (overlay theme, corners cleared for the ground — the background picture, 2026-10-24) → region cleared. Possible later: an opaque region minus the four corners (needs winit to take a region, not a bool). |
| Prose reuses shaped paragraphs | `sola-kit` `prose/view.rs` `layout` | **On master (2026-10-19); installed `mail` release.** Every app message rebuilt the view and re-shaped every line of the open letter (cosmic-text, advanced shaping). Lines whose spans + width are unchanged keep their paragraph; a selection drag re-shapes only the lines it touches. Unmeasured. |
| Mail listens to six bus topics | `sola-mail` `main.rs` | **On master (2026-10-19); installed `mail` release.** Was `TopicKind::ALL` — every system fact (windows, stats, …) woke the UI and rebuilt the three panes. Now Theme, MenuAction, CloseApp, Windows, WindowFloating, MailConfig. |
| Design boards paced | `sola-design` `pace.rs` + sola-browser `Cmd::BoardPace` | **Installed** `design` 2026-10-13, smoked. Boards nobody looks at paint at 1 fps (off-screen canvases, out of view on the shown canvas); in view 60 fps with focus, 1 fps without. 2026-10-15: boards are **never `was_hidden`** any more — Chromium keeps saved frames for only 5 hidden views (`FrameEvictionManager`: min(5, 2 + MiB/256)) and evicts the rest; an evicted CEF OSR view shown again gets no BeginFrames, so its board stayed CEF's white background for good (21 of 26 on Wicket `foundation`). Asleep is 1 fps, shown. Before: 7 spinner boards (Wicket `foundation`) held ~3.5 cores idle — perf: CEF GPU process 62% memmove in `FrameSinkVideoCapturerImpl::DidCopyFrame` (whole-surface readback), helper 89% memmove, Design UI 44% memset (`ipc::read_frame` zeroed `vec!`). CPU OSR cost is per frame × surface, so pace is the lever; transport ideas stay parked. |
| sola-design board patches | `sola-browser` `cef/host.rs` frame writer, `paint::changed_tiles` / `pack_patch`, `cpu_import::upload_patch`; `sola-design` `plane.rs`, `frames.rs` | **Installed** `design` release 2026-10-14 (slice 8). A board's frame crossed the frame socket **whole** on every paint (35–55 MB for a tall or 2× board), so a 10 px CSS spinner stuttered and pegged the helper (~146%) and Design (~118%). Now: a board's damage is cut to the 32 px tiles that changed (CEF reports one union rect), sent as a packed patch after one whole frame per size / view, merged in place in the mailboxes (`AbsorbDamage` takes the dropped frame by value), written straight into the board texture, mips rebuilt only over the region; frame wakes paced to ~120 Hz; link → board matching parsed once per view. Copy of Wicket foundation (26 boards, spinners on several): socket ~330 MB/s → ~6 MB/s; Design main thread ~6%; helper ~55%, of which ~80% is the tile compare. Browser tabs unchanged (whole frames). |
| Design boards paced: tiny + wakes | `sola-design` `pace.rs` (`Look::readable`), `frames.rs` | **Installed** `design` release 2026-10-14 (slice 8). On top of `BoardPace`: a board in view but tiny on screen (zoom < 0.3) is `Slow`; only boards in view wake the window (a wake presents it), no faster than their pace (`pace::hz`); the rest upload with the next present. Before (board patches without pacing): GPU 84% (P0, 114 W) — Design re-presented ~120×/s; at fit with these: GPU ~30% (P3, 65 W), Design under pmon's floor. |
| sola-design software compositing | `sola-browser` `on_before_command_line_processing` (`app_id == "sola-design"`) | **Installed** `design` release 2026-10-14. Zoomed in on one animated board (232%, 60 fps), GPU was still 55% (P0, ~105 W): Design's CEF GPU process at 36–44% SM, because with GPU compositing and CPU frames (no shared textures) Chromium reads every frame back whole. `--disable-gpu-compositing` for Design's helper only: the software compositor redraws the damage into the paint buffer. Same view: that process off pmon; Design ~5% SM; CPU Design ~10–16%, renderer ~18%, helper ~3%. Agent screenshots render as before. The browser keeps GPU compositing (its own call). |
| Image atlas 512², not 2048² | patched `iced_wgpu` `image/atlas.rs`, `image/cache.rs` | **On branch `vram-fixes`, private desk.** 1 MiB a window, not 16; an image too large for it gets its own texture. |
| No MSAA where meshes are straight | `sola-terminal`, `sola-projects`, `sola-editor` `.antialiasing(false)` | **On branch `vram-fixes`, private desk.** The target was as large as the largest window the app had had, and stayed. |
| Hidden page frees its texture | `sola-browser` `cef/frame.rs` `free_hidden` | **On branch `vram-fixes`, private desk.** Goes at the next prepare of any page window, so the last one or two wait for Browser's next draw. The parked frame comes back in the first present. |
| Hidden canvas closes when idle | `sola-design` `app/canvases.rs` `evict_idle` | **On branch `vram-fixes`, private desk.** Not the active window's. Its textures go at Design's next draw. |

Install that landed this track (2026-08-25): `shell` `agent` `arcade`
`browser` `install` `kit` `mail` `monitor` `paint` `preview` `settings`
`terminal` `workspaces`. (`agent` was in that bulk install; crate retired
2026-08-28.)

---

## Not fully smoked

| Item | Why it is still open |
|------|----------------------|
| Opaque-region / GLES scanout | Session-spawned kit apps did not re-exec on the 2026-08-25 install. Restart projects, terminal, mail, browser chrome, kit, arcade, preview, then watch **river SM %** at idle. |
| Self-watch on `cargo make install` | Those same PIDs stayed up for hours after the binary was replaced. Shell and CEF engines did restart. Need to know why session children missed the inode swap. |
| First overlay after a **fresh shell** | Parked 2×2 surfaces still have to map once before Super+Space can skip create/map. Later opens are Frame + stack. |
| Projects blink | `BlinkTick` every **530 ms** is still a full-window present (~2 Hz) while a cursor is blinking, of the focused window alone since 2026-11-04. Visible in the 2026-08-25 sample (then `workspaces`, ~11% SM, ~16% CPU). |
| Browser click | `LeftPressed` / `CursorReleased` still present chrome once per click. Marked acceptable. |
| Working ring at desk | Storybook Sidebar / a Working Grok row can still pin GPU if `RedrawRequested` is left chained. |
| Overlay `Resized` hang | If iced never reports ≥64×64 after Frame, Super+Space would not join Composition. Not seen after the iced-`Resized` slice. |

---

## Next / later

**Next (as of 2026-08-25):** River **NVIDIA knobs**, only after clients
stop presenting storms and after opaque-region is actually live on tiled
kit windows.

**sola-design next:** smoke the pacing at the desk (zoom into an
animated board: 60 fps; fit: 1); if the helper's tile compare still
shows, compare only rows CEF's rect actually crosses per tile column.

**Later / park**

- `sola-terminal-perf.log` is always on, has no rotation, and stood at 79 MB on novus
  on 2026-11-04 (`perf.rs`; `SOLA_TERMINAL_PERF=0` turns it off). Cap it or default it off.
- Terminal (and, cheaper still, Projects) **cursor blink** without a process-wide
  present — `request_redraw` only on the cell that blinks, or a cheaper
  tick that does not redraw the whole 5K window.
- Find and fix **session self-watch** so `cargo make install <app>`
  always re-execs user apps, not only managed `sola-*` daemons.
- Confirm **direct scanout** (or at least no ARGB blend) for Cinema
  opaque-region windows — `WAYLAND_DEBUG` / river logs, not NVML alone.
- Opaque **XRGB** swapchains in Cinema (today: ARGB + opaque-region
  hint). Only if region is not enough for scanout.
- Tiles: an opaque region **minus the rounded corners**, so River can skip
  the picture under them again (needs winit to take a region).
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
stat -c '%y' /opt/sola/bin/sola-projects
ps -o lstart,etime,args -C sola-projects
```

For VRAM, by process and by what it holds:

```bash
nvidia-smi pmon -c 1 -s m   # fb MiB per process (the CEF GPU helpers are separate rows)

# Window buffers: the dma-bufs rivulet holds, with their sizes
r=$(pidof rivulet); for f in /proc/$r/fd/*; do
  readlink "$f" | grep -q dmabuf && grep -h '^size:' "/proc/$r/fdinfo/${f##*/}"
done | sort | uniq -c

# Every texture an app makes, on a private headless desk: no build needed
RUST_LOG=info,wgpu_core=trace sola-terminal   # look for create_texture
```

Compare against the baseline (~30–40%) and the 2026-08-25 row above.
If p-state leaves P8 at a quiet desk, something is presenting again —
check the architecture regression column before adding a timer.
