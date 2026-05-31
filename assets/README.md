# Assets

Source assets that ship to `/opt/sola/share/` via `cargo make assets sync`.

Most asset packs (Lucide / Simple Icons, McMojave cursors, the open-source
fonts Inter / JetBrains Mono / Roboto Flex / Roboto Condensed / Iosevka Term
Slab) are **pulled from upstream** on demand — they are pinned in
`crates/sola-assets/upstream.toml` and never live in this directory.

This directory is the staging area for assets that the sync can **not** fetch:
license-restricted files the developer must supply locally. Redistributable
assets may also be committed here in the future; only the files listed in
`fonts/.gitignore` are excluded from version control.

## Developer-supplied fonts (not in the repo)

These are **not redistributed** in this repo and must be provided by you. Drop
the `.ttf` files into `assets/fonts/`, then install them to the runtime font
directory:

### SF Pro / SF Compact — Apple system fonts (Apple license)

The kit's default UI font. Obtain from Apple
(<https://developer.apple.com/fonts/>) and extract the TTFs.

```
assets/fonts/SF-Pro.ttf
assets/fonts/SF-Pro-Italic.ttf
assets/fonts/SF-Compact.ttf
assets/fonts/SF-Compact-Italic.ttf
```

Install:

```sh
mkdir -p /opt/sola/share/fonts/SFPro
cp assets/fonts/SF-Pro.ttf assets/fonts/SF-Pro-Italic.ttf \
   assets/fonts/SF-Compact.ttf assets/fonts/SF-Compact-Italic.ttf \
   /opt/sola/share/fonts/SFPro/
```

### Iosevka Fixed — optional mono UI option

The kit offers "Iosevka Fixed" as a per-role mono font option. Iosevka is SIL
OFL (redistributable), but the kit consumes the `.ttf` build directly rather
than the WOFF2 the terminal ships, so it is staged here manually. Get the
`Iosevka Fixed` TTFs from <https://github.com/be5invis/Iosevka/releases>.

```
assets/fonts/Iosevka-Fixed.ttf
assets/fonts/Iosevka-Fixed-Bold.ttf
```

Install:

```sh
mkdir -p /opt/sola/share/fonts/Iosevka
cp assets/fonts/Iosevka-Fixed.ttf assets/fonts/Iosevka-Fixed-Bold.ttf \
   /opt/sola/share/fonts/Iosevka/
```

Missing fonts are non-fatal: the kit logs a warning at startup and falls back
(SF Pro → Inter; Iosevka Fixed is only used if explicitly selected). The font
file list the kit registers lives in `crates/sola-kit/src/fonts.rs`
(`FONT_FILES`); the upstream pins live in `crates/sola-assets/upstream.toml`.
