# Assets

Source assets that ship to `/opt/sola/share/` via `cargo make assets sync`.

The asset packs (Lucide / Simple Icons, McMojave cursors) are **pulled from
upstream** on demand — they are pinned in `crates/sola-assets/upstream.toml`
and never live in this directory.

## Fonts are a system concern (not bundled)

Sola no longer ships or registers font files. Fonts are resolved by family
name through the **system fontconfig database**. The two families Sola
defaults to — **Inter** (UI) and **JetBrains Mono** (mono) — must be installed
system-wide. The font picker also offers any other family fontconfig knows
about. See `docs/manual/distribution.md` for install instructions.
