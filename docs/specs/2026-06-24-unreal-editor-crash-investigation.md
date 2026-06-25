# UnrealEditor-under-Sola Crash — Investigation (DEFERRED)

**Date:** 2026-06-24
**Status:** OPEN — deferred. The floating-windows feature (Phase A) shipped, but
it does **not** fix this crash. Resume here when picking the bug back up.

---

## 1. Symptom

UnrealEditor 5.8 (`app_id = UnrealEditor`, Vulkan + SDL3, launched via
`~/.local/bin/unreal-editor` → `nix develop -c run-unreal`) dies **deterministically
~1s after loading the "One" project** (project picker comes up fine; the crash is
on project *load*, not on bringup). Clean exit (SDL quits the engine; no UE crash
dump). The compositor drops the Wayland connection.

## 2. Crash signature

UnrealEditor log (`~/Workspace/Unreal/Projects/One/Saved/Logs/One.log`) tail at
the moment of death:

```
LogVulkanRHI: AcquireNextImage() failed due to the outdated swapchain, not even attempting to present.
LogSDL3: Wayland display connection closed by server (fatal)
LogLinuxWindow: Warning: Received SDL_EVENT_QUIT, requesting engine exit.
LogCore: FUnixPlatformMisc::RequestExit(bForce=true, ReturnCode=0, ...)
```

River (the real wlroots Wayland **server**) log (`/opt/sola/log/river.log`),
correlated:

```
error(wm): timeout occurred, some imperfect frames may be shown   (×many; 1269 cumulative)
info(wlroots): [wayland] error in client communication (pid <UE pid>)
```

So: **River closes UE's socket** ("error in client communication"), and on the UE
side that surfaces as an *outdated swapchain* immediately followed by *Wayland
display connection closed by server (fatal)*.

## 3. Topology (important — easy to get wrong)

- `river` (PID varies; `/run/current-system/sw/bin/river -log-level info -c :`) is
  the **Wayland server** UE connects to. Its log is `/opt/sola/log/river.log`.
- `sola-river` is **only the WM bridge** — a *client* of river's
  `river-window-management-v1` protocol. It proposes dimensions / sets positions;
  it is NOT in UE's data path for buffers/frames.
- UE talks to `river`, not `sola-river`.

## 4. What has been ruled OUT

- **"Sola force-resizes UE → swapchain invalidated → crash"** (the original design
  premise in `2026-06-24-floating-windows-design.md` §1, "Root cause confirmed
  in-repo"). **DISPROVEN.** With `Zones: UnrealEditor: Float`, sola sends only
  `propose_dimensions(0,0)` (no resize) — yet UE crashes identically at the same
  spot. The early resize is therefore **not** the cause (or not the only cause).
- **Window position / zoning.** Float came up unzoned ("random place") and still
  crashed. Not position-dependent.

## 5. Separate bug found & FIXED (do not re-chase)

While reproducing, both launchers failed *before Wayland* with
`Unreal Engine root not found :(` (1s exit code 1). Root cause: sola was started
from a TTY shell sitting in `~/Workspace` where **direnv** is active, so sola
exported `DIRENV_DIR=-/home/joshua/Workspace` into every child (the launcher and
every terminal). The Unreal flake's `shellHook` (`~/Workspace/Unreal/flake.nix:49`)
only sources `.envrc-user` (which sets `UE_PATH`) **when `DIRENV_DIR` is unset**, so
the stale inherited value made it skip → empty `UE_PATH`.

**Fix (applied):** both `~/.local/bin/unreal-editor` and
`~/.local/bin/unreal-editor-debug` now `unset DIRENV_DIR DIRENV_FILE DIRENV_DIFF
DIRENV_WATCHES` before `nix develop`. Verified: `UE_PATH` and
`UE_SDL_VIDEODRIVER=wayland` resolve again. This is **unrelated** to the crash —
it only blocked relaunching.

## 6. Current best understanding (unconfirmed)

River drops UE's connection during the heavy, partially-blocking **project load**.
Two live hypotheses, not yet distinguished:

1. **Protocol error.** UE/SDL3 issues a request River considers illegal during the
   load/reconfigure, and River posts a `wl_display.error(...)` then closes. The
   "outdated swapchain" would be a *consequence* (surface reconfigured/destroyed),
   not the trigger.
2. **Socket / event-buffer overflow.** UE blocks its Wayland thread for ~seconds
   during project load and stops reading; River's per-client buffer overflows and
   it drops the client. The 1269 cumulative `timeout occurred` WM render-sequence
   timeouts show `sola-river` is *also* slow to complete render sequences, adding
   compositor-side pressure.

The **`AcquireNextImage() failed due to the outdated swapchain`** line landing
*immediately before* the disconnect is the key clue and is consistent with either
(a configure/resize arriving from River, or the surface being torn on disconnect).

## 7. Mitigation tried — insufficient

- **`sola-river` churn fix** — commit `1214e70` ("only send
  propose_dimensions/set_position on change"). Cuts the re-propose/re-position
  storm that produced the WM render-sequence timeouts. **This is installed and
  live** — the crash reproduced *with* it. So reducing the WM churn alone does
  **not** prevent the disconnect. (The `timeout occurred` count in `river.log` is
  cumulative across old + new runs, so it is not a clean before/after measure of
  how much the churn fix helped.)

## 8. Next steps to resume

The churn fix (§7) is already live and didn't stop the crash, so the next move is
to *observe the disconnect directly*, not to tune more levers blind.

1. **Capture a WAYLAND_DEBUG trace** of the crash (primary next step): run
   `~/.local/bin/unreal-editor-debug` from a sola terminal, load "One", let it
   crash → `/opt/sola/log/ue-wayland-<ts>.log`. Inspect the tail:
   - A `wl_display@1.error(<object>, <code>, "<message>")` near the end ⇒
     **hypothesis 1** (protocol error) — the message names the exact violated rule.
   - A stream of inbound events that simply stops, no error line ⇒ **hypothesis 2**
     (overflow / client too slow).
2. **Correlate** the trace's last surface/configure events with the
   `AcquireNextImage outdated swapchain` line to see what reconfigured the surface.
3. Compare against a known-good Vulkan client (e.g. `vkcube --wsi wayland`) under
   the same zone to see if any GPU client survives a forced reconfigure on this
   River build.

## 9. Tools staged

- `~/.local/bin/unreal-editor-debug` — `WAYLAND_DEBUG=1` + tee to
  `/opt/sola/log/ue-wayland-<ts>.log`; mirrors the real launcher's `cd` +
  `nix develop -c run-unreal`, with direnv state cleared (see §5).

## 10. Related artifacts

- `docs/specs/2026-06-24-floating-windows-design.md` — the feature this bug
  motivated (its §1 root-cause claim is **superseded** by §4 above).
- `docs/specs/2026-06-24-floating-windows-phase-a-plan.md` — Phase A plan (shipped).
- Phase A commits: `569322e`, `ddd8641`, `e95890a`, `dd6522e`, `f3fd5fe`,
  `cecbbb9`, `1214e70`.
