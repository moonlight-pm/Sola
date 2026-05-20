# Distribution

What a host system needs to run Sola, and the patches we currently
carry. **NixOS only** — that's the target and the only system we
test on. Cross-distro packaging is explicitly not a goal.

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

**File:** `nix/patches/river-xwayland-destroy-state.patch` (in this
repo; also applied via `nix/module.nix`).

**What it fixes:** River 0.4 panics with `reached unreachable code`
when an Xwayland surface is destroyed without a preceding unmap —
i.e. any X client (Steam, a game, a misbehaving toolkit) that
crashes mid-render or tears its surface down out of order. Easiest
repro is `gamescope -- steam` exiting with children still mapped,
but the same class of bug fires from bare Xwayland clients too.

**Where:** `river/Window.zig:328` (`Window.destroy()`) hits the
`unreachable` arm because `XwaylandWindow.handleDestroy()`
(`river/XwaylandWindow.zig:148`) sets `Window.impl = .destroying`
without transitioning `Window.state` out of `.ready` /
`.initialized` / `.mapped`. The companion XDG path
(`XdgToplevel.handleDestroy`) already heals state correctly; the
patch mirrors that logic for X11 and additionally handles `.mapped`
gracefully (X11 does not guarantee unmap-before-destroy the way XDG
does).

**How we ship it:** `nix/module.nix` overrides `pkgs.river` and
appends the patch. See that file for the canonical apply.

**When to drop:** if `XwaylandWindow.handleDestroy` upstream gains
the same `switch (window.state)` block that `XdgToplevel.handleDestroy`
has, the patch becomes redundant.

## Steam

Runs directly under sola-river — just launch `steam` (or pick it
from the launcher). No `gamescope` wrapper is required.

