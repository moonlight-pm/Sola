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

Sola is a set of Oath packages: `pkg:sola` is the desk core (the bus, the
call host, the window manager, the shell, the session manager, the lock
screen, the KVM bridge, `solactl`, and the icons and cursors), and each
application has its own, `pkg:sola-<app>` (Terminal, Projects, Browser,
Design, Oath, …). [Architecture](architecture.md#packages) lists them.

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
| `cargo make oath apply` | Stages, then puts each tree in the store (`sudo oath store`), pins it (`sudo oath set pkg:<name> hash=… needs=…`, each need at the hash this machine has live, or the one just stored), and applies the ones that changed in one `sudo oath apply`. Name packages (`cargo make oath apply sola`) to build and move only those | The machine: one apply and one generation. The `/bin` links move, and PID 1 restarts every service whose program moved ([Updating and rolling back](#updating-and-rolling-back)). |
| `cargo make oath publish` | From a committed tree only. Stores what a build reads of the source (`pkg:sola-src`) and of every crate the lock file names (`pkg:sola-crates`); files `plan:sola-build`, which compiles every program once in `oath build`'s sandbox against this machine's live packages, and a plan per package that copies its programs out; builds them; uploads the plans and sources to Oath's default origin (not `pkg:sola-build` itself: a rebuild runs its plan); pins and applies the products here as `apply` does, refusing as it does; then runs `oath publish --confirm pkg:<name> --committed <commit time>` for each, which uploads the package if the origin lacks it and its head, `pkg/<name>.json`, signed with `~/.config/oath/sign_ed25519`. A package whose head is that hash already is left alone. Only from a clean `master` that is `origin/master`. `--local` builds the plans and uploads, applies and publishes nothing | This machine: one apply, as `apply` makes, so it runs what it publishes. The origin, which anyone can read. Every machine whose `host:local.publishers` trusts this machine's key for `sola,sola-*` takes the new heads at its next `oath update`. |

`apply` and `publish` need the machine owner's go-ahead: `apply` changes
what the machine runs, and `publish` changes it too and puts out heads
that other machines install. `publish` reads the origin's write key from
`OATH_ORIGIN_S3`, `OATH_ORIGIN_KEY_ID`, and `OATH_ORIGIN_SECRET`, in the
environment or in `~/.config/oath/origin.env`, and signs with
`~/.config/oath/sign_ed25519` (or `OATH_SIGN_KEY`). That key signs heads
of Sola's packages only where a person added its public half to a
machine's `host:local.publishers` as `names="sola,sola-*" ssh-ed25519 …`.
It needs an `oath` with `publish` (Oath R72, desk release bda181db1fc6).
If it stops after the apply, running it again finishes: the builds are
cached, nothing moves again, and heads already out are left alone. A
package's version is the crate's version, never the commit: where a
package came from is its `source` (`oath get pkg:<name>`).

A new package, or a new or changed service, still needs Oath's desk
profile: a machine reads heads only for the packages it has.

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
`solactl desk launch <app_id>`. The session manager reopens the applications
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
shells keep running, and so do the open applications (the session
manager keeps its children). No session needs ending. An application
whose own package moved is stopped and started again on its new program by
the session manager, alone, once the apply has said so; one that isn't Sola's (Steam,
a game) is never restarted for it.

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
| `this machine's oath has no publish` | `cargo make oath publish` needs Oath R72 (desk release bda181db1fc6) on the machine. |
| `no signing key at …` | `cargo make oath publish` signs heads with `~/.config/oath/sign_ed25519`. Make one with the `ssh-keygen` line in the message. |
| `oath apply` refuses a published release, naming a need | The release was built against a package the machine has at another hash. Build Sola on the machine. |
| A new build does not run | `cargo make install` copied it into `/opt/sola/bin`, which nothing on Oath runs. Use `cargo make oath apply`. |
| Bots and `solactl bots` cannot reach the bots daemon | The desk profile runs no service for `sola-botsd`. |
| No desktop and no console after **Exit Sola** | Oath has no login service (`svc:login`, R42), so nothing takes the screen, and the shell and the session manager stay stopped after a clean exit. Restart the machine. |
| `~/.local/state/sola/sola.log` keeps growing | Only `sola` rotates it, and `sola` does not run on Oath. |
