# KVM

Sola's KVM lets the physical keyboard and mouse of one machine, the server,
control another machine, the peer, over UDP. The server runs Sola. When the
pointer crosses the shared screen edge, input goes to the peer, which
injects it: a Linux peer runs `sola-kvm listen`, a Mac peer runs
`sola-kvm-mac`. The design is in the
[KVM spec](../specs/2026-07-27-sola-kvm-design.md).

## Server and peers

| Machine | How it runs |
|---------|-------------|
| Server (the machine whose keyboard and mouse are shared) | Started by `sola` after River is up: `sola-kvm` is in the process manager's managed set (`MANAGED`) and runs `server --input evdev`. |
| Linux peer | `sola-kvm listen` (Wayland virtual pointer and keyboard). The keyboard starts with NumLock on, so the server's number pad types digits (a Mac's has no NumLock); the server's NumLock key (a Mac's Clear) toggles it. On Oath: `svc:sola-kvm` as `home`. |
| Mac peer (optional) | LaunchAgent `com.sola.kvm-mac` runs `/opt/sola/bin/sola-kvm-mac listen`. |

## Requirements

Edge capture and exclusive keyboard focus need `sola-river` bound to
`river_layer_shell_v1`, with Meta chord suppression during exclusive focus.
Without it, River closes layer surfaces immediately, and shell chords still
take Meta keys while the pointer is on the peer.

See [sola-river layer-shell](sola-river-layer-shell.md). After a restart,
the log shows:

```text
bound river_layer_shell_v1 (layer-shell clients enabled)
```

## Config

```bash
# write defaults to ~/.config/sola-kvm/config.toml
sola-kvm init
# from a source checkout: cargo run -p sola-kvm -- init

sola-kvm show
```

Key fields:

