# Fonts

Sola does not bundle, register, or ship font files. Every Sola app resolves
fonts by family name through the system fontconfig database, which
`sola_kit::fonts::ensure_system_fonts` loads into iced's font database at
startup. If a family an app asks for is not installed system-wide, the text
shaper silently falls back to a default face, so the required families must
be installed before Sola runs.

## Required families

| Family | Role | License |
|--------|------|---------|
| **Inter** | UI text | SIL OFL 1.1 |
| **JetBrains Mono** | Mono and code | SIL OFL 1.1 |

Sola uses these when the preferred families below are not installed. If
either is missing too, UI or mono text falls back to whatever fontconfig
picks, which usually looks wrong.

## Preferred families

When they are installed, Sola uses **SF Pro Text** for UI text and
**Iosevka Term Slab** for mono; the default theme names them. Sola does not
ship either. SF Pro Text is a licensed face and is not in the Sola NixOS
module: install it system-wide yourself from a copy you are entitled to
use.

## Other families

The font picker in Kit (the `sola-kit` storybook) offers any family
fontconfig finds installed, in addition to the families above.

A family you select in the picker must be installed system-wide. Picking
one that is not installed falls back to the default face for that role.
Install the family first, then select it. The family name you install must
match the name the picker shows (fontconfig's family name).

## Installing the fonts

### NixOS

`services.sola.enable = true` installs **Inter** and **JetBrains Mono**
through the Sola NixOS module. Add other families yourself if you want them
in the picker:

```nix
fonts.packages = with pkgs; [
  # optional extras the picker can offer:
  nerd-fonts.iosevka-term-slab
];
```

Rebuild (`nixos-rebuild switch`) so fontconfig picks them up. Confirm with
`fc-list : family | grep -E 'Inter|JetBrains Mono'`.

### Font files

Licensed families such as SF Pro are not in nixpkgs. Put their `.ttf` or
`.otf` files in `~/.local/share/fonts/` and run `fc-cache -f`. Confirm a
family is visible with:

```bash
fc-list : family | grep -E 'SF Pro Text|Iosevka Term Slab'
```
