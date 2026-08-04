# Active work

## Current

**none**

### Last completed

**opening-app → master**: Menubar “Opening {label}…” toast while launcher
starts an app; clears on new matching window / fail / exit / 20s timeout.
Toast centered in the menubar (overlay), not the right stats cluster.

**sola-kvm-performance → master**: Key auto-repeat (Linux EV_KEY=2 → wire
`pressed=2` + Mac `kCGKeyboardEventAutorepeat`) and remote lag fixes (poll
wait, motion/scroll coalesce, quieter Mac high-rate logging). Both hosts
installed and smoked.

### Future / follow-ups

- Permanent /dev/input ACL or udev for sola-kvm (avoid per-boot setfacl)
- Permission fan-out UX when TUI + sola-agent both attached (ask mode)
- Remaining worktrees: `libei-portal` (archive/cleanup)
- Optional: further Mac warp path cost (associate/disassociate every motion)

### Resume

```text
# no active feature worktree
```
