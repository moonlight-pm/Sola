# Active work

## Current

**sola-mail kit port** — `crates/sola-mail` layered kit app (protocol +
worker + ui), parity with apocrypha mail. Design:
`docs/specs/2026-07-27-sola-mail-kit-design.md`. Branch: `sola-mail`.

### Last completed

**sola-kvm → master**: Sola-native software KVM (novus → ember). Layer-shell
edge capture, UDP spray, Mac CGEvent agent, sola MANAGED autostart. lan-mouse
purged both hosts.

### Future / follow-ups

- Permanent /dev/input ACL or udev for sola-kvm (avoid per-boot setfacl)
- Permission fan-out UX when TUI + sola-agent both attached (ask mode)
- Remaining worktrees: `libei-portal` (archive/cleanup)
- sola-mail: polish compose multiline body, storybook N/A

### Resume

```text
# branch sola-mail — crates/sola-mail
cargo make build mail
cargo test -p sola-mail
```
