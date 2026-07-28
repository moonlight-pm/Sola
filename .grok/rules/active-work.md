# Active work

## Current

**sola-kvm-performance** (branch `sola-kvm-performance`) — key auto-repeat +
input latency.

### Done in this branch

- Forward Linux `EV_KEY` value `2` as wire `Key.pressed=2` (was dropped)
- Mac inject sets `kCGKeyboardEventAutorepeat` + unicode on repeats
- Latency: idle `poll` wait (no fixed 2ms sleep after busy ticks), coalesce
  motion/scroll runs, quiet Mac high-rate `info` logging

### Smoke after install

1. Install novus: `cargo make install sola-kvm` (needs explicit permission)
2. Ember: rebuild/install sola-kvm-mac via `apps/sola-kvm-mac/scripts/install.sh`
3. Hold a key in a text field on Mac → should auto-repeat
4. Rapid mouse move + typing while remote → less lag / fewer stalls

### Last completed (prior)

**sola-kvm keyboard/click/scroll** → master: open keyboard nodes, Mac CGEvent
inject (suppression + unicode), scroll invert/speed, stable signed .app

### Future / follow-ups

- Permanent /dev/input ACL or udev for sola-kvm (avoid per-boot setfacl)
- Permission fan-out UX when TUI + sola-agent both attached (ask mode)
- Remaining worktrees: `libei-portal` (archive/cleanup)
- Optional: further Mac warp path cost (associate/disassociate every motion)

### Resume

```text
# this worktree / branch
cargo make build sola-kvm
cargo test -p sola-kvm
# Mac agent (separate tree): apps/sola-kvm-mac
```
