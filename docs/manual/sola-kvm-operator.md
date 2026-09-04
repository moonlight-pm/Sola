# sola-kvm operator guide (novus server)

Software KVM: physical mouse/keyboard on **novus** control a **peer**
over UDP (Linux `sola-kvm listen` or macOS `sola-kvm-mac`). Design:
`docs/specs/2026-07-27-sola-kvm-design.md`.

## Status vs lan-mouse

**lan-mouse is fully removed** from the desk path. Daily KVM is sola-kvm only.

| Host | Autostart |
|------|-----------|
| **novus** (server) | Managed by **`sola`** (`MANAGED` includes `sola-kvm` → `server --input evdev`) after River is up |
| **Linux peer** (client) | `sola-kvm listen` (Wayland virtual pointer + keyboard). On Oath: `svc:sola-kvm` as `home`. |
| **ember** (macOS client, optional) | LaunchAgent `com.sola.kvm-mac` → `/opt/sola/bin/sola-kvm-mac listen` |

Do not reinstall Lan Mouse.app or the old `lan-mouse` user unit.

## Requires sola-river with layer-shell

Edge capture / exclusive keyboard focus needs **`sola-river`** bound to
`river_layer_shell_v1` (and Meta chord suppress during exclusive focus).
Without that, River closes layer surfaces immediately and shell chords
still eat Meta keys while remote.

- Infra notes: [`sola-river-layer-shell.md`](./sola-river-layer-shell.md)
- Log marker after restart: `bound river_layer_shell_v1 (layer-shell clients enabled)`

## Config

```bash
# write defaults (~/.config/sola-kvm/config.toml)
sola-kvm init
# or: cargo run -p sola-kvm -- init

sola-kvm show
```

Key fields:

| Section | Fields |
|---------|--------|
| `[peer]` | `host` (client IP), `port` (default `4242`) |
| `[layout]` | `side` (`right`/`left`/`top`/`bottom`), `align`, `mac_width`, `mac_height` (peer output size; names are historical) |
| `[motion]` | `scale` — multiplies relative deltas while remote |
| `[primary]` | novus logical size (default `5120×2160`); Phase C does not yet pull bus `OutputGeometry` |
| `[clipboard]` | `enable` (default true), `max_bytes` (default 8 MiB). Sync on **Enter** (novus → peer) and **Leave** (peer → novus). Text and `image/png` (screenshots / Preview Copy). TCP on the **same port** as UDP. |

Bottoms-aligned Mac to the right of novus (desk default):

```toml
[layout]
side = "right"
align = "bottom"
mac_width = 2560
mac_height = 2880

[motion]
scale = 1.25   # tune until Mac feel matches local

[peer]
host = "10.0.0.21"
port = 4242
```

## Run the server (novus)

Logs: `/opt/sola/log/sola.log` (shared) and stderr; also check for `sola-kvm` lines.

### Autostart (managed by sola)

1. Install binaries: `/opt/sola/bin/sola` (with MANAGED including sola-kvm) and `/opt/sola/bin/sola-kvm`
2. Config: `~/.config/sola-kvm/config.toml` (`peer.host` = client IP)
3. **Input device access** (required for `evdev`; re-plug creates new nodes):

   **Permanent (preferred — NixOS / `services.sola.enable`):** the sola NixOS
   module installs a udev rule (`TAG+="uaccess"` + `GROUP=input`) so logind
   re-grants the seat user on every plug. Rebuild/switch the host config that
   imports `nix/module.nix`. Rule source: `crates/sola-kvm/udev/99-sola-kvm-input.rules`.

   **One-shot ACL** (until udev is in place) — always quote `-m`, never bare
   `$USER` if it might be empty (`u::rw-` → *Invalid argument near character 3*):

   ```bash
   # helper (re-execs under sudo, expands username safely)
   sudo crates/sola-kvm/scripts/grant-input-acl.sh
   # equivalent one-liner:
   sudo setfacl -m "u:$(id -un):rw" /dev/input/event[0-9]*
   killall sola-kvm   # sola relaunches; re-opens devices
   ```

   If logs show `pointers=0 keyboards=1`, the mouse node is not readable and
   remote enter is refused (prevents “novus mouse / Mac keyboard” split).

4. Start Sola from the TTY as usual — `sola-kvm` is launched after the Wayland socket is ready

Manual override (debug):

```bash
/opt/sola/bin/sola-kvm server --input evdev
# or: cargo run -p sola-kvm -- server --input evdev
```

### Feed backend (no device perms)

