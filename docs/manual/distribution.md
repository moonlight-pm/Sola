# Distribution: fonts are a system concern

Sola does **not** bundle, register, or ship font files. Every Sola app
resolves fonts **by family name** through the system fontconfig database
(loaded into iced's font db at startup by
`sola_kit::fonts::ensure_system_fonts`). If a family the app asks for is not
installed system-wide, the text shaper silently falls back to a default
face — so the required families must be present **before** Sola runs.

## Required families

| Family            | Role        | License     |
| ----------------- | ----------- | ----------- |
| **Inter**         | All UI text | SIL OFL 1.1 |
| **JetBrains Mono**| Mono / code | SIL OFL 1.1 |

These are the defaults every Sola app and the bus theme seed reach for. If
either is missing, UI/mono text falls back to whatever fontconfig picks,
which usually looks wrong.

## Optional families

The font picker (`sola-settings` / the `sola-kit` storybook) offers any
family fontconfig finds installed, in addition to the two defaults above.
Common extra choices:

- **Iosevka Term Slab** (or any installed monospace) — selectable as the
  mono role if you prefer it over JetBrains Mono.

A family you select in the picker must be installed system-wide; picking one
that isn't installed falls back to a default face. **Install the family
first, then select it.** The family *name* you install must match the name
the picker shows (fontconfig's family name).

## Installing the fonts

### NixOS

```nix
fonts.packages = with pkgs; [
  inter
  jetbrains-mono
  # optional extras the picker can offer:
  nerd-fonts.iosevka-term-slab
];
```

Rebuild (`nixos-rebuild switch`) so fontconfig picks them up.

### Other distributions

Install via your distro's package manager (the family *names* must match
what the picker offers — `Inter`, `JetBrains Mono`):

```sh
# Debian/Ubuntu
sudo apt install fonts-inter fonts-jetbrains-mono

# Arch
sudo pacman -S inter-font ttf-jetbrains-mono

# Fedora
sudo dnf install rsms-inter-fonts jetbrains-mono-fonts
```

Or drop the `.ttf`/`.otf` files into a fontconfig-scanned directory
(`~/.local/share/fonts/` or `/usr/share/fonts/`) and run `fc-cache -f`.
Confirm a family is visible with:

```sh
fc-list | grep -i inter
fc-list | grep -i "jetbrains mono"
```
