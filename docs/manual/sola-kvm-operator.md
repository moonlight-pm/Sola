# sola-kvm operator guide (novus server)

Software KVM: physical mouse/keyboard on **novus** control **ember** (macOS)
over UDP. Design: `docs/specs/2026-07-27-sola-kvm-design.md`.

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
| `[peer]` | `host` (ember IP, default `10.0.0.21`), `port` (default `4242`) |
| `[layout]` | `side` (`right`/`left`/`top`/`bottom`), `align`, `mac_width`, `mac_height` |
| `[motion]` | `scale` — multiplies relative deltas while remote |
| `[primary]` | novus logical size (default `5120×2160`); Phase C does not yet pull bus `OutputGeometry` |

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

Logs: `/opt/sola/log/sola-kvm.log` (via `sola_core::log`) and stderr.

### Feed backend (default — no device perms)

Line protocol on stdin. Useful for development and pairing with Phase B
without HID grab.

```bash
# terminal A — dump packets (or run Mac agent on ember)
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

## Pair with Phase B (ember)

1. Mac agent listening on `0.0.0.0:4242` (Accessibility granted).
2. On novus: `sola-kvm show` → confirm peer IP.
3. Smoke without full grab:

```bash
sola-kvm send-test --to 10.0.0.21:4242
# or demo / feed as above
```

4. Full path: `sola-kvm server --input evdev` (or feed) + Mac agent injects.

## State machine (what “works” without live Sola)

Pure logic in `crates/sola-kvm/src/server.rs` (unit tested):

1. **Local** — integrate relative motion in primary space; clamp to output.
2. **Edge enter** — motion that would leave primary into the virtual Mac rect
   → `Enter` + `Motion` UDP, grab side-effect.
3. **Remote** — scale motion → Mac-local abs `Motion`; forward button/key/scroll.
4. **Leave** — virtual cursor exits toward primary (or `leave` / force) →
   synthetic button/key ups (stuck-modifier recovery) → `Leave` → release grab.

## Build / test

```bash
cargo test -p sola-kvm
cargo make build sola-kvm
# do not install without explicit permission
```
