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

`apps/*` is temporarily outside the workspace while each app is
rewritten against the new bus; once re-added, the same scanner
picks them up automatically.

## Source Files

| File | Purpose |
|---|---|
| `src/main.rs` | CLI parsing (clap), command dispatch |
| `src/build.rs` | Cargo build invocation |
| `src/install.rs` | Binary discovery, local install logic |
| `src/watch.rs` | File watching + auto-reinstall |
