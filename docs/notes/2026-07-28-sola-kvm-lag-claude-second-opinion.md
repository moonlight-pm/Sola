# sola-kvm intermittent lag — Claude second opinion (capture)

**Date:** 2026-07-28  
**Source:** Claude Code CLI (`fable` / `xhigh`) via `/ask-claude`  
**Scratch:** `/tmp/grok-1000/ask-claude.G3IRkN`  
**Status:** Reference for later work — **not implemented wholesale**  
**Context session:** `kvm-performance` thrash + metrics + multi-pass inject fixes

This document captures Claude’s second opinion on multi-second mouse lag in the
novus → ember KVM path, plus Grok’s evaluation notes, so we do not lose the
thread when returning to performance work.

---

## Symptom (agreed)

- Remote mouse on ember is usually fine, then **lags for a few seconds**, then
  smooths / “catches up.”
- Happens mid-session while thrashing, not only first crossover after idle.
- Metrics show **novus send path healthy**; pain is concentrated on **ember**.

---

## What we already proved with metrics (pre-Claude)

| Side | Finding |
|------|---------|
| **novus** | ~110–125 Hz when thrashing; `send_avg_ms` ~0.005; almost never slow |
| **ember (early)** | CGWarp every motion → `inject_ms` 8–14 ms spikes → socket `pre=16–17` → `motion_hz` 120→~40 |
| **ember (later)** | Inject can be cheap (~0.1 ms) but **paint rate still collapses**; `gap_max_ms` 145–1700 |
| **macOS bugs fixed** | `set_read_timeout(0)` illegal → drain never ran + WARN spam; various sleep/spin/coalesce experiments |

---

## Claude’s three headline points

### 1. Hard warps reintroduced at high rate

- Module comment / `inject.rs` intent: hard warp only on enter / click resync.
- `agent.rs` (at time of review) hard-warped **every other paint** while thrashing
  (~60 CGWarps/s).
- With measured hard-warp spikes of 8–37 ms, that alone can collapse the paint
  loop on a bad stretch.

**Action later:** A/B thrash with hard warp **only** on Enter + click resync
(no periodic / alternate paint warps). Re-measure `inject_max_ms` and
`motion_hz`.

### 2. Slow synchronous CGEventTap on ember (external, high value)

- Every `CGEventPost(kCGHIDEventTap, …)` walks registered filtering taps.
- A slow mouse-mask client can block delivery until
  `kCGEventTapDisabledByTimeout` (~1 s), then recover — matches “few seconds
  then smooth.”
- Suspects: Mos, LinearMouse, Scroll Reverser, BetterTouchTool, some WM/remote
  tools; key-only taps less relevant for motion thrash.
- Also: **“Shake mouse pointer to locate”** (System Settings → Accessibility →
  Display) can fire under thrash and load WindowServer.

**Diagnostic (highest information / low cost):**

```text
CGGetEventTapList()  →  pid, event mask, options, avgUsecLatency
```

Log while remote and during a lag episode; the slow tap lights up.

### 3. Metrics can mislead (we partly already felt this)

| Issue | Detail |
|-------|--------|
| **Two streams, one message** | Recv and inject both log `"kvm-metrics ember window"` with no `role=` |
| **`gap_max` pollution** | Counts human micro-pauses the same as stalls |
| **Recv `motion_hz`** | Post-coalesce; bursty arrival looks like low Hz even when all packets arrived |
| **No wire timestamps** | Cannot split network delay vs ember scheduling vs WindowServer |
| **Seq gaps at debug only** | Loss invisible at default `info` |

**Instrumentation backlog (Claude experiment 1):**

1. Tag metrics: `role=recv` / `role=inject`.
2. Only count `gap_max` when motion was **pending** during the gap.
3. Per-window **seq-gap** count at info.
4. Optional: novus monotonic µs on `Motion` → ember p50/p95/max `recv−send` **jitter**.
5. Time **`CGWarpMouseCursorPosition` vs `CGEventPost` separately** per call; log max/window.
6. Log **`CGGetEventTapList`** every remote window (or on LAG spike).

---

## Other ranked candidates (Claude)

| Rank | Candidate | Notes |
|------|-----------|--------|
| A | **Wi‑Fi / AWDL** on ember | Ethernet coalescing is µs-scale; Wi‑Fi/AWDL can be 100–500 ms+. Test: `ping -i 0.02`, `sudo ifconfig awdl0 down` |
| B | **`thread::yield_now` depressed yield** | Darwin `swtch_pri` — self-demotes inject (same family as poll(0) demotion) |
| C | **`setpriority` + pthread QoS mix** | Apple discourages; prefer **QoS only** on macOS |
| D | **NSActivity / IOPM never implemented** | `begin_latency_critical_activity` is a no-op stub; Synergy used `IOPMAssertionDeclareUserActivity` |
| E | **Single event-driven thread** | Post on receipt (Barrier/Input Leap style); no paint timer |
| F | **If two threads stay** | Event-driven wait on `gen` (`os_sync_wait_on_address` / ulock), not sleep/spin |
| G | **DriverKit virtual HID** | Nuclear; only if taps confirmed unavoidable |

---

## Claude’s ordered experiment plan

1. **Instrument to bisect** (before more behavior churn) — see table above.  
2. **Remove alternating / periodic hard warp** — enter + click only; consider setting
   `kCGMouseEventDeltaX/Y` on moved events.  
3. **Collapse to one ordered event-driven path** (or event-driven wait on gen).  
4. **Wi‑Fi falsification** if ember is wireless.  
5. **Real latency/power assertions**; drop Nice on macOS in favor of QoS.  
6. **Virtual HID** only if experiment 1 forces it.

---

## Correctness risks Claude flagged (not just lag)

### Motion vs discrete race (real)

