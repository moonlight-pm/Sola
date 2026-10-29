# Secure access to the desk — design

**Status:** proposed 2026-10-29 (Joshua: "I'd like to resolve these so that our docs can
honestly describe a secure setup, that exists"). Answers [D11](../open-questions.md#d11--what-quit-sola-means-on-oath-p1--answered-2026-10-29)
and [D12](../open-questions.md#d12--the-public-manual-says-the-desk-has-no-login-and-no-lock-screen-p1).

## Goal

Every way into a running desk can be closed, and the manual says how:

| Way in | Today | With this |
|---|---|---|
| The network, through the KVM | `sola-kvm listen` injects input from anyone (UDP and clipboard TCP on `0.0.0.0:4242`) | Paired machines only (part 1) |
| The keyboard, at boot | The desk starts signed in | Optionally asks for the password (part 3) |
| The keyboard, while away | No lock | Super+L; optionally on idle (part 3) |
| The console, after Exit Sola | Nothing on the VT; only a reboot brings the desk back | A console login on the VT (part 4) |

All of it is **optional** and stays out of the way once set: with a password set,
Joshua's own setup still starts signed in (auto-login), never locks on its own,
and locks only on Super+L. The defaults are today's behavior; the manual
describes both the open default and the closed setup, and what each leaves open.

## 1. KVM pairing (Sola: `sola-kvm`, `apps/sola-kvm-mac`) — built 2026-10-29

- `sola-kvm pair <peer>` on the server makes a 32-byte key, installs it on the
  peer over ssh (`ssh <peer> sola-kvm pair --accept`, key on stdin; a Mac:
  `--remote-bin sola-kvm-mac`), then saves it here: `~/.config/sola-kvm/pair.key`
  (0600, 64 hex digits) on both. If the peer refuses, neither side changes.
  `pair --forget` removes it. Each side reads the key at start.
- **A side with a key speaks only paired traffic; a side without one behaves
  as before** (and logs `not paired`; the manual says the port is open). A key
  file that cannot be read stops sola-kvm instead of running open.
- **Envelope** (`crates/sola-kvm/src/seal.rs`, included by the Mac agent):
  `KVMS` | version 2 | channel (input / clipboard) | sender id (16 random bytes
  per process) | counter (u64) | time (unix ms) | XChaCha20-Poly1305
  ciphertext and tag; the header is the associated data and the nonce is
  id + counter, so nonces never repeat under the key.
- **The receiver drops** what does not open, what is sealed for the other
  channel, what is more than 30 s from its clock, and a counter already seen
  from that sender (a window of 64 lets UDP reorder). A replay is caught by
  the counter inside the window and by the clock after it. No handshake:
  input does not wait for the clipboard connection.
- **Input:** every UDP datagram is an envelope. **Clipboard:** every TCP
  message is one length-prefixed envelope.
- Crates already in the lockfile: `chacha20poly1305` 0.10, `getrandom` 0.2.
- **Rollout:** the server runs on novus (`master`, the `sola-kvm` worktree),
  the peer on canto (`next`), and the Mac agent is its own crate. All get the
  change before anyone pairs; an unpaired machine keeps working meanwhile.

## 2. The password (Oath)

Oath owns the password of the desk's user, not Sola:

- Stored hashed by Oath; none set by default.
- A small root service with a local socket (peer checked by `SO_PEERCRED`)
  answers: **status** (password set? auto-login on?), **check** (password →
  right / wrong, with a growing delay after wrong ones), **set** (old, new;
  new empty removes it), **auto-login** (on / off; needs the password).
- `oath passwd` sets it from a console. Settings → Login sets it on the desk,
  through the same service.
- **Auto-login** belongs to Oath because the console login (part 4) reads it
  too. Default on.

The exact socket path, wire format and service name are Oath's to choose; Sola
needs only the four calls.

## 3. The lock screen (Sola: new `sola-lock`)

- A Wayland client on `ext-session-lock-v1` (rivulet has it): while locked,
  the compositor shows and sends input only to the lock surfaces, and stays
  locked if `sola-lock` dies until a new one takes over. Drawn with
  smithay-client-toolkit + tiny-skia + cosmic-text (iced and winit cannot make
  lock surfaces): the background picture dimmed, the clock, a password field,
  the reason for a refusal. The password is checked by Oath's service (part 2).
- **When it locks:**
  - Super+L — whenever a password is set. With none, Super+L says
    "Set a password in Settings → Login to lock."
  - At desk start, when auto-login is off.
  - When the idle fade is complete, if **Lock when idle** is on (off by
    default).
- **Settings → Login:** Password (Set / Change / Remove), **Sign in
  automatically** (auto-login, on), **Lock when idle** (off), a line naming
  Super+L.
- Typing the password through a paired KVM works like the local keyboard.

## 4. Exit Sola drops to a console (Oath, with a small Sola part)

- **Exit Sola** ends the desk as today. Oath then starts a console login on the
  desk's VT: with auto-login on it opens a shell as the desk's user; with it
  off it asks for the password (part 2).
- From that console, one command brings the desk back (Oath's to name, e.g.
  `oath desk`).
- Sola's part: Exit Sola tells Oath the desk ended on purpose (so Oath starts
  the console instead of treating it as a crash), however Oath wants to hear
  it.

## Menu

The flower menu's **Quit Sola** is now **Exit Sola**, and **Restart Shell** is
gone (2026-10-29, Joshua: "Just get rid of that menu item").

## Order

1. KVM pairing — Sola alone (both branches, the Mac agent).
2. Oath's password service and console login — the Oath session (buccina).
3. `sola-lock` and Settings → Login — against Oath's service once it exists.
4. Exit Sola → console — the Oath side of part 4 plus Sola's notice.
5. The manual: a Security chapter (what is open by default, how to close each
   way in), and the Introduction's "Starting a session" (D12).
