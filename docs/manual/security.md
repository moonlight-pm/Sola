# Security

By default a Sola desk on Oath trusts whoever sits at its keyboard: the
desk starts signed in and nothing locks it. Each way in can be closed. This chapter lists each way into a running desk, what it lets
through by default, and what closes it. Oath keeps the desk user's
password and decides who has the screen and keyboard; Sola draws the
desk, locks it, and runs the KVM. The design is in the
[secure access spec](../specs/2026-10-29-secure-access-design.md).

## Ways in

| Way in | By default | To close it |
|---|---|---|
| The network, through the KVM | `sola-kvm` does not start unpaired (Oath's desk profile has `svc:sola-kvm` off); paired, it takes input and clipboard only from the paired machine's key | Nothing more to close; to open it, pair ([Pairing the KVM](#pairing-the-kvm)) |
| The network, over SSH | Oath's sshd takes keys only (no passwords, no root login) | Nothing more to close |
| The keyboard, at boot | The desk starts signed in | A password, with **Sign in automatically** off ([The password](#the-password)) |
| The keyboard, while you are away | Nothing locks the desk | Super+L, or **Lock when the windows have faded** ([Locking](#locking)) |
| The keyboard, at the console | The console that **Exit Sola** or `oath desk stop` puts on the screen opens a shell as `home` without asking | A password, with **Sign in automatically** off |

The password, the lock and the console login need Oath's login service
(`svc:login`, Oath R42 and later). Without it there is no password: the
desk starts signed in, Super+L says **Locking needs Oath's login
service**, and Settings → Login says the service is not running.

## Pairing the KVM

`sola-kvm` (the server, and `listen` on a peer) and the Mac agent do not
start unpaired, and say so in their log. Pairing gives each machine its own
key pair and the other's public key, as SSH swaps host keys; from then on
each drops whatever does not come from the other's key.

On the server, once, with ssh to the peer working:

```bash
sola-kvm pair home@<peer-host>                       # an Oath peer
sola-kvm pair <mac-host> --remote-bin sola-kvm-mac   # a Mac peer
```

The peer is an ssh destination; without one, `pair` uses `peer.host` from
the server's config. It sends the server's public key to the peer
(`sola-kvm pair --accept` there), which keeps it and answers with its own,
which the server keeps. The private key, `~/.config/sola-kvm/key`, never
leaves its machine and is readable by its owner only; the other's public
key is in `peer.pub`. If the peer cannot take the key, the server does not
change. Then restart `sola-kvm` on both machines; on Oath, turn
`svc:sola-kvm` on, or `pkill -x sola-kvm` when it already runs, and PID 1
starts it. Each side then logs `paired` with the other's key (`oath logs
svc:sola-kvm` on Oath). Without ssh, swap the keys by hand with `sola-kvm
pair --public` and `sola-kvm pair --accept` ([KVM notes](sola-kvm-operator.md#pairing)).

Paired, every input packet and every clipboard message is encrypted and
authenticated with Noise IK between the two keys. The peer drops a packet
from any other key, one it has already taken, and a handshake no later
than the last one; it refuses a clipboard connection from any other key.
Dropped input is logged at most every 10 seconds, with the sender's
address. Keys that cannot be read stop `sola-kvm` from starting.

`sola-kvm pair --forget` (`sola-kvm-mac pair --forget` on a Mac) forgets
the other machine's key: that side does not start until paired again. A
machine paired by an older `sola-kvm` (a shared `pair.key`) does not start
until paired again. Setup of the server and of each kind of peer is in
the [KVM notes](sola-kvm-operator.md).

The server captures input through the compositor while the pointer is on
the peer: it holds the keyboard, a pointer lock and a shortcuts inhibitor,
and needs no access to `/dev/input`. Ctrl+Alt+Esc takes the keyboard and
pointer back.

## The password

The desk user, `home`, has no password until one is set. **Settings →
Login** sets, changes or removes it; changing or removing it needs the
current one. Oath keeps it, not Sola: `svc:login` stores it as an
Argon2id hash that only root can read, and answers every wrong password
after a delay that doubles each time, from 1 second up to 30. From a
console or over SSH:

```bash
oath password            # asks the current password (if one is set), then the new one twice
oath password --status   # password: set or none; autologin: on or off; seat: desk or console
```

An empty new password removes it.

**Sign in automatically** (Settings → Login; Oath's `host:local.autologin`,
on by default) decides whether a password is asked for as the desk starts
and when the console opens. With a password set, changing it needs the
current password typed above it. With auto-login off and a password set,
the desk is locked as it starts. A password with **Sign in automatically**
on still lets you lock by hand: the desk starts signed in and never locks
on its own.

## Locking

Super+L, or **Lock Screen** in the flower menu, locks the screen. With no
password set, a toast says **Set a password in Settings → Login to lock**.

**Settings → Appearance → Idle → Lock when the windows have faded** (off by
default) locks once the idle fade is complete.

The lock screen (`sola-lock`) shows the background picture under a dark veil,
the time and the date, and a password field. Return checks the password,
Backspace erases a character, and Escape or Ctrl+U clears the field. Keys
typed while **Checking…** shows are ignored. A password typed through a
paired KVM works like the local keyboard.

While locked, the compositor (`ext-session-lock-v1`) shows and sends input
to the lock screen only. If `sola-lock` dies, the screen stays locked and the
shell starts it again, up to five times a minute.

## The console

The screen and keyboard (tty1) belong either to the desk or to the
console. Every boot starts with the desk. **Exit Sola** closes your
applications, ends the desk, and tells Oath's login service that it ended
on purpose; `oath desk stop`, over SSH, does the same from outside. PID 1
stops the desk's services and starts the console on tty1, which opens a
shell as `home` when auto-login is on and asks for the password when it is
off. `oath desk`, from that shell or over SSH, gives the screen back and
starts the desk's services again. [Oath's
manual](https://oath.moonlight.pm/manual) covers the rest.

## What stays open

These hold with or without a password:

- **`home` is root.** `sudo` asks no password, so any program running as
  `home` can do anything: a terminal, an SSH session, a coding agent in
  Projects, a bot.
- **The bus and the call plane trust every process of `home`.** Any of
  them can read every bus topic (mail and calendar passwords cross the
  bus in clear), publish any topic, and call every method: type and click
  into any window, take screenshots of the screen, and drive Browser and
  Design. `solactl` and its MCP server reach the same methods. See
  [The bus](bus.md) and [The call plane](call.md).
- **Design's share links** have no password and no expiry: anyone with a
  link sees its canvas until **New Link** replaces it.
- **The keyboard can always end the desk.** Every SysRq key is on, so
  anyone at the keyboard can kill the desk (Alt+SysRq+K) or restart the
  machine (Alt+SysRq+B).
- **The disk is not encrypted**, and Oath's boot menu has no password:
  whoever boots another system from a USB stick reads and writes
  everything, `/home` included.
- **A serial console**, where Oath finds a serial port, is a root shell
  with no password.
- **The lock screen** draws without the kit (iced cannot make lock
  surfaces); it follows the picture but not the theme's colors.
- **The lock does not reach through the KVM**: it does not cover another
  computer's screen; lock that computer on its own.
