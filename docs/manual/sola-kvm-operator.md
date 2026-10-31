# KVM

Sola's KVM lets the physical keyboard and mouse of one machine, the server,
control another machine, the peer, over UDP. The server runs Sola. When the
pointer crosses the shared screen edge, input goes to the peer, which
injects it: a Linux peer runs `sola-kvm listen`, a Mac peer runs
`sola-kvm-mac`. The design is in the
[KVM spec](../specs/2026-07-27-sola-kvm-design.md).

`sola-kvm` ships on Oath in `pkg:sola`. The Mac agent, `sola-kvm-mac`, is
built and installed on the Mac from
[`apps/sola-kvm-mac`](../../apps/sola-kvm-mac/).

## Server and peers

| Machine | How it runs |
|---------|-------------|
| Server (the machine whose keyboard and mouse are shared) | `sola-kvm server`; bare `sola-kvm` is the same. It captures through the compositor, not `/dev/input`. Under the `sola` process manager, `sola-kvm` is in the managed set and starts 750 ms after the other managed processes, as `server`. |
| Linux peer | `sola-kvm listen` (Wayland virtual pointer and keyboard), in a running River or Sola session. The keyboard starts with NumLock on, so the server's number pad types digits (a Mac's has no NumLock); the server's NumLock key (a Mac's Clear) toggles it. |
| Mac peer (optional) | LaunchAgent `com.sola.kvm-mac` runs `/Applications/SolaKvmMac.app/Contents/MacOS/sola-kvm-mac listen --bind 0.0.0.0:4242`. |

