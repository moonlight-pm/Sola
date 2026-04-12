# sola-make

**Crate:** `crates/sola-make/`
**Binary:** `sola-make` (invoked via `cargo make`)
**Role:** Build and deploy orchestration using the xtask pattern.

## Usage

```bash
cargo make build              # Build everything
cargo make build <target>     # Build a specific crate
cargo make deploy canto       # Build release + rsync to canto
```

Alias configured in `.cargo/config.toml`:
```toml
[alias]
make = "run -q -p sola-make --"
```

## Deploy

`cargo make deploy canto`:
1. Builds entire workspace in release mode
2. Creates `/opt/sola/bin/` and `/opt/sola/log/` on canto via SSH
3. Auto-discovers all workspace binaries (scans `crates/` and `apps/` for `src/main.rs`)
4. Rsyncs each binary to `canto:/opt/sola/bin/`
5. Skips `sola-make` itself

## Source

Single file: `src/main.rs` — CLI parsing (clap), build args, deploy logic, binary discovery.
