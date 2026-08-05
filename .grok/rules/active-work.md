# Active work

## Current

**none**

### Last completed

**kvm-performance → master**: sola-kvm clipboard (CLIP1 TCP, Enter→Mac /
Leave→Linux, hang-safe wl-copy), cold-enter IOPM/display wake, seat
resilience (Leave spray, stuck modifiers, udev uaccess), metrics/priority.

**screenshot-tool → master**: sola-preview + selection capture (macOS
Super+Shift+3/4/5), OpenImage handoff without stealing focus, shell freeze
fixes (bus mutex deadlock + singleton bus poller).

**opening-app → master**: Menubar “Opening {label}…” toast while launcher
starts an app; clears on new matching window / fail / exit / 20s timeout.
Toast centered in the menubar (overlay), not the right stats cluster.

**sola-kvm-performance → master**: Key auto-repeat (Linux EV_KEY=2 → wire
`pressed=2` + Mac `kCGKeyboardEventAutorepeat`) and remote lag fixes (poll
wait, motion/scroll coalesce, quieter Mac high-rate logging).

### Documented for later

**Clipboard follow-ups:** native pasteboard APIs, images, larger caps.
Spec: `docs/specs/2026-07-30-sola-kvm-clipboard-design.md`

**Claude lag second opinion:**
`docs/notes/2026-07-28-sola-kvm-lag-claude-second-opinion.md`

### Future / follow-ups

- Desk-check cold enter after ≥1–2 min on Linux only
- Edge hysteresis if thrash returns
- Permission fan-out UX when TUI + sola-agent both attached (ask mode)
- Remaining worktrees: `libei-portal` (archive/cleanup)
- sola-preview: zoom, clipboard copy, solactl `--region`

### Resume

```text
# lag: docs/notes/2026-07-28-sola-kvm-lag-claude-second-opinion.md
# clipboard: docs/specs/2026-07-30-sola-kvm-clipboard-design.md
```
