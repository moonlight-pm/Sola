# sola-make

**Crate:** `crates/sola-make/`
**Binary:** `sola-make` (invoked via `cargo make`)
**Role:** Build and install orchestration using the xtask pattern.

## Usage

```bash
cargo make build                    # Build everything (debug)
cargo make build <target>           # Build a specific crate
cargo make install                  # Build + install all to /opt/sola/bin
cargo make install <app>            # Build + install a single app
cargo make install <app> --watch    # Watch + reinstall on change
```

Alias configured in `.cargo/config.toml`:
```toml
[alias]
make = "run -q -p sola-make --"
```

## Install

`cargo make install`:
1. Builds the entire workspace in debug mode
2. Creates `/opt/sola/bin/` and `/opt/sola/log/` locally
3. Auto-discovers all workspace binaries (scans `crates/` for `src/main.rs`)
4. Copies each binary to `/opt/sola/bin/`
5. Skips `sola-make` itself

Apps come into the workspace as `crates/sola-<name>/` once they're
rewritten against the new bus (monitor first, then browser, mail,
etc.). The scanner picks them up automatically — no install-list
maintenance needed.

## Isolated / experimental crates

A crate can live under `crates/` but be deliberately kept out of the
Cargo workspace to avoid feature-unification bleed (e.g. iced pulling
in a wayland-sys feature that breaks sola-river). To add one:

1. Drop the crate at `crates/<name>/`.
2. Add it to the workspace's `exclude` list in the root `Cargo.toml`:
   ```toml
   [workspace]
   members = ["crates/*"]
   exclude = ["crates/<name>"]
   ```
3. That's it — no further sola-make changes needed.

`cargo make build` and `cargo make install` discover the exclude list
automatically, build each isolated crate with `cargo build
--manifest-path crates/<name>/Cargo.toml` (its own target dir, its own
feature graph), then install the resulting binary to
`/opt/sola/bin/<name>` alongside workspace binaries. Targeted builds
(`cargo make build sola-shell`) and targeted installs (`cargo make
install <name>`) skip the isolated loop and stay focused.

Convention: isolate only because the crate needs a separate feature
graph. Anything that can safely live in the workspace should.

## Source Files

| File | Purpose |
|---|---|
| `src/main.rs` | CLI parsing (clap), command dispatch |
| `src/build.rs` | Cargo build invocation |
| `src/install.rs` | Binary discovery, local install logic |
| `src/isolated.rs` | Out-of-workspace crate discovery + build/install |
| `src/watch.rs` | File watching + auto-reinstall |