Line protocol on stdin. Useful for development and pairing with Phase B
without HID grab.

```bash
# terminal A — Linux client inject (or `--dump` to log packets)
sola-kvm listen --bind 0.0.0.0:4242

# terminal B — drive server locally
printf '%s\n' \
  'abs 5119 2000' \
  'rel 3 0' \
  'rel 50 10' \
  'btn 0 1' \
  'btn 0 0' \
  'key 30 1' \
  'key 30 0' \
  'leave' \
  | sola-kvm server --input feed
```

Commands: `rel dx dy`, `abs x y`, `btn button 0|1`, `key keycode 0|1`,
`scroll dx dy`, `leave`, `#` comments.

### Demo backend (smoke)

```bash
# peer must accept UDP (listen or Mac agent)
sola-kvm server --input demo -- # uses config peer
# or override config:
# sola-kvm -c /path/to.toml server --input demo
```

Scripted: seed near right edge → enter → motion/button/key/scroll → leave → idle.

### Evdev backend (grab spike)

Reads `/dev/input/event*`, tracks an estimated primary cursor from relative
motion, enters remote when motion would leave the shared edge into the virtual
Mac rect, and takes **`EVIOCGRAB`** on all open nodes while remote.

```bash
# needs read+write on event nodes (input group or seat ACL)
sola-kvm server --input evdev
```

Limitations (honest):

- Local absolute position is **estimated** (starts at primary center); drift
  vs the real compositor cursor is expected. Precise edge hit needs a
  layer-shell barrier client (planned; requires `river_layer_shell_v1` in
  `sola-river` — present on `libei-portal`, not yet on this branch).
- Wayland pointer **warp** on leave is logged only (no compositor warp API yet).
- Meta chord suppress (so Cmd reaches Mac) currently depends on layer exclusive
  focus + `sola-river` chord disable. Evdev grab bypasses the compositor for
  events but does **not** by itself disable River xkb shell bindings if any
  still fire — prefer testing feed/demo against the Mac agent first.

## Clipboard

TCP on the **same port** as UDP (different protocol). Sync only when the
pointer **enters** (novus → peer) or **leaves** (peer → novus).

| Payload | Linux ↔ Linux | Mac client |
|---------|----------------|------------|
| UTF-8 text | yes | yes |
| `image/png` | yes (screenshots, Preview Copy) | rejected (pasteboard stays) |

Default cap is 8 MiB. Disable with `[clipboard] enable = false`.

## Pair with a Linux client

On the peer (River/Sola seat, `WAYLAND_DISPLAY` live):

```bash
sola-kvm listen --bind 0.0.0.0:4242
```

Linux inject sends each key **before** the matching `modifiers()` update so
River sees Super the same way as a physical keyboard. Super+Tab on the
peer confirms when Super is released (bare Super_L `ChordReleased`).

On novus: `peer.host` = that machine’s LAN IP, `mac_width` / `mac_height` =
the peer’s output (e.g. canto 1920×1080). Then:

```bash
sola-kvm send-test --to <peer-ip>:4242
# full path: sola-kvm server --input evdev (managed by sola)
```

`--dump` on listen logs packets and does not inject.

## Pair with Phase B (ember / macOS)

1. Mac agent listening on `0.0.0.0:4242` (Accessibility granted).
2. On novus: `sola-kvm show` → confirm peer IP.
3. Smoke without full grab:

```bash
sola-kvm send-test --to 10.0.0.21:4242
```

4. Full path: `sola-kvm server --input evdev` (or feed) + Mac agent injects.

## State machine (what “works” without live Sola)

Pure logic in `crates/sola-kvm/src/server.rs` (unit tested):

1. **Local** — integrate relative motion in primary space; clamp to output.
2. **Edge enter** — motion that would leave primary into the virtual Mac rect
   → `Enter` + `Motion` UDP, grab side-effect.
3. **Remote** — scale motion → Mac-local abs `Motion`; forward button/key/scroll.
   On the Mac agent, scroll is injected as **pixel** CG events with a simple
   **velocity gain** (slow notches stay small; fast spins ramp distance).
   Synthetic CG does not use macOS HID wheel acceleration — retune
   `scroll_accel` constants in `apps/sola-kvm-mac` if the ramp feels off.
4. **Leave** — virtual cursor exits toward primary (or `leave` / force) →
   synthetic button/key ups (stuck-modifier recovery) → `Leave` → release grab.

## Build / test

```bash
cargo test -p sola-kvm
cargo make build sola-kvm
# do not install without explicit permission
```
