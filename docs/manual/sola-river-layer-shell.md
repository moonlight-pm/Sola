# sola-river layer-shell (KVM infrastructure)

River maps `wlr-layer-shell` surfaces **only** while the window manager
holds `river_layer_shell_v1`. `sola-river` binds that global, attaches
per-output / per-seat children, and tracks exclusive keyboard focus.

## Why it exists

Software KVM on Sola (**sola-kvm**) needs edge barriers and/or exclusive
pointer/keyboard capture as layer surfaces. Without the WM binding
`river_layer_shell_v1`, River closes every layer surface immediately.

This is **not** a lan-mouse product path. lan-mouse is retired on the
desk; sola-kvm is the product. Layer-shell support is generic WM infra
useful for any layer client (KVM, future panels, etc.).

## Behaviour

| Piece | What sola-river does |
|-------|----------------------|
| Global bind | Logs `bound river_layer_shell_v1 (layer-shell clients enabled)` |
| Outputs / seat | `get_output` / `get_seat` children; first output `set_default` on manage |
| Exclusive focus | Skips `focus_window` / `clear_focus` while layer owns focus |
| Chord suppress | Disables shell xkb chords (Meta+Space, Meta+Tab, …) during exclusive focus so keys reach the layer client (e.g. Mac Cmd via sola-kvm) |

## Operator check

After install, restart Sola (or let sola-river self-watch re-exec) and
confirm in `/opt/sola/log/`:

```text
bound river_layer_shell_v1 (layer-shell clients enabled)
```

Desk KVM runbook: [`sola-kvm-operator.md`](./sola-kvm-operator.md).
