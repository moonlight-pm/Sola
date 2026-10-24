# Introduction

Sola is a desktop environment for Linux on Wayland, written in Rust. A
session is a set of separate processes under a process manager: an event
bus, a request/reply host, a bridge to the River compositor, the shell
(menubar, launcher, window switcher), and applications that share one
toolkit, sola-kit, and one theme.

This manual describes Sola as it works today. Sola is pre-release: there
are no releases or install images, and you build it from source. What each
part can do, and what it lacks, is listed in
[docs/capabilities.md](../capabilities.md).

## Requirements

- NixOS on x86-64 (`nixos-unstable`, or 25.05 and later). Nix on another
  distribution is not supported: the binaries expect NixOS library paths.
- An NVIDIA or Mesa (Intel, AMD) graphics stack. Sola points Vulkan at the
  NVIDIA driver when `VK_ICD_FILENAMES` is unset; on Mesa, set it to your
  driver's file in `/run/opengl-driver/share/vulkan/icd.d/` before starting
  `sola`.
- Rust 1.85 or later, and `sudo`.
- The font families in [Fonts](#fonts), installed system-wide.

## Setting up the host

Sola's NixOS module prepares the machine: a patched River 0.4.5, nix-ld for
the browser engine's libraries, the graphics environment, fonts, and the
udev rules KVM needs. Add the flake and enable the module, leaving
`/opt/sola` to your own builds:

```nix
inputs.sola.url = "git+https://github.com/moonlight-pm/Sola";

# in the host's modules:
sola.nixosModules.default
{
  services.sola.enable = true;
  services.sola.installRelease = false;   # cargo make install owns /opt/sola
}
```

Do not set `inputs.sola.inputs.nixpkgs.follows`. Sola pins its own nixpkgs,
where River 0.4.5 and its patches build. [CONTRIBUTING.md](../../CONTRIBUTING.md)
lists the compile tools to add to `configuration.nix` and the checks to run
after `nixos-rebuild switch`.

## Building and installing

```bash
git clone https://github.com/moonlight-pm/Sola
cd Sola
cargo make install-cef    # the browser engine, once (about 1.5 GB)
cargo make install        # build everything, copy it to /opt/sola/bin
```

`cargo make install-cef` is required: Browser and Design do not build
without it. It downloads Chromium Embedded Framework into
`~/.cache/sola/`. That build of the engine has no H.264 or AAC;
`scripts/cef-codecs/` builds one that has them, which takes hours.

Builds are optimized by default. `cargo make install shell` builds and
installs one component; `--debug` builds without optimization. The first
install also fetches the icon and cursor packs into `/opt/sola/share`.

## Fonts

Sola does not ship fonts. Applications find font families by name through
fontconfig, so a family must be installed system-wide before Sola runs. The
NixOS module installs **Inter** and **JetBrains Mono**, which Sola uses for
interface and mono text. When **SF Pro Text** and **Iosevka Term Slab** are
installed, Sola prefers them. SF Pro is licensed and not in nixpkgs: put its
`.ttf` or `.otf` files in `~/.local/share/fonts/` and run `fc-cache -f`.

```bash
fc-list : family | grep -E 'Inter|JetBrains Mono|SF Pro Text|Iosevka Term Slab'
```

[Theme and toolkit](theme.md) covers font roles and how a family is chosen
and changed.

## Starting a session

Sola owns the display and input, so it starts from a text console, not
inside another desktop.

1. Log out of any graphical session and switch to a console
   (`Ctrl+Alt+F2`).
2. Log in and run `/opt/sola/bin/sola`.
3. To leave, choose **Quit Sola** from the menubar, or run
   `pkill -x sola` from another console.

## Updating

Run `cargo make install` again. The process manager restarts a component
when its binary in `/opt/sola/bin` changes, and applications restart
themselves, so most updates need no new session. Replacing `sola` itself
ends the session.

## Where things are

| Path | What |
|------|------|
| `/opt/sola/bin` | Every Sola binary |
| `/opt/sola/share` | Icons, cursors, and other assets |
| `/opt/sola/log` | Logs: `sola.log` for the session, `river.log`, and `app-<app id>.log` for each application |
| `~/.cache/sola/cef-<version>/` | The browser engine |

`solactl logs <name>` prints `/opt/sola/log/<name>.log`; add `-f` to keep
following it (`solactl logs app-sola-mail -f`). For more detail, start the
session with `RUST_LOG=debug /opt/sola/bin/sola`.

## First-run problems

| Symptom | Cause |
|---------|-------|
| `CEF binary distribution not found` while building | `cargo make install-cef` has not been run. |
| `river not found in PATH` | The module is not enabled, or the rebuild was not switched to. |
| `'river' has been renamed to/replaced by 'river-classic'` | `inputs.sola.inputs.nixpkgs.follows` is set. Remove it. |
| pkg-config cannot find `xkbcommon` while building | `environment.extraOutputsToInstall` or `PKG_CONFIG_PATH` is missing; see CONTRIBUTING.md. |
| `sola` exits at once with a Wayland error | It was started inside another desktop session. Use a console. |
| A blank window and `Unable to initialize SkSurface` | The graphics stack or nix-ld is not set up for the browser engine. |
