# Sola Distribution

Inventory of system dependencies and resources Sola requires at runtime.

## Development Platform

Currently developed on NixOS. System packages are managed via `/etc/nixos/configuration.nix` with `environment.systemPackages`. Dev outputs (`.pc` files for pkg-config) are exposed via `environment.extraOutputsToInstall = [ "dev" ]`.

Binary resolution uses `$PATH` lookup (via `which` crate) — no hardcoded paths. This is required for NixOS where binaries live in `/nix/store/`.

## System Libraries

Required by GTK4, WebKit6, and compositor integration:

| Library | NixOS Package | Purpose |
|---------|--------------|---------|
| gtk4 | `gtk4` | GTK4 toolkit |
| webkit6 | `webkitgtk_6_0` | WebKit6 WebViews |
| libdrm | `libdrm` | DRM/KMS kernel interface |
| libgbm | `mesa` | GPU buffer allocation |
| libEGL, libGLESv2 | `mesa` | OpenGL ES rendering |
| libinput | `libinput` | Input device handling |
| libudev | `systemd` | Device enumeration |
| libseat | `seatd` | Session/seat management |
| libxkbcommon | `libxkbcommon` | Keyboard layout handling |
| harfbuzz | `harfbuzz` | Text shaping (transitive dep) |
| libepoxy | `libepoxy` | GL dispatch (transitive dep) |
| vulkan-loader | `vulkan-loader` | Vulkan runtime (transitive dep) |

## External Binaries

| Binary | Purpose | Resolution |
|--------|---------|-----------|
| `river` | Wayland compositor (wlroots) | `$PATH` lookup via `resolve_binary()` |

## Services

| Service | Package | Purpose |
|---------|---------|---------|
| seatd | `seatd` | Grants DRM/input device access without root |

The user must be in the `seat` group.

## Sola Binaries

| Binary | Install path | Source |
|--------|-------------|--------|
| sola | `/opt/sola/bin/sola` | `crates/sola/` |
| sola-bus | `/opt/sola/bin/sola-bus` | `crates/sola-bus/` |
| sola-river | `/opt/sola/bin/sola-river` | `crates/sola-river/` |
| sola-shell | `/opt/sola/bin/sola-shell` | `crates/sola-shell/` |
| sola-session | `/opt/sola/bin/sola-session` | `crates/sola-session/` |
| sola-browser | `/opt/sola/bin/sola-browser` | `apps/browser/` |
| sola-mail | `/opt/sola/bin/sola-mail` | `apps/mail/` |
| sola-terminal | `/opt/sola/bin/sola-terminal` | `apps/terminal/` |
| sola-settings | `/opt/sola/bin/sola-settings` | `apps/settings/` |
| sola-monitor | `/opt/sola/bin/sola-monitor` | `apps/monitor/` |
| sola-agent | `/opt/sola/bin/sola-agent` | `apps/agent/` |

## Runtime Directories

| Path | Purpose |
|------|---------|
| `/opt/sola/bin/` | All Sola binaries |
| `/opt/sola/log/` | Persistent log files (`sola.log`, rotated at 100KB) |
| `~/.config/sola/` | User config (`sola.toml`, legacy JSON configs) |
| `$XDG_RUNTIME_DIR/sola-bus` | Bus Unix socket |
| `$XDG_RUNTIME_DIR/sola-wayland` | Published wayland socket name |
| `$XDG_RUNTIME_DIR/sola-display` | Published X11 display name |

## NixOS-Specific Setup

Required in `configuration.nix`:

```nix
# Expose .dev outputs for pkg-config
environment.extraOutputsToInstall = [ "dev" ];

# Set PKG_CONFIG_PATH for building
environment.sessionVariables.PKG_CONFIG_PATH =
  "/run/current-system/sw/lib/pkgconfig:/run/current-system/sw/share/pkgconfig";

# nix-ld for dynamic linking
programs.nix-ld.enable = true;
```

## Cursor Themes

| Theme | Package | Notes |
|-------|---------|-------|
| Adwaita | `adwaita-icon-theme` | Default fallback, always required |

Cursor files live at `/usr/share/icons/<theme>/cursors/` (or nix store equivalent).
