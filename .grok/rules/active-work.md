# Active work

## Current

**screenshot-tool** (branch `screenshot-tool`): sola-preview + selection capture.

### Built (needs user install + smoke)

- Bus: `CaptureTarget::Region`, `Topic::OpenImage`
- sola-river: region capture
- sola-shell: Super+Shift+3 full / **4 selection** / **5 window**; marquee
  overlay; toast + open/raise sola-preview on shell-initiated captures
- **sola-preview**: kit image viewer with session history sidebar

### Last completed

**sola-kvm → master**: Sola-native software KVM (novus → ember).

### Future / follow-ups

- Permanent /dev/input ACL or udev for sola-kvm (avoid per-boot setfacl)
- Permission fan-out UX when TUI + sola-agent both attached (ask mode)
- Remaining worktrees: `libei-portal` (archive/cleanup)
- sola-preview: zoom, clipboard copy, solactl `--region`

### Resume

```text
# worktree: /home/joshua/orca/workspaces/Sola/screenshot-tool (branch screenshot-tool)
# after approval: merge to master + cleanup worktree
cargo make build   # already green
# user: cargo make install shell river preview bus   # ask first
```

