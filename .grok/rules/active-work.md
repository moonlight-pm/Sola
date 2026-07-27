# Active work

## Current

**sola-kvm** on branch `sola-kvm` — desk path live; lan-mouse purged both hosts.

### Autostart

| Host | Mechanism |
|------|-----------|
| novus | `sola` MANAGED includes `sola-kvm` → `server --input evdev` |
| ember | LaunchAgent `com.sola.kvm-mac` KeepAlive |

### After code install

Restart Sola from TTY so the new `/opt/sola/bin/sola` picks up MANAGED + launches sola-kvm.

### Installed

- `/opt/sola/bin/sola` (with sola-kvm managed)
- `/opt/sola/bin/sola-kvm`
- ember: `/opt/sola/bin/sola-kvm-mac` + LaunchAgent

## Last completed (prior, master)

app-icon-raster, session-id-routing, … (see git log)
