# Distribution

What a host system needs to run Sola, and the patches we currently
carry. Reference for packaging on NixOS and (eventually) other
distros.

## Compositor

**River ≥ 0.4.5, patched.**

Sola is a [[sola-river|River bridge]] — `sola-river` consumes the
Wayland-native window-manager protocol family that River 0.4
introduced (`river_window_manager_v1`, `river_xkb_bindings_v1`,
`river_libinput_config_v1`). These do not exist in River 0.3.x
("river-classic"), so Sola cannot run on the 0.3 line.

Within 0.4 we require **0.4.5 or later** for accumulated
Xwayland/Steam stability fixes (notably commit `45d4f4a` "Xwayland:
add missing null check" — reproduced in practice with Steam +
gamemoderun).

### Carried patch: `XwaylandWindow: heal window state on destroy`

**Status:** present in 0.4.5 and `main` (codeberg.org/river/river,
commit 6d32af3 at the time of writing). Not upstreamed.

**File:** `/etc/nixos/patches/river-xwayland-destroy-state.patch`

**What it fixes:** River 0.4 panics with `reached unreachable code`
when an Xwayland surface is destroyed without a preceding unmap —
e.g. when an X client crashes mid-render or when gamescope (running
nested) exits while children are still mapped. Reproduced reliably
by launching `gamescope -- steam` from the launcher.

**Where:** `river/Window.zig:328` (`Window.destroy()`) hits the
`unreachable` arm because `XwaylandWindow.handleDestroy()`
(`river/XwaylandWindow.zig:148`) sets `Window.impl = .destroying`
without transitioning `Window.state` out of `.ready` /
`.initialized` / `.mapped`. The companion XDG path
(`XdgToplevel.handleDestroy`) already heals state correctly; the
patch mirrors that logic for X11 and additionally handles `.mapped`
gracefully (X11 does not guarantee unmap-before-destroy the way XDG
does).

**Apply (NixOS):**
```nix
let
  river-patched = unstable.river.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./patches/river-xwayland-destroy-state.patch ];
  });
in
  environment.systemPackages = [ ... river-patched ... ];
```

**When to drop:** if `XwaylandWindow.handleDestroy` upstream gains
the same `switch (window.state)` block that `XdgToplevel.handleDestroy`
has, the patch becomes redundant.

## Steam

Works **only via gamescope nested**: `gamescope -- steam` (added as
a launcher entry). Gamescope appears as a single Wayland client
window on River; Steam runs inside it and its Xwayland chaos is
contained.

Direct `steam` (without gamescope) crashes River 0.4 even with the
patch above — Steam's bare Xwayland window-lifecycle dance hits
other code paths in the WindowManagerV1 destroy machinery that we
have not chased.

The carried patch is what keeps the gamescope-nested workflow
stable: when gamescope eventually exits (Steam crashing inside it,
user closing it, etc.) River no longer panics on the disconnect.

## Other host packages

Standard wlroots-side dependencies pulled in by River itself
(wayland, libinput, xkbcommon, pixman, wlroots ≥ 0.20). On the Sola
side, GTK4 + WebKitGTK 6.0 are runtime requirements for every
[[sola-app|WebView app]]. These are normal nixpkgs packages with no
patches.

## Default browser / URL handler

When a non-Sola app (`xdg-open`, GIO, electron's `shell.openExternal`,
`git web--browse`, etc.) opens an http/https URL, we want it to land
in `sola-browser` as a new tab. We do this with a stock XDG MIME
handler and a thin CLI:

- `solactl open <URL>` connects to sola-bus and emits
  `Topic::OpenUrl`. `sola-browser` subscribes and creates a tab.
- `crates/sola-browser/dist/applications/sola-browser.desktop`
  declares Sola Browser as a handler for `x-scheme-handler/http`,
  `x-scheme-handler/https`, `text/html`, and `application/xhtml+xml`,
  with `Exec=/opt/sola/bin/solactl open %u`.
- `cargo make install` copies that file to
  `~/.local/share/applications/sola-browser.desktop` (it mirrors any
  `crates/*/dist/` tree onto `$XDG_DATA_HOME`) and runs
  `update-desktop-database` so the cache picks it up immediately.

User-local install on purpose: Sola is a single-user system, and
`$XDG_DATA_HOME` (default `~/.local/share`) is always on the search
path, so xdg-open / GIO find the handler with no `XDG_DATA_DIRS`
ceremony.

### Required host packages

NixOS — add to `environment.systemPackages`:
```nix
xdg-utils          # xdg-open, xdg-mime
desktop-file-utils # update-desktop-database
```

Without `xdg-utils`, electron-style apps that call `xdg-open` for the
default browser silently fail with no fallback.

### One-time host setup

After the first `cargo make install`:

1. Register Sola Browser as the default for web links — writes the
   pairing into `~/.config/mimeapps.list`:
   ```sh
   xdg-mime default sola-browser.desktop x-scheme-handler/http
   xdg-mime default sola-browser.desktop x-scheme-handler/https
   xdg-mime default sola-browser.desktop text/html
   ```
2. Optional — make terminal apps (`git web--browse`, `gh browse`,
   `htmlview`, etc.) route through us too. In `~/.zshrc`:
   ```sh
   export BROWSER='/opt/sola/bin/solactl open'
   ```

Verify: `xdg-settings get default-web-browser` should print
`sola-browser.desktop`, and `xdg-mime query default x-scheme-handler/https`
should match.

### Troubleshooting

- `solactl: bus connect failed: …` — sola isn't running, so there's
  nothing to receive the URL. Start `sola` first.
- The wrong browser opens — the caller process started before
  `xdg-utils` was on PATH or before the handler was registered.
  Restart the caller (Zed, Slack, etc.) from a fresh shell.
- xdg-open routes to the wrong browser despite `xdg-mime default` —
  check `~/.config/mimeapps.list` for an older entry and remove it,
  then re-run `xdg-mime default`.
- Flatpak/Snap apps still open the previous browser — those go through
  `xdg-desktop-portal` and need a portal `OpenURI` backend, which we
  don't ship yet. See the upcoming portal-backend work.

## See also

- [[Sola]] — system overview, runtime environment
- [[Process Model]] — how River fits under sola's supervision
- River upstream: codeberg.org/river/river
