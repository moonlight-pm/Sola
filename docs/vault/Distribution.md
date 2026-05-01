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

## WebKit runtime modules

WebKitGTK is modular: it dynamically loads pieces of the system at
runtime — TLS, codecs, audio sinks — rather than statically linking
them. NixOS does not auto-wire those into a WebView's environment, so
**every WebKit feature has to be both installed and reachable via an
env var**, or the WebContent process aborts (often with no
`load-failed` signal — just a silent renderer kill that leaves the tab
at `about:blank`). [[sola-browser]] catches these via
`connect_web_process_terminated` and auto-reloads, but the underlying
fix is to give WebKit what it needs.

Two known-required pieces today, both wired in
`/etc/nixos/configuration.nix`:

### TLS — `glib-networking`

WebKitGTK's HTTPS goes through GIO's TLS backend, provided by
`glib-networking` as a GIO module (`libgiognutls.so`). Without it,
every `https://` URL fails with `TLS support is not available` from
the load-failed handler.

```nix
environment.systemPackages = with pkgs; [ glib-networking ];
environment.sessionVariables = {
  GIO_EXTRA_MODULES = "${pkgs.glib-networking}/lib/gio/modules";
};
```

### Media — GStreamer plugin set

WebKit uses GStreamer for `<audio>`/`<video>`. The first time a page
constructs a media element, `MediaPlayerPrivateGStreamer::createAudioSink()`
walks the registry for a usable sink. If none is found,
`WTFCrashWithInfo` aborts the WebContent process. (Symptom: any page
that auto-plays or pre-fetches audio kills the tab.)

The minimum set:

| Package | What it provides |
|---|---|
| `gstreamer` | Core registry/runtime |
| `gst-plugins-base` | Base elements (alsasink, decodebin) |
| `gst-plugins-good` | **`pulsesink` / `pipewiresink` / `autoaudiosink`** — without this, no audio sink exists |
| `gst-plugins-bad`, `gst-plugins-ugly` | Codecs not in `good` (mpegaudioparse etc.) |
| `gst-libav` | Actual decoders (mp3, aac, h264) via FFmpeg |

```nix
environment.systemPackages = with pkgs; [
  gst_all_1.gstreamer
  gst_all_1.gst-plugins-base
  gst_all_1.gst-plugins-good
  gst_all_1.gst-plugins-bad
  gst_all_1.gst-plugins-ugly
  gst_all_1.gst-libav
];
environment.sessionVariables = {
  GST_PLUGIN_SYSTEM_PATH_1_0 = lib.concatMapStringsSep ":"
    (p: "${p}/lib/gstreamer-1.0")
    (with pkgs.gst_all_1; [
      gstreamer.out          # NB: must be `.out`, not `gstreamer`
      gst-plugins-base
      gst-plugins-good
      gst-plugins-bad
      gst-plugins-ugly
      gst-libav
    ]);
};
```

> **Watch the gstreamer output.** `pkgs.gst_all_1.gstreamer` (no
> qualifier) resolves to the `bin` output (tools like `gst-inspect-1.0`),
> which has no `lib/gstreamer-1.0/` directory — pointing the env var
> there silently drops core elements like `typefind` and `decodebin`,
> and WebKit segfaults later inside `createVideoSink`. Use
> `gstreamer.out` to get the default output that ships
> `libgstcoreelements.so`. The other `gst-plugins-*` packages don't
> have this split.

### Diagnosing future "WebView dies on this site" reports

The pattern is the same each time: WebKit tries to load a feature,
the system can't satisfy it, the WebContent process hard-aborts.
Recipe to identify the next missing piece:

1. **Reproduce the kill** — e.g., click the breaking link.
2. **Confirm it's a renderer crash** — look in `/opt/sola/log/sola.log`
   for the `web-process-terminated; reloading` line. `reason=Crashed`
   means a real abort (vs `ExceededMemoryLimit` or `TerminatedByApi`).
3. **Get the stack** — `coredumpctl list --since="10 minutes ago"`
   will show the WebKitWebProcess core; `coredumpctl info <PID>` (or
   `journalctl --since`) prints the symbolicated trace. The top
   non-WTF frame names the WebKit subsystem that aborted —
   `createAudioSink`, `createNetworkSession`, etc. — which usually
   maps directly to the missing module.
4. **Add the module + env var, rebuild, relog** to propagate the
   updated session vars to a fresh sola process.

After updating session vars, the running sola won't see the change
(it inherited the old environment). Either log out of the TTY and
back in, or `source /etc/set-environment && pkill sola` and relaunch.

## GSettings schemas

GTK4 and WebKitGTK lazily call `g_settings_new()` for built-in widgets
(color chooser, font chooser, recent-files manager, drag-and-drop, the
WebKit `<input type="color">` dialog, etc.). When the matching schema
isn't reachable, GLib's lookup falls into `g_error()` — a fatal log
level — and the process aborts with:

```
GLib-GIO-ERROR **: No GSettings schemas are installed on the system
zsh: trace trap (core dumped)  sola-...
```

The signature is distinct from the WebKit renderer crashes above: the
GTK *host* process dies, not the WebContent process, and the abort
shows up on the TTY rather than as a `web-process-terminated` line in
`/opt/sola/log/sola.log`.

NixOS doesn't install a default schema dir; nixpkgs ships compiled
schemas under each package's `share/gsettings-schemas/<pkg-version>/glib-2.0/schemas/`
(a namespaced path that does NOT match GLib's default search of
`<dir>/glib-2.0/schemas/` directly under XDG_DATA_DIRS). The
`wrapGAppsHook` helper aggregates every dependency's schemas into a
single dir that it prepends to each wrapped binary's XDG_DATA_DIRS;
cargo-built binaries skip that wrap entirely.

So on NixOS the fix is two parts: install the schema-providing
packages AND point `XDG_DATA_DIRS` at the per-package schemas dirs.

```nix
environment.systemPackages = with pkgs; [
  # gtk4 / webkit6
  gtk4
  gsettings-desktop-schemas
  webkitgtk_6_0
  # ...
];

environment.sessionVariables = {
  XDG_DATA_DIRS = lib.concatMapStringsSep ":"
    (p: "${p}/share/gsettings-schemas/${p.name}")
    [ pkgs.gsettings-desktop-schemas pkgs.gtk4 ];
};
```

NixOS merges the `sessionVariables` value with the per-user profile
prefix paths (`~/.nix-profile/share`, `/run/current-system/sw/share`,
etc.) when it composes `/etc/set-environment`, so this adds entries
rather than replacing the user's existing XDG_DATA_DIRS.

Symptoms before the package landed:
- `sola-kit`'s color picker triggers an immediate process abort when
  WebKit opens the GTK color chooser dialog.
- Other Sola apps may abort intermittently when a code path that needs
  schemas is reached for the first time (e.g. file dialogs).

After `nixos-rebuild switch`, log out of the TTY and back in (or
otherwise re-source `/etc/set-environment`) before relaunching sola so
the updated `XDG_DATA_DIRS` is visible to its child processes.

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