| Section | Fields |
|---------|--------|
| `[peer]` | `host` (the peer's IP), `port` (default `4242`) |
| `[layout]` | `side` (`right`, `left`, `top`, `bottom`), `align`, `mac_width`, `mac_height` (the peer's output size, for any peer, not only a Mac) |
| `[motion]` | `scale`: multiplies relative motion while the pointer is on the peer |
| `[primary]` | The server's logical output size (default `5120×2160`). Set it by hand: it is not read from the bus (`OutputGeometry`). |
| `[clipboard]` | `enable` (default `true`), `max_bytes` (default 8 MiB). See [Clipboard](#clipboard). |

A Mac to the right of the server, bottom edges aligned:

```toml
[layout]
side = "right"
align = "bottom"
mac_width = 2560
mac_height = 2880

[motion]
scale = 1.25   # tune until motion on the Mac feels like local

[peer]
host = "10.0.0.21"
port = 4242
```

## Run the server

`sola-kvm` logs to stderr and to `/opt/sola/log/sola.log`, which it shares
with the rest of Sola; look for `sola-kvm` lines.

### Autostart

1. Install `sola` and `sola-kvm` in `/opt/sola/bin/`. The process manager's
   managed set (`MANAGED`) must include `sola-kvm`.
2. Write `~/.config/sola-kvm/config.toml` with `peer.host` set to the
   peer's IP.
3. Give the seat user access to the input devices; the `evdev` backend
   needs it. See [Input device access](#input-device-access).
4. Start Sola from the TTY as usual. `sola` launches `sola-kvm` once the
   Wayland socket is ready.

To run the server by hand, for debugging:

```bash
/opt/sola/bin/sola-kvm server --input evdev
# from a source checkout: cargo run -p sola-kvm -- server --input evdev
```

### Input device access

The `evdev` backend needs read and write access to `/dev/input/event*`.
Re-plugging a device creates new nodes, so the access has to survive a
re-plug.

**Permanent (preferred).** On NixOS with `services.sola.enable`, the Sola
NixOS module installs a udev rule (`TAG+="uaccess"` and `GROUP=input`), so
logind grants the seat user access again on every plug. Rebuild and switch
the host configuration that imports `nix/module.nix`. The rule is
[`crates/sola-kvm/udev/99-sola-kvm-input.rules`](../../crates/sola-kvm/udev/99-sola-kvm-input.rules).

**One-shot ACL**, until the udev rule is in place. Always quote the `-m`
argument, and never use a bare `$USER` that might be empty: `u::rw-` fails
with *Invalid argument near character 3*.

```bash
# helper in a source checkout (re-execs under sudo, expands the user name safely)
sudo crates/sola-kvm/scripts/grant-input-acl.sh
# the same as a one-liner:
sudo setfacl -m "u:$(id -un):rw" /dev/input/event[0-9]*
killall sola-kvm   # sola relaunches it, and it re-opens the devices
```

If the log shows `pointers=0 keyboards=1`, the mouse node is not readable,
and `sola-kvm` refuses to enter the peer. This prevents a split where the
mouse stays on the server while the keyboard goes to the peer.

### Feed backend

Reads a line protocol on stdin and needs no device permissions. Useful for
development, and for testing a peer without grabbing input devices.

```bash
# terminal A: the Linux peer injects (or add --dump to log packets)
sola-kvm listen --bind 0.0.0.0:4242

# terminal B: drive the server locally
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
`scroll dx dy`, `leave`, and `#` comments.

### Demo backend

Sends a scripted sequence to the peer in the config: a start near the right
edge, enter, motion, a button, a key, and scroll, then leave and idle. The
peer must accept UDP (`sola-kvm listen` or the Mac agent).

```bash
sola-kvm server --input demo
# with another config file:
sola-kvm -c /path/to.toml server --input demo
```

### Evdev backend

Reads `/dev/input/event*`. `sola-kvm` keeps a thin layer-shell strip on the
shared edge; when the real pointer hits it, input goes to the peer, and
`sola-kvm` takes `EVIOCGRAB` on all open nodes until the pointer comes
back. Without layer-shell in `sola-river` (see
[Requirements](#requirements)), edge enter is disabled and the log says
`layer-shell barrier unavailable`; the feed and demo backends still work.

```bash
# needs read and write on the event nodes (input group or seat ACL)
sola-kvm server --input evdev
```

Limits:

- On leave, the local pointer is not warped to the return point; the point
  is only logged. Sola has no compositor warp for it yet.
- Meta chords reach the peer (as Cmd on a Mac) only through layer-shell
  exclusive focus and `sola-river`'s chord suppression. The evdev grab keeps
  events from the compositor but does not by itself disable River's shell
  key bindings. When testing against the Mac agent, try the feed or demo
  backend first.

## Clipboard

The clipboard goes over TCP on the same port as the UDP input (a different
protocol). It syncs only when the pointer enters the peer (server to peer)
or leaves it (peer to server).

| Payload | Linux peer | Mac peer |
|---------|------------|----------|
| UTF-8 text | yes | yes |
| `image/png` (screenshots, Preview Copy) | yes | rejected; the Mac pasteboard keeps its contents |

The default cap is 8 MiB (`max_bytes`). Turn sync off with
`[clipboard] enable = false`.

## Pair with a Linux peer

On the peer, in a running River or Sola session (`WAYLAND_DISPLAY` set):

```bash
sola-kvm listen --bind 0.0.0.0:4242
```

`--dump` logs packets and does not inject.

The peer injects each key before the matching `modifiers()` update, so
River sees Super the same way as from a physical keyboard, so Super
chords on the peer (Super+Tab, Super+Space) work as they do locally. Kernel auto-repeat (`EV_KEY` value 2) is not injected:
River would stack press counts and leave Super or Alt stuck (the terminal
drops Super+letter or prints ESC and the character). The compositor
generates its own repeat.

On the server, set `peer.host` to the peer's LAN IP and `mac_width` /
`mac_height` to the peer's output size (for example 1920×1080). Then test:

```bash
sola-kvm send-test --to <peer-ip>:4242
# then the full path: sola-kvm server --input evdev (started by sola)
```

## Pair with a Mac peer

1. Start the Mac agent listening on `0.0.0.0:4242`, with the Accessibility
   permission granted.
2. On the server, run `sola-kvm show` and confirm the peer IP.
3. Test without grabbing input: `sola-kvm send-test --to 10.0.0.21:4242`.
4. Run the full path: `sola-kvm server --input evdev` (or the feed
   backend), and the Mac agent injects.

## How it works

The server is a state machine in
[`crates/sola-kvm/src/server.rs`](../../crates/sola-kvm/src/server.rs):

1. **Local**: relative motion moves a position in the server's output
   space, clamped to the output.
2. **Edge enter**: motion that would leave the server's output into the
   peer's rectangle (with the evdev backend, a hit on the edge strip)
   sends `Enter` and `Motion` over UDP and grabs input.
3. **Remote**: motion is scaled and sent as absolute `Motion` in the peer's
   coordinates; buttons, keys, and scroll are forwarded. The Mac agent
   injects scroll as pixel CG events with a simple velocity gain: slow
   notches stay small, fast spins cover more distance. Synthetic CG events
   do not get macOS HID wheel acceleration; if the ramp feels wrong, retune
   the `scroll_accel` constants in
   [`apps/sola-kvm-mac`](../../apps/sola-kvm-mac/).
4. **Leave**: when the peer's virtual cursor exits toward the server (or on
   `leave`, or a forced leave), the server sends synthetic button and key
   releases so no modifier stays stuck, sends `Leave`, and releases the
   grab.

## Build from source

```bash
cargo test -p sola-kvm
cargo make build sola-kvm
```