Neither side starts unpaired: pair them first ([Pairing](#pairing)).

On Oath the `sola` process manager does not run: PID 1 runs `sola-kvm` as
the service `svc:sola-kvm`, as the desk user, and restarts it whenever it
exits. Oath's desk profile has it as a peer, `sola-kvm listen --bind
0.0.0.0:4242`, after `svc:sola-river`, and off: turn it on once the machine
is paired. To make an Oath machine the server, change that service's `exec`
to `sola-kvm server` with `oath set`.

## Requirements

The server captures through the compositor, which must offer:

| Protocol | For |
|----------|-----|
| `zwlr_layer_shell_v1` | The strip on the shared edge, and the keyboard while remote (exclusive keyboard interactivity) |
| `zwp_pointer_constraints_v1` | Locking the pointer while remote |
| `zwp_relative_pointer_manager_v1` | Reading motion while the pointer is locked |
| `zwp_keyboard_shortcuts_inhibit_manager_v1` | Keeping the window manager's key and pointer bindings off while remote |

rivulet offers all four. River 0.4.5 has no shortcuts inhibitor: there the
server starts, logs that the window manager's bindings still fire, and
Super chords act on the server while the pointer is on the peer. On River,
layer surfaces also need `sola-river` bound to `river_layer_shell_v1` (see
[sola-river layer-shell](sola-river-layer-shell.md)).

## Config

```bash
# write defaults to ~/.config/sola-kvm/config.toml (--force overwrites)
sola-kvm init
# from a source checkout: cargo run -p sola-kvm -- init

sola-kvm show
```

Key fields:

| Section | Fields |
|---------|--------|
| `[peer]` | `host` (the peer's IP; `init` writes a sample address, so set it), `port` (default `4242`) |
| `[layout]` | `side` (`right`, `left`, `top`, `bottom`), `align`, `mac_width`, `mac_height` (the peer's output size, for any peer, not only a Mac), optional `offset_x` / `offset_y` |
| `[motion]` | `scale`: multiplies relative motion while the pointer is on the peer. `edge_band` (default 64 px) and `enter_push` (default 48): for the feed and demo backends, how near the shared edge the pointer must be and how far you push against it before it enters; with the compositor, reaching the edge strip enters. |
| `[primary]` | The server's logical output size (default `5120×2160`). Set it by hand: it is not read from the bus (`OutputGeometry`). |
| `[clipboard]` | `enable` (default `true`), `max_bytes` (default 8 MiB), `sync_on_enter`, `sync_on_leave` (both default `true`). See [Clipboard](#clipboard). |

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
host = "<peer-ip>"
port = 4242
```

## Run the server

`sola-kvm` logs to stderr and to `sola.log` in Sola's log directory
(`~/.local/state/sola/` on Oath), which it shares with the rest of Sola;
look for `sola-kvm` lines.

### Autostart

1. Write `~/.config/sola-kvm/config.toml` with `peer.host` set to the
   peer's IP. Without the file, `sola-kvm` runs on the defaults and says
   so in the log.
2. Pair the server with the peer ([Pairing](#pairing)).
3. Start the session. On Oath, PID 1 starts `svc:sola-kvm`; elsewhere,
   `sola` starts `sola-kvm` once the Wayland socket is up. Either way
   `sola-kvm` waits up to 30 seconds for the Wayland socket so it can put
   its edge strip on screen.

To run the server by hand, for debugging:

```bash
sola-kvm server
# from a source checkout: cargo run -p sola-kvm -- server
```

### Feed backend

Reads a line protocol on stdin. Useful for development, and for testing a
peer without capturing input. Like every backend it sends only to the
paired peer; it waits up to 3 s for the peer to answer the handshake.

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

### Compositor capture

The default. `sola-kvm` keeps a strip 2 px wide along the shared edge of
the first output, on the overlay layer. When the real pointer reaches it,
and the paired peer is answering, the strip:

1. takes the keyboard (exclusive keyboard interactivity), so the focused
   window gets a keyboard leave;
2. locks the pointer and reads relative motion;
3. inhibits the compositor's shortcuts, so Super chords and pointer
   bindings go to the peer (rivulet keeps only Ctrl+Alt+F1–F12).

If the compositor does not hand over the keyboard or the lock within
0.4 s, the pointer stays on the server. While the peer is not answering
the handshake, reaching the edge does nothing and the log says so (at
most every 10 s).

The pointer comes back when the peer's cursor crosses back over the
shared edge, on Ctrl+Alt+Esc (from anywhere on the peer), when the
compositor takes the keyboard or the lock away (a VT switch, the lock
screen), or when the peer stops answering. Coming back gives everything
up and puts the cursor 32 px inside the server's edge, so it does not
cross straight back.

Wayland does not repeat keys for a client; `sola-kvm` repeats a held key
at the seat's rate and delay, as the kernel did, for a Mac peer (a Linux
peer ignores the repeats and lets its compositor repeat).

## Clipboard

The clipboard goes over TCP on the same port as the UDP input (a different
protocol). It syncs only when the pointer enters the peer (server to peer,
`sync_on_enter`) or leaves it (peer to server, `sync_on_leave`).

| Payload | Linux peer | Mac peer |
|---------|------------|----------|
| UTF-8 text | yes | yes |
| `image/png` (screenshots, Preview Copy) | yes | rejected; the Mac pasteboard keeps its contents |

The default cap is 8 MiB (`max_bytes`). Turn sync off with
`[clipboard] enable = false`.

## Pairing

Each machine has its own key pair, made the first time it pairs:
`~/.config/sola-kvm/key` (private, readable by you only) and `key.pub`.
Pairing swaps the public keys, as SSH swaps host keys: each machine keeps
the other's in `~/.config/sola-kvm/peer.pub`. On the server, once, with ssh
to the peer working:

```bash
sola-kvm pair <peer-host>                            # a Linux peer (home@<host> for Oath)
sola-kvm pair <mac-host> --remote-bin sola-kvm-mac   # a Mac peer
```

It sends this machine's public key to the peer over ssh (`pair --accept`
there), which keeps it and answers with its own, then keeps that here, and
prints both. Without a peer, it uses `peer.host` from the config. If the
peer cannot take the key, nothing changes here. Then restart sola-kvm on
both machines (on Oath, `pkill -x sola-kvm`; PID 1 starts it again); each
logs `paired` at start.

Without ssh, pair by hand: `sola-kvm pair --public` prints a machine's
public key, and `echo <the other's key> | sola-kvm pair --accept` keeps the
other's key and prints this one's. Do it in both directions.

Paired, every input packet and clipboard message is encrypted and
authenticated with Noise IK (`Noise_IK_25519_ChaChaPoly_BLAKE2s`) between
the two keys. For input, the server sends a handshake, the peer answers,
and packets travel under that session's keys, with a new session every two
minutes; the peer drops a packet from any other key, one it has already
taken, and a handshake no later than the last (only the server's clock has
to move forward). The clipboard connection runs its own handshake, and the
peer refuses a connection from any other key. Dropped input is logged at
most every 10 s, with the sender's address.

Neither side starts unpaired, and neither starts with keys it cannot read.
A machine paired by an older sola-kvm has a shared key,
`~/.config/sola-kvm/pair.key`: it does not start until paired again, which
removes that file. `sola-kvm pair --forget` (`sola-kvm-mac pair --forget`)
forgets the other machine's key: that side does not start again until
paired. Pair both sides from versions that pair by public key.
[Security](security.md) lists the desk's other ways in.

## Set up a Linux peer

On the peer, in a running River or Sola session (`WAYLAND_DISPLAY` set):

```bash
sola-kvm listen --bind 0.0.0.0:4242
```

`--dump` logs packets and does not inject.

The peer injects each key before the matching `modifiers()` update, so
River sees Super the same way as from a physical keyboard, and Super
chords on the peer (Super+Tab, Super+Space) work as they do locally.
The server's key repeats (`pressed` 2 on the wire) are not injected: the
compositor would stack press counts and leave Super or Alt stuck (the
terminal drops Super+letter or prints ESC and the character). The
compositor generates its own repeat.

On the server, set `peer.host` to the peer's LAN IP and `mac_width` /
`mac_height` to the peer's output size (for example 1920×1080). Then test:

```bash
sola-kvm send-test --to <peer-ip>:4242
# then the full path: the server as it runs in the session (see Autostart)
```

## Set up a Mac peer

1. On the Mac, `apps/sola-kvm-mac/scripts/install.sh` builds the agent,
   signs it as `/Applications/SolaKvmMac.app`, and installs the
   LaunchAgent, which listens on `0.0.0.0:4242`. Grant it the
   Accessibility permission once (System Settings → Privacy & Security →
   Accessibility).
2. On the server, run `sola-kvm show` and confirm the peer IP.
3. Test without capturing input: `sola-kvm send-test --to <peer-ip>:4242`.
4. Run the full path: `sola-kvm server` (or the feed backend), and the
   Mac agent injects.

## How it works

The server is a state machine in
[`crates/sola-kvm/src/server.rs`](../../crates/sola-kvm/src/server.rs):

1. **Local**: relative motion moves a position in the server's output
   space, clamped to the output.
2. **Edge enter**: the pointer reaching the edge strip (with the feed and
   demo backends, motion that would leave the server's output into the
   peer's rectangle) captures input and sends `Enter` and `Motion` over
   UDP.
3. **Remote**: motion is scaled and sent as absolute `Motion` in the peer's
   coordinates; buttons, keys, and scroll are forwarded. The Mac agent
   injects scroll as pixel CG events with a simple velocity gain: slow
   notches stay small, fast spins cover more distance. Synthetic CG events
   do not get macOS HID wheel acceleration; if the ramp feels wrong, retune
   the `scroll_accel` constants in
   [`apps/sola-kvm-mac`](../../apps/sola-kvm-mac/).
4. **Leave**: when the peer's virtual cursor exits toward the server (or on
   `leave`, or a forced leave), the server sends synthetic button and key
   releases so no modifier stays stuck, sends `Leave`, and gives the
   keyboard and pointer back.

## Build from source

```bash
cargo test -p sola-kvm
cargo make build sola-kvm
```
