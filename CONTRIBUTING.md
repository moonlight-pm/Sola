# Contributing to Sola

This is the dev-setup guide for working on Sola from a NixOS box. If
all you want is the prebuilt binaries, see `INSTALL.md` — this doc is
for people who want to compile from source and iterate.

The Sola binaries hardcode several `/opt/sola/*` paths (`/opt/sola/bin`,
`/opt/sola/share`, `/opt/sola/log`, the per-app CEF cache under
`~/.cache/sola/cef-…`). The dev flow leans into that: `cargo make
install` populates `/opt/sola/bin` and `/opt/sola/share` directly,
mirroring what the released binaries expect.

## Prerequisites

- **NixOS.** Tested on `nixos-unstable` against the pinned nixpkgs in
  `flake.lock`. Should also work on `nixos-25.05`+.
- **Working GPU stack** — Mesa-only (Intel/AMD) or NVIDIA proprietary.
- **A user with sudo** — `cargo make install` runs `sudo cp` to write
  binaries to `/opt/sola/bin/`.
- **Read access to the repo** — your SSH key registered with GitHub
  and your account added as a collaborator on `moonlight-pm/Sola`.

## 1. NixOS configuration

The easiest path is to import the same `nixosModules.default` that
release-installs use. It gives you everything Sola needs at runtime
(patched River, nix-ld libraries for CEF's transitive deps, GStreamer
plugins for the legacy WebKit stack, env vars, `hardware.graphics`),
plus the FHS shim for `/opt/sola/`.

In your `/etc/nixos/flake.nix`:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    sola.url = "git+ssh://git@github.com/moonlight-pm/Sola";
  };

  outputs = { self, nixpkgs, sola, ... }: {
    nixosConfigurations.<your-host> = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        sola.nixosModules.default
        { services.sola.enable = true; }
      ];
    };
  };
}
```

> **Do not add `inputs.sola.inputs.nixpkgs.follows = "nixpkgs"`** — Sola
> pins its own nixpkgs to a revision where `pkgs.river` (0.4.5, the
> zig rewrite) and our carried Xwayland-destroy-state patch are known
> to build cleanly. See `INSTALL.md`'s warning for the longer story.

Then add Rust + a few dev tools to your `configuration.nix`:

```nix
environment.systemPackages = with pkgs; [
  rustup
  pkg-config
  gcc
  git
  zstd
];
```

`nixos-rebuild switch --flake /etc/nixos`, then:

```sh
rustup default stable           # one-time
rustup component add rust-src   # for rust-analyzer goto-definition into stdlib
```

## 2. Reclaim `/opt/sola/` from the module

The module installs the *released* binaries and symlinks
`/opt/sola/bin` → `${package}/bin`, `/opt/sola/share` →
`${package}/share` so a Nix-only install works out of the box. For
dev, you want your own `cargo make install` builds at those paths, so
remove the symlinks first:

```sh
sudo rm -f /opt/sola/bin /opt/sola/share
```

The activation script intentionally refuses to clobber a real
directory at either path, so once you've populated `/opt/sola/bin`
and `/opt/sola/share` via `cargo make install` (next step), future
`nixos-rebuild` runs will leave your dev installs alone.

If you ever want to switch back to the released binaries, delete the
real directories and re-run `nixos-rebuild switch` — the activation
will recreate the symlinks.

## 3. First-time build

```sh
git clone git@github.com:moonlight-pm/Sola
cd Sola
cargo make install-cef    # ~/.cache/sola/cef-<version>/ — ~1.5GB download, once
cargo make build          # full debug build of the workspace — ~10 minutes first time
cargo make install        # copies binaries to /opt/sola/bin (sudo)
                          # also: `cargo make assets pull` runs automatically if
                          # /opt/sola/share/ is missing or older than a week
