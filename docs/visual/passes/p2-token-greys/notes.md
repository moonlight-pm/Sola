# P2 — Token & type baseline

## Intent

Retune seed greys from GitHub Primer dark toward macOS Dark Mode system
greys. Keep cyan accent sparse. Quiet selection; neutralize switcher glass.

## Accent decision

**Keep cyan** (`#00d4ff`) — design language §2.1 sparse signal.

## Seed delta (high level)

| Token | Before (Primer) | After (macOS greys) |
|-------|-----------------|---------------------|
| bg-primary | `#0d1117` | `#1c1c1e` |
| bg-secondary | `#161b22` | `#2c2c2e` |
| bg-hover | `#1a2030` | `#3a3a3c` |
| border | `#2d333b` | `#48484a` |
| text-primary | `#e6edf3` | `#f5f5f7` |
| text-tertiary | `#6e7681` | `#636366` / muted `#98989d` |
| selection | `#1f6feb` | `#1a3a45` |
| accent | `#00d4ff` | `#00d4ff` (unchanged) |
| shell-switcher-bg | `#00d4ff2e` | `#2c2c2ecc` |
| shell-switcher-border | `#00d4ff59` | `#ffffff26` |

## After install

Sticky `~/.config/sola/theme/current.yaml` may still hold old Primer values —
reset Default in storybook Theme page or remove that file and restart to see
seed defaults. Then capture `after-*.png` here.
