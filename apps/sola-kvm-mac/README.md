# sola-kvm-mac

macOS client for **sola-kvm** (ember).

**Status:** stub — Phase B of `docs/specs/2026-07-27-sola-kvm-design.md`.

## Role

- Bind UDP (`peer.port`, default `4242`)
- Decode sola-kvm packets (`crates/sola-kvm` wire format)
- Inject via Accessibility / CGEvent
- Run in the **GUI login session** (LaunchAgent), not SSH

## Phase B checklist

1. UDP listen + warp on `Enter` / abs `Motion`
2. Button / key / scroll inject + Linux evdev → CGKeyCode table
3. LaunchAgent (`open` app bundle or gui-domain binary)
4. Manual feed from novus: `sola-kvm send-test --to 10.0.0.21:4242`

Until this lands, use on novus:

```bash
sola-kvm listen --bind 0.0.0.0:4242
# other terminal:
sola-kvm send-test --to 127.0.0.1:4242
```

## Permissions

- Accessibility (and Post Event if required) once in System Settings
- Confirm inject with a console-session process; SSH context is unreliable for TCC