```

`cargo make install-cef` populates `~/.cache/sola/cef-<version>/Release/`
with the patched libcef.so (the `patchelf` step that points its
`DT_RUNPATH` at the nix-ld dispatch dir runs automatically). The CEF
tarball is ~500MB compressed; this is the slow step on a fresh box.

`cargo make build` defaults to debug. For release-mode builds (much
smaller, more optimized, slower to compile), pass `--release`:

```sh
cargo make build --release
```

## 4. Run it

Sola is a full desktop session — it owns the display and input. **Do
not run it from inside another desktop session.**

1. Log out of any graphical session.
2. Switch to a TTY (`Ctrl+Alt+F2` or similar).
3. Log in.
4. Run from the repo or from anywhere — your installed binary is at
   `/opt/sola/bin/sola`:
   ```sh
   /opt/sola/bin/sola
   ```
   Or with debug logging mirrored to a file:
   ```sh
   RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
   ```
5. Quit via the menubar's "Quit Sola" or `pkill sola` from another TTY.

## 5. Day-to-day workflow

```sh
cargo make build                       # full workspace
cargo make build <crate>               # one crate (e.g. `cargo make build shell`)
cargo make install                     # install all rebuilt binaries
cargo make install <app>               # install one
cargo make install <app> --watch       # watch + reinstall on change (frontend dev)
```

The `install` step `sudo cp`s into `/opt/sola/bin`. Sola's parent
process watches `/opt/sola/bin/` and restarts any child whose binary
changes — so for most apps you don't have to manually re-launch them.

For sola's core (the process manager itself, the shell, the
compositor bridge), restart by killing sola and re-launching from
the TTY.

## 6. Repository layout

See `CLAUDE.md` at the repo root for the canonical workspace structure
and project conventions. The headline pieces:

- `crates/sola/` — process manager (the binary you actually launch).
- `crates/sola-bus/` — the IPC bus everything talks over.
- `crates/sola-shell/` — menubar, launcher, switcher, zoning.
- `crates/sola-river/` — bridges River (the underlying Wayland compositor)
  events onto the bus.
- `crates/sola-kit/` — CEF/Remix v3 app framework (current).
- `crates/sola-app/` — GTK4/WebKit app framework (legacy, being ported away).
- `crates/sola-*` — individual apps (settings, monitor, browser, terminal, mail).
- `crates/sola-make/` — the `cargo make ...` build system.
- `crates/sola-assets/` — third-party asset pulls (icons, fonts, cursors).
- `docs/manual/` — long-form architecture & reference docs.
- `docs/specs/` — design docs and implementation plans (date-prefixed).
- `docs/vault/` — Obsidian vault; the canonical one to skim is
  `Distribution.md`.

## 7. Debugging

- Logs at `/opt/sola/log/<process>.log` plus `/opt/sola/log/sola.log`
  (aggregate). `tail -F /opt/sola/log/sola.log` from a second TTY
  while iterating.
- `RUST_LOG` accepts the standard env-filter syntax:
  `RUST_LOG=info,sola_kit=trace,cef=warn`.
- `solactl apps` lists running apps + window IDs.
- `solactl logs <app>` tails one app's log.
- `solactl eval <app> '<js-expression>'` runs JS inside a CEF app's
  WebView and prints the JSON result — invaluable for diagnosing
  Remix v3 state.
- `solactl emit <Topic> '<json-payload>'` injects bus events from
  the command line.
- For River-side issues, look at `/opt/sola/log/river.log`.

If a CEF subprocess GPU init fails (`Unable to initialize SkSurface`),
check `__EGL_VENDOR_LIBRARY_DIRS` and `VK_ICD_FILENAMES` —
`docs/vault/Distribution.md` has the deep dive.

## 8. Commit conventions

Match the existing log style — `git log --oneline -20` is the
reference. Common prefixes: `feat`, `fix`, `refactor`, `docs`,
`test`, `chore`. Subject in imperative present (`add foo`, `fix
bar`), body explains the *why* and any non-obvious gotchas.

Commits include a `Co-Authored-By` trailer when AI tools are part
of the loop; otherwise just normal commits.

## 9. Cutting a release (maintainers)

`cargo make publish` bundles `/opt/sola/bin` + the CEF Release tree
+ `/opt/sola/share`, pre-patches RUNPATHs for the consumer side,
zstd-compresses, computes the SRI hash, rewrites `nix/release.nix`,
commits, tags, pushes, and runs `gh release create`. See INSTALL.md's
"For maintainers" section for the full pipeline.

## 10. Getting help

- File issues at `https://github.com/moonlight-pm/Sola/issues`.
- Architecture and "why does it work this way" questions: look in
  `docs/vault/` and `docs/specs/` first — the design docs are
  fairly comprehensive.
