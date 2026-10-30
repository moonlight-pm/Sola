# Security

By default a Sola desk on Oath trusts the network it is on and whoever
sits at its keyboard: the desk starts signed in, nothing locks it, and
the KVM takes input from any machine that reaches it. This chapter lists
each way into a running desk, what it lets through by default, and what
closes it. Oath keeps the desk user's password and decides who has the
screen and keyboard; Sola draws the desk and runs the KVM.

## Ways in

| Way in | By default | To close it |
|---|---|---|
| The network, through the KVM | `svc:sola-kvm` injects any input that reaches UDP port 4242, and its clipboard connection on TCP port 4242 has no check: anyone on the network can type into the desk and read or set its clipboard | Pair the KVM ([Pairing the KVM](#pairing-the-kvm)) |
| The network, over SSH | Oath's sshd takes keys only (no passwords, no root login) | Nothing more to close |
| The keyboard, at boot | The desk starts signed in | None: Sola has no lock screen |
| The keyboard, while you are away | Nothing locks the desk | None: Sola has no lock screen |
| The keyboard, at the console | Where Oath has `svc:login`, the console that `oath desk stop` puts on the screen opens a shell as `home` without asking | A password, and auto-login off ([The password](#the-password)) |

## Pairing the KVM

Unpaired, `sola-kvm listen` logs `not paired: input and clipboard are
open to the network` when it starts. Pairing gives the server (the
machine whose keyboard and mouse are shared) and the peer one key, and
from then on each drops whatever the other did not seal with it.

On the server, once, with ssh to the peer working:

```bash
sola-kvm pair home@<peer-host>                       # an Oath peer
sola-kvm pair <mac-host> --remote-bin sola-kvm-mac   # a Mac peer
```

The peer is an ssh destination; without one, `pair` uses `peer.host`
from the server's config. It makes a 32-byte key, installs it on the peer over ssh (`sola-kvm pair
--accept` there, the key on standard input), then saves it on the server.
Both keep it at `~/.config/sola-kvm/pair.key`, readable by its owner
only. If the peer cannot take the key, neither side changes. Then restart
`sola-kvm` on both machines; on Oath, `pkill -x sola-kvm`, and PID 1
starts it again. Each side then logs `paired: input and clipboard sealed
with the pair key` (`oath logs svc:sola-kvm` on Oath).

Paired, every input packet and every clipboard message is sealed with
the key (XChaCha20-Poly1305). The receiver drops a message that does not
open with the key, one sealed for the other channel, one it has already
seen, and one whose clock is more than 30 seconds from its own, so keep
both machines' clocks in sync. Dropped input is logged at most every
10 seconds, with the sender's address. A key file that cannot be read
stops `sola-kvm` from starting rather than letting it run open.

`sola-kvm pair --forget` (`sola-kvm-mac pair --forget` on a Mac) removes
that side's key: it is open to the network again, and the other side,
still paired, no longer reaches it. Pair both sides from versions that
have pairing; a side without it cannot read a paired one. Setup of the
server and of each kind of peer is in the [KVM notes](sola-kvm-operator.md).

## The password

The desk user, `home`, has no password until one is set. On an Oath
machine with `svc:login` (where `oath password --status` answers), Oath
keeps it:

```bash
oath password            # asks the current password (if one is set), then the new one twice
oath password --status   # password: set or none; autologin: on or off; seat: desk or console
```

An empty new password removes it. `svc:login` keeps it as an Argon2id
hash that only root can read, and answers every wrong password after a
delay that doubles each time, from 1 second up to 30.

**Auto-login** (`host:local.autologin`) is on by default. To turn it off:

```bash
sudo oath set host:local autologin=false
sudo oath apply host:local
```

The screen and keyboard (tty1) belong either to the desk or to the
console. Every boot starts with the desk. `oath desk stop`, over SSH,
ends the desk on purpose: PID 1 stops the desk's services and starts the
console on tty1, which opens a shell as `home` when auto-login is on and
asks for the password when it is off. `oath desk`, from that shell or
over SSH, gives the screen back and starts the desk's services again.

What Sola does with the password: nothing. It has no lock screen, so
the desk starts signed in at every boot whatever auto-login says, and
Settings has no page for the password. **Exit Sola** does not hand the
screen to the console: the screen is left without a desktop, as described
in [Ending the session](process-manager.md#ending-the-session). So the
password guards one thing: the console after `oath desk stop`, with
auto-login off. [Oath's manual](https://oath.moonlight.pm/manual) covers
the rest.

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
