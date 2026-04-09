# Sola Distribution

Inventory of system dependencies and resources Sola requires at runtime. This will inform packaging when we build a custom Arch-based distro.

## System Libraries

Required by the Smithay compositor backend:

| Library | Arch Package | Purpose |
|---------|-------------|---------|
| libdrm | `libdrm` | DRM/KMS kernel interface |
| libgbm | `mesa` | GPU buffer allocation |
| libEGL, libGLESv2 | `mesa` | OpenGL ES rendering |
| libinput | `libinput` | Input device handling |
| libudev | `systemd-libs` | Device enumeration |
| libseat | `seatd` | Session/seat management |
| libxkbcommon | `libxkbcommon` | Keyboard layout handling |

## Services

| Service | Package | Purpose |
|---------|---------|---------|
| seatd | `seatd` | Grants DRM/input device access without root |

The user must be in the `seat` group.

## Cursor Themes

Sola loads cursors from the system xcursor theme at runtime.

| Theme | Package | Notes |
|-------|---------|-------|
| Adwaita | `adwaita-icon-theme` | Default fallback, always required |

Cursor files live at `/usr/share/icons/<theme>/cursors/`. Sola currently loads only the "default" cursor from Adwaita. Future: full cursor shape support (text, pointer, grab, resize, etc.).

## Fonts

None yet. Will be needed when we render shell chrome (WebKit handles its own fonts).

## Binaries

| Binary | Install path | Source |
|--------|-------------|--------|
| sola | `/opt/sola/bin/sola` | `crates/sola/` |

## Runtime Directories

| Path | Purpose |
|------|---------|
| `/opt/sola/bin/` | Compositor binary |
| `/opt/sola/log/` | Persistent log files |
| `$XDG_RUNTIME_DIR/wayland-*` | Wayland client socket (auto-created) |

## Future Additions

- WebKit6 / libwebkitgtk-6.0 — for shell UI and app rendering
- App binaries (terminal, file manager, etc.)
- Shell frontend assets (HTML/CSS/JS)
- Custom cursor theme (if we don't use Adwaita)
- Wallpaper/theme assets
- Session/login configuration (greetd or getty auto-login)
