# Active work

## Current

**none** (desk-check clipboard + cold-enter)

### Last completed (this session)

- **clipboard Mac→Linux recovery** — root cause: hung `wl-copy` / arboard
  X11 blocked the clip worker so ember never got Ack, dropped TCP peer
  (`no TCP peer` on Leave). Fixes: skip arboard on Wayland; thread budget
  around clip I/O; pending push + soft Ack timeout; leave-spray reconnect;
  clear-before-copy + write epoch. Deployed to `/opt/sola/bin/sola-kvm` +
  ember agent.
- **ember cold-enter path** (Input Leap parity): IOPM + display wake +
  session asserts + NSProcessInfo latency activity on Enter/Leave.

### Earlier completed

- **sola-kvm clipboard v1** — TCP CLIP1 same port as UDP; worker threads;
  Enter → Mac, Leave → Linux; FNV hash cache. Spec:
  `docs/specs/2026-07-30-sola-kvm-clipboard-design.md`
- sola-kvm split-seat / boot resilience / leave spray / hard-warp policy
- novus udev `uaccess` for `/dev/input` (system-119)
- ember stuck-modifier clear on Enter/Leave

### Documented for later

**Clipboard (done v1):** see above. Follow-ups: native pasteboard APIs,
images, larger caps.

**Claude lag second opinion:**
`docs/notes/2026-07-28-sola-kvm-lag-claude-second-opinion.md`

### Future / follow-ups

- Desk-check cold enter after ≥1–2 min on Linux only (hitch gone?)
- Edge hysteresis if thrash returns
- Permission fan-out UX when TUI + sola-agent both attached (ask mode)

### Resume

```text
# install ember sola-kvm-mac, idle on Linux 1–2 min, cross over — feel lag?
# lag: docs/notes/2026-07-28-sola-kvm-lag-claude-second-opinion.md
# clipboard: docs/specs/2026-07-30-sola-kvm-clipboard-design.md
```
