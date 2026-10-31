# Introduction

Sola is a desktop environment for Linux on Wayland, written in Rust. A
session is a set of separate processes: an event bus, a request/reply
host, a window manager for the compositor, the shell (the top row, the bar
of windows on the left, the launcher), a session manager, and applications
that share one toolkit, sola-kit, and one theme.

Sola runs on Oath, a Linux distribution administered through its catalog
with the `oath` command. Oath supplies the compositor, rivulet (River 0.4.5
ported to C, serving the same protocols), and its PID 1 runs Sola's
processes as services. This manual describes Sola on Oath as it works
today. Sola is pre-release and each part is incomplete; what each can do,
and what it lacks, is listed in [docs/capabilities.md](../capabilities.md).

## Requirements

- An x86-64 machine running Oath with its `desk` profile. The profile adds
  the compositor, sound, fonts, the tools that build Sola, and Sola's own
  packages to Oath's base system.
- An AMD GPU. Oath's Mesa drives AMD GPUs only (radeonsi and RADV); on an
  Intel GPU, or in QEMU, Sola's shell gets no GL context and the desktop
  does not come up.
- To build Sola on the machine: a checkout of Sola's repository. The desk
  profile carries the compilers and libraries the build needs and the
  browser engine (`pkg:cef`), and the desk user, `home`, has `sudo`
  without a password, which `cargo make oath apply` uses.

## Installing

Sola is three Oath packages:

| Package | Contents |
|---------|----------|
| `pkg:sola` | The bus, the call host, the window manager, the shell, the session manager, the lock screen, Terminal, Projects, Paint, Preview, Scope, Monitor, Mail, Calendar, Settings, Bots and its daemon, Oath (the catalog app), the KVM bridge, `solactl`, and the icons and cursors |
| `pkg:sola-browser` | Browser, which also runs [web apps](web-apps.md) |
| `pkg:sola-design` | Design |

