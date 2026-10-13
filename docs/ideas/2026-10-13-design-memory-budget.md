# sola-design memory — parked mitigations

**Status:** idea (parked 2026-10-13). Do not implement from this file.
Promote into a freeze + plan + `CURRENT.md` **Now** before any code.

**Locked already** ([decision log](../open-questions.md#decision-log),
2026-10-13): the budget **adapts** to what the machine has free (if that
can be done reliably), and over budget Design **refuses** the agent's new
board / canvas with an error that says what is wrong, what is live, and
how to free room.

**Shipped (host, not Sola):** zram swap + earlyoom in
`/etc/nixos/configuration.nix` (see below).

---

## Incident (2026-10-13)

- About ten agents worked in Design at once, each on its own canvas. RAM
  (125G) filled; River could not map new windows, the rest ran. Joshua
  got in over ssh from another machine; memory stayed near 100%.
- An agent was then asked to close Design canvases. That tipped the box
  into a full freeze — no ssh — and a hard reset.
- No swap, and nothing watching: the kernel never OOM-killed (no OOM line
  in the whole 15-day boot). It evicted file pages — every process's code,
  sshd's too — and re-read them: a livelock. journald logged *Under memory
  pressure* from 16:38 to the reset at 16:47. systemd-oomd was running but
  monitored no cgroup (`oomctl`).
- Design's log of the run was overwritten when it started again, so how
  closing made it worse is a guess: there is no `canvas.close`, so the
  agent had to go around it (`project.remove`, file edits, `open`), and
  every agent call wakes boards to full rate — a waking board repaints
  whole, new full-size buffers in every process on the path.

## Measured cost (after the reboot, ~30 boards live)

| Process | RSS | Note |
|---------|-----|------|
| CEF GPU / compositor (child of the unsandboxed zygote) | 9.7G | ~5.2G shm, ~2.0G anon — under **CPU** OSR |
| Engine helper (`--engine --profile=sola-design`) | 2.7G | ~2.0G anon |
| Renderers | ~135M each | one per board (each board is its own browser) |
| Chrome (`sola-design`) | 0.5–0.7G | + 1.6G VRAM |

About **17G for ~30 boards — roughly 0.5G per board**, ~10× its frame
(1440 × ~2000 at the fixed 2× raster = 40–57MB). Taller boards cost more
(`MAX_FRAME_BYTES` allows 160MB per frame). Ten agents × ten boards is
~50G at rest, before loads, captures, and detail renders.

Nothing caps it: **Asleep stops painting but frees nothing**, an
off-screen canvas closes only after 10 idle minutes (an agent that keeps
calling never idles), and `open` / `canvas.new` always succeed.

## Host safety net (shipped 2026-10-13)

```nix
zramSwap.enable = true;              # 62.8G zstd
services.earlyoom = {
  enable = true;
  freeMemThreshold = 5;              # TERM ≤5% avail + ≤10% swap free; KILL at half
  extraArgs = [ "--avoid" "^(sola|sola-bus|sola-call|sola-river|river|sola-shell|sola-session|Xwayland|sshd|sshd-session|tmux: server|systemd|systemd-journal|systemd-logind|dbus-daemon)$" ];
};
```

Chromium sets `oom_score_adj` 300 on renderers and 200 on its GPU
process, so earlyoom takes CEF boards and tabs first. Never fired yet.
systemd-oomd user-slice monitoring was not used: River runs in the TTY's
`session-1.scope`, so oomd could pick the whole session.

## Parked mitigations (rough order)

1. **Budget with admission control.** Track live board bytes (Design knows
   each board's frame size) against a budget that follows free memory
   (`MemAvailable`, `/proc/pressure/memory`). Over budget, `open` /
   `canvas.new` / a board that would grow fail with a plain reason: the
   memory, the live canvases and their boards, and the way out
   (`canvas.close`).
2. **`canvas.close`** on the call plane (solactl + MCP). There is none
   today; an agent told to close canvases has no clean move.
3. **Recover a killed renderer.** sola-browser's CEF code has no
   render-process-terminated handling; with earlyoom a dead board now
   becomes likely. Show it as a board error, reopen on the next look or
   call.
4. **A discarded tier** below Asleep: close the CEF browser, keep the last
   picture on the plane (perhaps at 1×), reopen on a look or a call, least
   recently used first. **Fork — ask first:** the decision says not to
   silently drop other boards; does a discard that keeps the picture count?
5. **Cheaper boards.** Raster at 1× when not in view (`RASTER_SCALE` is a
   fixed 2.0 — 4× the bytes). Find where the ~10 copies live: renderer
   tiles and viz output in shm (GPU process), engine `CefFrame`, frame
   IPC, chrome pending, texture. Measure whether `was_hidden` releases
   tile memory (Asleep vs Live per board).
6. **Share renderer processes** (`--renderer-process-limit`): hosts stay
   distinct, so per-host page zoom holds. Saves ~100M a board; measure.
7. **Fence Design in its cgroup** (`MemoryHigh` / `MemoryMax` on its
   `sola-app-*` scope) so a runaway restarts Design, not the box. Catch:
   the CEF engine moves itself into its own
   `app-org.chromium.Chromium-<pid>.scope` (its GPU process stays in
   Design's scope); a limit must cover both or stop the move.
8. **See it.** `status` reports memory (board bytes + process RSS); keep
   the previous run's log (`app-sola-design.log` is overwritten on start —
   likely true of every app).