(Earlier notes here described a `gamescope -- steam` workaround
because bare Steam used to crash river before the carried Xwayland
destroy-state patch was in place. The patch + current river/wlroots
combo handles Steam's Xwayland teardown cleanly, and the wrapper
is no longer needed. Gamescope is still useful for specific games
that need a fixed render resolution or extra HDR/scaling control,
but it's not a sola requirement.)

## Other host packages

Standard wlroots-side dependencies pulled in by River itself
(wayland, libinput, xkbcommon, pixman, wlroots ≥ 0.20). On the Sola
side, GTK4 + WebKitGTK 6.0 are runtime requirements for every
[[sola-app|WebView app]]. These are normal nixpkgs packages with no
patches.

## CEF runtime libraries (sola-kit)

`sola-kit` (and any future CEF-backed Sola app) links against
`libcef.so` from `~/.cache/sola/cef-<version>/Release/`. libcef.so
itself pulls in ~26 transitive native libraries
(glib/nss/atk/dbus/cups/X11/gbm/…). On NixOS these don't live at
`/usr/lib`, and we want zero `LD_LIBRARY_PATH` gymnastics or wrapper
scripts at runtime — so we make `libcef.so` itself know where to look.

The shape:

1. **`programs.nix-ld.libraries`** in `/etc/nixos/configuration.nix`
   collates the required packages' `.so` files into a single flat
   directory at `/run/current-system/sw/share/nix-ld/lib`. The path is
   stable — `/run/current-system` is repointed by `nixos-rebuild
   switch`, so the indirection always resolves to the active config's
   library set.
2. **`patchelf` runs once** as part of `sola_make::cef::ensure_cef`
   immediately after a fresh CEF tarball is extracted. It appends
   `/run/current-system/sw/share/nix-ld/lib` to `libcef.so`'s
   `DT_RUNPATH` (which the upstream tarball ships as just `$ORIGIN`).
   With that, libcef.so's own RUNPATH covers all its transitive deps;
   the `sola-kit` binary doesn't need any extra rpath plumbing.

Why not just bake the path into the sola-kit executable's RPATH? Per
`ld.so(8)`, an executable's `DT_RPATH` only covers its *own* direct
deps. libcef.so's deps are loaded under libcef.so's own
`DT_RPATH`/`DT_RUNPATH`, not the executable's. So the patch must land
on libcef.so itself.

### Required `programs.nix-ld.libraries` entries

```nix
programs.nix-ld.enable = true;
programs.nix-ld.libraries = with pkgs; [
  glib                                    # libglib-2.0, libgobject-2.0, libgio-2.0
  nss nspr                                # libnss3, libnssutil3, libsmime3, libnspr4
  atk                                     # libatk-1.0
  at-spi2-atk                             # libatk-bridge-2.0
  at-spi2-core                            # libatspi
  dbus                                    # libdbus-1
  cups                                    # libcups
  expat                                   # libexpat
  cairo pango                             # libcairo, libpango-1.0
  alsa-lib                                # libasound
  libxkbcommon                            # libxkbcommon
  libgbm                                  # libgbm (split from mesa as of nixpkgs 25.x)
  libdrm mesa                             # libdrm + the rest of mesa
  systemd                                 # libudev
  xorg.libX11 xorg.libXcomposite xorg.libXdamage xorg.libXext
  xorg.libXfixes xorg.libXrandr xorg.libxcb
];
```

`environment.systemPackages` must also include `patchelf` so
`ensure_cef` can run it, **and** `libxkbcommon` so its `.pc` file
is in the system `PKG_CONFIG_PATH` for `smithay-client-toolkit`'s
build script (the previous WebKit/GTK build path got xkbcommon
pkg-config transitively through `gtk4`'s dev output; the sctk path
needs it explicitly).

```nix
environment.systemPackages = with pkgs; [
  patchelf
  libxkbcommon
];
```

After `nixos-rebuild switch`, verify with:

```sh
ls /run/current-system/sw/share/nix-ld/lib/ | grep -E '^(libglib|libnss|libgbm|libcups)'
```

— those `.so` files should be present.

### Re-patching libcef.so

`ensure_cef` only patchelfs after a fresh download. If you already have
libcef.so cached (`is_present()` short-circuits), or you change
`programs.nix-ld.libraries` in a way that requires re-patching, force
a re-patch by deleting the cache and re-running `cargo run -p
sola-make -- install-cef`:

```sh
rm -rf ~/.cache/sola/cef-*
cargo run -p sola-make -- install-cef
```

(Or run `patchelf --add-rpath /run/current-system/sw/share/nix-ld/lib
~/.cache/sola/cef-*/Release/libcef.so` directly.)

### Verifying a sola-kit build is self-resolving

```sh
ldd /opt/sola/bin/sola-kit | grep "not found"
```

Empty output = libcef.so's patched RUNPATH covers everything. Anything
in the list points at a missing `programs.nix-ld.libraries` entry; add
the package, `nixos-rebuild switch`, and re-patch libcef.so (see above)
— no rebuild of sola-kit needed.

## CEF GPU runtime (sola-kit, NixOS)

Beyond the dynamic-linker plumbing above, the GPU subprocess CEF spawns
needs three system-level inputs to initialize a GL/Vulkan context.
These are all set automatically when sola-kit starts up — but the
*system* has to provide them:

### 1. EGL vendor dispatch

`__EGL_VENDOR_LIBRARY_DIRS` must point at NixOS's libglvnd ICD JSON
directory so libEGL can find the active vendor driver. sola-kit's
`run<A>()` sets it to `/run/opengl-driver/share/glvnd/egl_vendor.d`
(populated by `hardware.graphics.enable = true` plus
`hardware.nvidia` / Mesa). Without this, libEGL loads but dispatches
to nothing → the GPU process can't initialise Skia (`Unable to
initialize SkSurface`) and `OnAcceleratedPaint` never fires.

### 2. Vulkan ICD discovery

`VK_ICD_FILENAMES` must point at the active vendor's Vulkan ICD JSON.
sola-kit defaults it to
`/run/opengl-driver/share/vulkan/icd.d/nvidia_icd.x86_64.json`. The
ANGLE/Vulkan backend (`--use-angle=vulkan`) needs this; the GL
backend uses it to query device capabilities.

### 3. NVIDIA / Mesa userspace libraries on the RUNPATH

`/run/opengl-driver/lib` is appended to libcef.so's `DT_RUNPATH` (and
to libEGL.so / libGLESv2.so / libvk_swiftshader.so / libvulkan.so.1)
by `sola_make::cef::patch_cef_libs_for_nix_ld`. This makes
`libnvidia-glcore.so`, `libnvidia-glsi.so`, `libGLX_nvidia.so.0`,
`libnvidia-tls.so`, `libgbm.so` (mesa-libgbm), etc. discoverable when
the EGL ICD dlopens them.

### CEF OSR transport: wl_shm (current) vs dma-buf (deferred)

CEF's offscreen rendering produces frames in two ways: a
zero-copy **dma-buf** path (`shared_texture_enabled = 1`,
`OnAcceleratedPaint`) or a CPU readback path (`shared_texture_enabled
= 0`, `OnPaint`) that delivers a BGRA8888 buffer for us to memcpy
into a `wl_shm` slot.

**sola-kit currently uses the wl_shm path** (see
`crates/sola-kit/src/cef/browser.rs::Browser::new` —
`shared_texture_enabled = 0`). The supporting code is at
`Surface::present_paint` in `crates/sola-kit/src/wayland/surface.rs`.

The dma-buf path is preferred long-term — zero copies, GPU-direct —
but on NVIDIA's proprietary driver it fails at link time inside CEF's
prebuilt ANGLE binaries. Specifically, CEF 147's
`Release/libEGL.so` / `Release/libGLESv2.so` reference several
**Mesa-only** EGL extension symbols:

```
eglExportDMABUFImageMESA, eglExportDMABUFImageQueryMESA,
eglImageFlushExternalEXT, eglQueryDevicesEXT
```

These don't exist in NVIDIA's `libEGL_nvidia.so.0`. With
`shared_texture_enabled = 1`, the GPU process gets stuck at
`shared_image_representation.cc:408 Unable to initialize SkSurface`
— `OnAcceleratedPaint` never fires.

Separately, NVIDIA proprietary 580.x's `libnvidia-glcore.so`
references `__malloc_hook` / `__realloc_hook` / `__free_hook` /
`__memalign_hook`, all removed in glibc 2.34+. Whether this is
actually load-fatal vs. lazy-failure depends on link options and
shows up in `LD_DEBUG=libs` output regardless.

**Why does Helium / Chromium-based AppImages work on the same box?**
AppImages run inside `appimage-run`, which wraps the binary in a
`bwrap` FHS sandbox where `/usr/lib/libEGL.so.1` is Mesa's libglvnd
build. Inside that sandbox, the load-time symbol lookup for
`eglExportDMABUFImageMESA` resolves against Mesa's dispatch table
(symbols exist; runtime dispatches to NVIDIA via the same libglvnd
indirection NixOS uses). The shape of the FHS is what makes ANGLE's
libEGL load cleanly. Outside the FHS, our nix-ld setup doesn't
provide the equivalent dispatch surface, so the linker fails.

#### When to revisit dma-buf

Switch back to dma-buf when one of:

1. **You change to NVIDIA Open + Mesa NVK.** Set
   `hardware.nvidia.open = true` in `/etc/nixos/configuration.nix`
   and let Mesa drive the GPU. Mesa's libEGL exposes the `*MESA`
   extensions natively. Requires a compatible GPU (Turing GTX 16xx /
   RTX 20-series or newer for full NVK support). Note: we previously
   had Mesa drivers and switched away due to other GPU stability
   issues; that investigation may need revisiting.
2. **The host is Intel or AMD** (Mesa-only userspace) — works
   out of the box; just flip `shared_texture_enabled` back to `1`.
3. **Performance demands it** — heavy animation or 4K video in
   `sola-browser` is the regime where the wl_shm CPU readback is
   measurable. For sola-shell, sola-settings, sola-kit storybook,
   etc., the difference is in the noise.
4. **CEF starts shipping libEGL with the dispatch table independent
   of host vendor** (i.e. ships its own libglvnd-style dispatcher
   in Release/). Watch upstream for this.

The flip is one line:
```rust
// crates/sola-kit/src/cef/browser.rs::Browser::new
window_info.shared_texture_enabled = 1;  // dma-buf
```
The dma-buf code path (`Surface::present_dmabuf`,
`KitRenderHandler::on_accelerated_paint`) is left in place exactly
for this — no rewrite needed.

### Quick diagnosis: what's the GPU stack failing on?

If the wl_shm transport stops working in a future Chromium / NVIDIA
combination, gather:

```sh
WAYLAND_DISPLAY=wayland-1 LD_DEBUG=libs LD_DEBUG_OUTPUT=/tmp/ld /opt/sola/bin/sola-kit
# inspect /tmp/ld.<pid> for "undefined symbol" lines around libEGL,
# libGLESv2, and libnvidia-*. The first fatal-tagged miss is usually
# diagnostic.
```

Compare against `tail /opt/sola/log/river.log`. If sola-kit didn't
reach the configure ack, the issue is host-side (Wayland event pump,
sctk plumbing). If it did but `on_paint` doesn't fire, the GPU
process is failing to rasterise — same root-cause class as the
dma-buf-on-NVIDIA situation above.

### Why not packaging sola-kit as a Nix derivation?

That would be the most-correct NixOS solution and we'll get there for
release, but for the develop-and-iterate phase the binary is a
`cargo build` artefact. The patchelf-libcef + nix-ld path approach
lets us keep `cargo run` ergonomics while still producing a binary
that runs cleanly without any environmental contortions.

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

## A note on GTK native widgets and GSettings

Anything that opens a stock GTK widget — `GtkColorChooser`,
`GtkFontChooser`, `GtkPrintDialog`, the file picker, etc. — will lazily
call `g_settings_new()` and abort the host process if the matching
schema isn't reachable. Cargo-built binaries (every Sola app today)
skip nixpkgs' `wrapGAppsHook`, which is what normally aggregates
package-provided schemas into a per-binary `XDG_DATA_DIRS`.

The pragmatic answer is **don't use stock GTK chooser widgets from a
Sola WebView**. The `<input type="color">` element, for example,
spawns the GTK color chooser dialog, which is both visually foreign
(no token/theme integration) and a crash hazard on a Sola system.
The general guidance: build the picker in the WebView (HTML/CSS/JS,
themed via [[sola-kit]] tokens), don't invoke a system dialog. Same
logic applies to font choosers, print dialogs, and file pickers as
they come up — host the UI inside the app rather than calling out.

If a stock GTK widget genuinely is the right answer for some future
case, the fix is to either wire `XDG_DATA_DIRS` to point at the
per-package `share/gsettings-schemas/<pkg-version>/` dirs in
`environment.sessionVariables`, or wrap the binary the
nixpkgs-idiomatic way. We'll cross that bridge if the use case
appears.

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