- Motion: atomics  
- Buttons/keys: mpsc  
- Recv preserves order in `coalesce_recv_batch`, then **splits streams** and
  destroys order.
- Late inject can hard-warp to a *newer* position **before** a click that was
  earlier on the wire → click lands wrong (drag-select).

**Fix direction:** one ordered queue; coalesce only adjacent motion runs
(already what `coalesce_recv_batch` does if we stop splitting).

### Lost Leave over UDP (real)

Leave does: re-associate pointer, stuck-key release, multi-click reset.  
Fire-and-forget Leave → drop means:

- Local Mac pointer hardware stays dead (association off)
- Stuck modifiers until next session

**Fix direction:** resend Leave ~3× (idempotent), or ACK Enter/Leave; Enter
should defensively clear stuck keys / `MOD_FLAGS`.

### CG thread safety

- Posting CGEvents / CGWarp / CGAssociate from a non-main thread is **fine**
  (Mach IPC to WindowServer).
- Hopping to main/CFRunLoop is **not** required for this issue class.
- Soft CGEvent moves: Claude claims they **do** move the system arrow while
  dissociated (stale-arrow fear overstated) — **verify empirically** rather
  than treating as gospel.

### Minor

- `configure_source_throttled` re-applies a once-at-create property; counter
  naming is misleading.
- Inject-side metrics fabricate `pre_coalesce` and pollute `pkts_in`.

---

## Answers Claude gave to our numbered questions (condensed)

| Q | Claude |
|---|--------|
| Main-thread inject? | **No** — non-main CG post is fine |
| Display-link paint? | **No** — event-driven post-on-receipt is right |
| Wrong inject API? | `kCGHIDEventTap` is correct; problem is taps *between* post and delivery |
| Relative wire motion? | **Keep absolute** — self-healing on loss; relative adds drift |
| Soft move stale arrow? | Soft move should move arrow; don’t need periodic hard warps (verify) |
| App Nap / assertions? | Listed as tried but **not implemented** in code — do IOPM + real NSActivity |
| Network? | Ethernet: no. **Wi‑Fi/AWDL: yes, seriously** |
| Compositor stalls? | Plausible; measure warp vs post + tap latency |
| Reference stacks? | Same CGEvent approach; they **don’t** run a paint timer or periodic warps |

---

## Grok evaluation (session)

**Agree strongly:** event-tap hypothesis; metrics tagging; hard-warp every-other-paint
self-contradiction; Leave reliability; click/motion race.

**Pushback:** “60 warps/s × 37 ms = 2 s/s” is worst-case, not average — still a
spike driver. Soft-move-always-moves-arrow needs a desk check. Single-thread
post-on-receipt needs latest-wins if CGWarp can still spike 10–30 ms.

**Suggested order when we resume lag work:**

1. Metrics hygiene + warp/post split timing + event-tap dump  
2. Hard warp only enter/click (A/B thrash)  
3. Wi‑Fi/AWDL check  
4. Real power/latency assertions  
5. Ordered single-queue or event-driven wait on gen  
6. Leave resend / Enter defensive cleanup  

---

## Code touchpoints (as of 2026-07-28 session)

| Area | Path |
|------|------|
| Ember agent | `apps/sola-kvm-mac/src/agent.rs` |
| Ember inject | `apps/sola-kvm-mac/src/inject.rs` |
| Ember metrics | `apps/sola-kvm-mac/src/metrics.rs` |
| Ember priority stubs | `apps/sola-kvm-mac/src/priority.rs` |
| Novus run/pacer | `crates/sola-kvm/src/run.rs` |
| Novus input | `crates/sola-kvm/src/input.rs` |
| Novus metrics | `crates/sola-kvm/src/metrics.rs` |
| Protocol | `crates/sola-kvm/src/protocol.rs` + mac mirror |

---

## Split-seat incident (ops + code)

**User symptom:** “novus has the mouse, ember has the keyboard.”

**Mechanism:**

1. Mouse re-plug → new `/dev/input/event*` **without** user ACL.  
2. sola-kvm still had old pointer fds (or entered when pointer was OK).  
3. Re-enter / grab: pointer ioctls fail `ENODEV`, keyboard grab succeeds.  
4. Logs still said `grab ON` → keys exclusive to kvm → Mac; mouse is the new
   local device.

**Code hardening (same era as this note):**

- Refuse edge enter without a live pointer.  
- `prune_dead` + periodic `rescan_new` for hotplug.  
- **Force Leave** if pointer lost while remote.  
- **Abort EVIOCGRAB** unless at least one pointer grab succeeds; release any
  keyboard grabs on abort.

**ACL one-shot (correct form):**

```bash
sudo setfacl -m "u:$(id -un):rw" /dev/input/event[0-9]*
# or: sudo /opt/sola/bin/sola-kvm-grant-input-acl
killall sola-kvm
```

**Permanent:** `nix/module.nix` udev `TAG+=uaccess` + `crates/sola-kvm/udev/99-sola-kvm-input.rules`.

---

## Implemented later (2026-07-30 — “do anyway” correctness)

While lag was already acceptable (no thrash), three correctness items from
this note shipped without the full lag experiment plan:

1. **Leave ×3 spray** on every leave path (`crates/sola-kvm/src/run.rs`
   `LEAVE_SPRAY`) — not only startup.
2. **Hard warp enter/click only** — removed alternate paint CGWarp
   (`apps/sola-kvm-mac/src/agent.rs`).
3. **Wire-ordered click cursor** — discrete queue is `StampedDiscrete`
   with stream-cursor stamp so later motion cannot steal click position.

Lag instrumentation / event-tap / Wi‑Fi / single-thread rewrite still parked.

## Out of scope for this note

- Full Claude lag experiment implementation (resume later).
