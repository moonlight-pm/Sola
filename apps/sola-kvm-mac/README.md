# sola-kvm-mac

macOS client for **sola-kvm** (ember).

Listens for UDP KVM packets from **novus** and injects pointer/keyboard via
**CoreGraphics** (`CGWarpMouseCursorPosition` + `CGEventPost`).

**Status:** Phase B — agent source + LaunchAgent + Linux-testable decode/keymap.
Built and unit-tested on Linux (inject is a logging stub off-macOS). **Real
CGEvent inject must be verified on ember.**

## Role

| Concern | Behavior |
|---------|----------|
| Bind | UDP `0.0.0.0:4242` (override with `--bind`) |
| `Enter` | Warp cursor to absolute Mac `(x, y)` |
| `Motion` | Absolute warp/set in Mac screen coords |
| `Button` / `Key` / `Scroll` | `CGEvent` inject |
| `Leave` | Release any keys we still think are down |
| Session | **GUI login** LaunchAgent — not SSH |

## Layout (source)

```text
apps/sola-kvm-mac/
  Cargo.toml              # standalone package (not a Sola workspace member)
  src/
    main.rs               # CLI: listen | version
    agent.rs              # UDP loop
    protocol.rs           # KVM1 decode (mirror of crates/sola-kvm)
    keymap.rs             # Linux evdev → CGKeyCode
    inject.rs             # CGEvent (macOS) / stub (other)
  LaunchAgents/
    com.sola.kvm-mac.plist
  scripts/
    install-launchagent.sh
  README.md
```

Wire format is owned by `crates/sola-kvm/src/protocol.rs`. This tree keeps a
**byte-compatible decoder** so the Mac agent does not depend on the full Sola
workspace. If the server adds fields, update the mirror here (or escalate —
do not silently diverge).

## Build (on ember / macOS)

Requires Rust toolchain (`rustup`).

```bash
cd apps/sola-kvm-mac
cargo build --release
# optional install location used by the LaunchAgent template:
sudo mkdir -p /opt/sola/bin
sudo cp target/release/sola-kvm-mac /opt/sola/bin/
sudo chmod 755 /opt/sola/bin/sola-kvm-mac
```

Cross-compile from Linux is **not** set up (needs macOS SDK / osxcross). Ship
source + build on ember.

### Linux (decode / unit tests only)

From this repo on novus:

```bash
cd apps/sola-kvm-mac
cargo test
cargo run -- version
# inject path logs stubs:
cargo run -- listen --bind 127.0.0.1:4242
```

## Accessibility permission

Synthetic input requires TCC **Accessibility**:

1. System Settings → **Privacy & Security** → **Accessibility**
2. Enable **`sola-kvm-mac`** (or the Terminal used when first launching a dev build)
3. If inject still no-ops: remove the entry, re-add, and relaunch the agent

**SSH is unreliable for TCC.** Prefer a LaunchAgent in the Aqua GUI session, or
launch from Terminal.app while logged in at the console.

## LaunchAgent (GUI session)

```bash
# After building + copying binary to /opt/sola/bin/sola-kvm-mac:
./scripts/install-launchagent.sh --bin /opt/sola/bin/sola-kvm-mac --bind 0.0.0.0:4242
```

Manual equivalent:

```bash
mkdir -p ~/Library/LaunchAgents ~/Library/Logs
# edit ProgramArguments + log paths in the plist, then:
cp LaunchAgents/com.sola.kvm-mac.plist ~/Library/LaunchAgents/
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.sola.kvm-mac.plist
launchctl enable gui/$(id -u)/com.sola.kvm-mac
launchctl kickstart -k gui/$(id -u)/com.sola.kvm-mac
```

Unload:

```bash
launchctl bootout gui/$(id -u)/com.sola.kvm-mac
```

Logs: `~/Library/Logs/sola-kvm-mac.{out,err}.log`

### Why not SSH / system daemon

- `CGEventPost` / Accessibility grants attach to the **GUI user’s** session.
- `launchctl` domain must be **`gui/$(id -u)`**, not `system/`.
- Do not use a LaunchDaemon as root for input injection.

## Manual test from novus

On **ember**, agent running and Accessibility granted:

```bash
# on ember (foreground smoke):
RUST_LOG=debug /opt/sola/bin/sola-kvm-mac listen --bind 0.0.0.0:4242
```

On **novus**:

```bash
# from Sola tree (Phase A tools):
cargo run -p sola-kvm -- send-test --to 10.0.0.21:4242
# or installed:
sola-kvm send-test --to <ember-ip>:4242 --x 100 --y 200
```

Expected sequence (see `crates/sola-kvm` `send-test`):

1. `Enter` → cursor jumps near (100, 200) on Mac
2. `Motion` → small move
3. Left button down/up
4. Key `A` (evdev 30) down/up
5. Scroll tick
6. `Leave`

Firewall: allow UDP **4242** from novus → ember on the trusted LAN.

## Key map (v1)

Linux `KEY_*` → Mac `kVK_*` for:

- Letters A–Z, digits 0–9, common punctuation
- Modifiers: Ctrl / Shift / Alt(Option) / Meta(Command) (left + right)
- Arrows, Home/End/PageUp/PageDown, Backspace, Forward Delete
- Space, Escape, Tab, Enter
- F1–F12

Unmapped keycodes are logged and dropped. Extend `src/keymap.rs` as needed.

## Wire format notes (read-only vs server)

Matches `crates/sola-kvm` v1:

| Field | Value |
|-------|--------|
| Magic | `u32` LE `0x4b564d31` (on wire bytes `31 4d 56 4b`) |
| Version | `1` |
| Header | magic + version + type + seq(u32) = 10 bytes |
| Button | `0=left, 1=right, 2=middle`; `pressed` 0/1 |
| Key | Linux **evdev** keycode `u32` + `pressed` u8 |
| Motion / Enter | absolute Mac-local `i32` x,y |

### Gaps / escalate (do not change protocol from this tree)

No protocol changes required for Phase B. If Phase C needs:

- relative motion
- high-res scroll units
- modifier mask semantics beyond KEY events

document the need and let Phase C update `crates/sola-kvm` first; then mirror here.

## Non-goals (v1)

TLS, pairing UI, clipboard, multi-client, reverse control (Mac → novus).

## Done checklist

- [x] UDP listen + warp on Enter / abs Motion
- [x] Button / key / scroll inject + Linux → CGKeyCode table
- [x] LaunchAgent plist + install script (gui domain)
- [x] README + `send-test` instructions
- [x] Decode/keymap unit tests runnable on Linux
- [ ] **On-device:** CGEvent inject smoke on real ember (manual)