Oath's `desk` profile pins each package by hash, together with the hashes
of the packages it needs, so a machine installed from a desk image has
Sola with nothing more to do. Oath's own manual covers building a desk
image (`cargo make build --profile desk` in Oath's repository) and
installing it. Each package's programs are linked into `/bin`;
`oath get pkg:sola` shows the pin and what is live.

`oath update` moves these three packages to the pins of the output it
takes while the machine still has the pins its current output set (an
output lists them; older ones did not, and an update from one of those
leaves them). Once you move a pin yourself, with `cargo make oath apply`
or `oath set`, the machine keeps it, and an update moves only its needs.
Take an update that moves Sola with `--reboot`: an update without it
leaves the desk's services running their old programs (see
[Updates](process-manager.md#updates)). To take a published Sola release
on a machine that keeps its own pins, set each pin to the entry the
release's publish printed, which is the JSON `oath set --from-json`
takes, and apply the three together:

```bash
sudo oath set pkg:sola --from-json '{"present": true, "hash": "sha256-…", "needs": […]}'
sudo oath set pkg:sola-browser --from-json '{"present": true, "hash": "sha256-…", "needs": […]}'
sudo oath set pkg:sola-design --from-json '{"present": true, "hash": "sha256-…", "needs": […]}'
sudo oath apply pkg:sola pkg:sola-browser pkg:sola-design
```

Apply fetches a pinned package the store lacks from the machine's origins.
A release is built against particular packages (Mesa, Wayland, CEF, and
others), and its `needs` name their hashes; apply refuses it on a machine
that has any of those at another hash. Build Sola on that machine instead.

## Building from source

On the Oath machine, as `home`:

```bash
git clone --branch next https://github.com/moonlight-pm/Sola
cd Sola
cargo make oath stage
cargo make oath apply     # only with the machine owner's go-ahead
```

| Command | What it does | What it changes |
|---------|--------------|-----------------|
| `cargo make oath stage` | Builds every program in the three packages, optimized (`--debug` for an unoptimized build), against this machine's packages and `pkg:cef`. Lays out each package under `target/oath/<name>/`: `bin/`, `share/` for `pkg:sola`, a `cef` link to `pkg:cef` for Browser and Design, and `components.json` with the version | The checkout. If `~/.cache/sola/cef-<version>` holds no engine, it becomes a link to `pkg:cef`'s. |
| `cargo make oath apply` | Stages, then puts each tree in the store (`sudo oath store`), pins it (`sudo oath set pkg:<name> hash=… needs=…`, each need at the hash this machine has live, or the one just stored), and applies the three in one `sudo oath apply` | The machine: one apply and one generation. The `/bin` links move, and PID 1 restarts every service whose program moved ([Updating and rolling back](#updating-and-rolling-back)). |
| `cargo make oath publish` | From a committed tree only. Stores the source (`pkg:sola-src`) and every crate the lock file names (`pkg:sola-crates`); files `plan:sola-build`, which compiles every program once in `oath build`'s sandbox against this machine's live packages, and a plan per package that copies its programs out; builds them; uploads the packages, plans, and sources to Oath's default origin; and prints the desk profile entries that pin the products. `--local` builds the plans and uploads nothing | This machine's store and its `plan:*` objects, and the origin, which anyone can read. This machine's own `pkg:sola*` pins do not change. |

`apply` and `publish` need the machine owner's go-ahead: `apply` changes
what the machine runs, and `publish` puts out a release that other
machines install. `publish` reads the origin's write key from
`OATH_ORIGIN_S3`, `OATH_ORIGIN_KEY_ID`, and `OATH_ORIGIN_SECRET`, in the
environment or in `~/.config/oath/origin.env`. A package's version is the
workspace version and the short commit (`0.1.0+g<commit>`), with `.dirty`
added when the checkout has uncommitted changes.

On Oath, `cargo make install` and `cargo make install-cef` are not used:
`install` copies binaries into `/opt/sola/bin`, which nothing on Oath runs,
and the engine comes from `pkg:cef`.

## Starting a session

There is nothing to start by hand. At boot, PID 1 starts seatd, rivulet,
and Sola's services as `home`, and the screen shows Sola once rivulet and
the shell are up. The desktop comes up signed in, with no password, until
you set one in Settings → Login and turn **Sign in automatically** off;
Super+L locks it. Reach the machine otherwise over SSH as `home`, with a
key. [Security](security.md) lists every way into a running desk
and how to close each one, and [Process manager](process-manager.md)
lists the services and how PID 1 runs them.

Applications open from the launcher (Super+Space) or with
`solactl session launch <app_id>`. The session manager reopens the applications
that were open when the session last ended.

To end the session, choose **Restart Computer** or **Shut Down** from the
flower menu. **Exit Sola** closes the applications and hands the screen to
Oath's console, signed in or asking for the password as Settings → Login
says; `oath desk` brings the desk back
([Ending the session](process-manager.md#ending-the-session)).

## Updating and rolling back

A change to Sola is an apply of its packages: `cargo make oath apply`
from a checkout, or `oath set` and `oath apply` of a published release
([Installing](#installing)). PID 1 restarts each service whose program
comes from a package that moved, and no other: the Sola services restart,
while rivulet and the tmux servers that hold Terminal's and Projects'
shells keep running. The restarted session manager reopens the
applications that were open, and Terminal and Projects reattach to their
tmux sessions. No session needs ending. An application does not restart
itself when its package moves.

Every apply takes a generation. `sudo oath undo` puts back the pins the
last apply changed and relinks `/bin`, and PID 1 restarts the same
services on the previous programs; run it again to go back one more
apply. `oath log` lists the applies. Undo puts back only the pins: your
files and settings stay as they are.

## Fonts

Sola ships no fonts. `pkg:sola` needs Oath's font packages, Noto (with
CJK and colour emoji) and Iosevka Term Slab, so they are present wherever
Sola is. Interface text is Noto Sans and mono text Iosevka Term Slab;
when **SF Pro Text** is installed, interface text uses it.
[Fonts](distribution.md) covers how a family is chosen and how to add one,
and [Theme and toolkit](theme.md) covers font roles.

## Where things are

On Oath, `~` is `/home`, the desk user's home directory.

| Path | What |
|------|------|
| `/bin/sola-*`, `/bin/solactl` | Links to Sola's programs in the store (`/oath/store/pkg/sola/<hash>/bin/`, and the Browser and Design packages) |
| `/oath/store/pkg/sola/share` | Icons and the McMojave cursors |
| `~/.config/sola/` | Applications' settings, and the bus's saved state, `state.yaml` |
| `~/.local/state/sola/` | Sola's logs: `sola.log` for every Sola process, and `app-<app id>.log` for each application |
| `/oath/log/svc/<service>.log` | Each service's output, as PID 1 records it (`oath logs svc:<service>`) |
| `/run/user/1/` | The bus and call sockets (`sola-bus`, `sola-call`) and rivulet's `wayland-N` |

`solactl logs <name>` prints `~/.local/state/sola/<name>.log`; add `-f` to
keep following it (`solactl logs app-sola-mail -f`).
`oath logs svc:rivulet` shows the compositor's output. [Logs](process-manager.md#logs)
covers the log filter.

## First-run problems

| Symptom | Cause |
|---------|-------|
| No top row or bar after boot, on an Intel GPU or in QEMU | Oath's Mesa drives AMD GPUs only, so the shell gets no GL context. |
| `sola: not on Oath; PID 1 runs Sola's services (oath ls --kind svc)` | `sola` was run on Oath, where PID 1 runs the session. Nothing needs starting. |
| `cargo make oath: not an Oath machine (no /oath/INDEX.md)` | `cargo make oath` runs only on an Oath machine. |
| `commit first: a published source is the committed tree` | `cargo make oath publish` was run with uncommitted changes. |
| `oath apply` refuses a published release, naming a need | The release was built against a package the machine has at another hash. Build Sola on the machine. |
| A new build does not run | `cargo make install` copied it into `/opt/sola/bin`, which nothing on Oath runs. Use `cargo make oath apply`. |
| Bots and `solactl bots` cannot reach the bots daemon | The desk profile runs no service for `sola-botsd`. |
| No desktop and no console after **Exit Sola** | Oath has no login service (`svc:login`, R42), so nothing takes the screen, and the shell and the session manager stay stopped after a clean exit. Restart the machine. |
| `~/.local/state/sola/sola.log` keeps growing | Only `sola` rotates it, and `sola` does not run on Oath. |
