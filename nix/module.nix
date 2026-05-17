{ config, lib, pkgs, ... }:

let
  cfg = config.services.sola;

  # River carries one patch we haven't upstreamed (Xwayland window
  # destroy state healing). See docs/vault/Distribution.md for the
  # rationale.
  river-patched = pkgs.river.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./patches/river-xwayland-destroy-state.patch ];
  });

  sola-pkg = pkgs.callPackage ./sola.nix { };
in
{
  options.services.sola = {
    enable = lib.mkEnableOption "Sola desktop shell";

    package = lib.mkOption {
      type = lib.types.package;
      default = sola-pkg;
      description = "The Sola package (binaries + bundled CEF).";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = with pkgs; [
      cfg.package
      river-patched

      # Sola runtime tooling.
      patchelf            # used by sola's CEF housekeeping
      libxkbcommon        # pkg-config used by smithay-client-toolkit
      xdg-utils           # xdg-open / xdg-mime for default-browser routing
      desktop-file-utils  # update-desktop-database

      # WebKitGTK (legacy sola-app stack) modules. Sola-kit apps don't
      # need these, but several legacy apps (sola-browser,
      # sola-terminal, sola-shell, sola-mail) still depend on them at
      # runtime.
      glib-networking     # WebKit TLS
      gst_all_1.gstreamer
      gst_all_1.gst-plugins-base
      gst_all_1.gst-plugins-good
      gst_all_1.gst-plugins-bad
      gst_all_1.gst-plugins-ugly
      gst_all_1.gst-libav
    ];

    # CEF (sola-kit) needs ~26 transitive native libs at runtime. We
    # collate them via nix-ld's dispatch dir and patch libcef.so's
    # DT_RUNPATH to point at it — see docs/vault/Distribution.md for
    # the long story. The package's libcef.so ships pre-patched.
    programs.nix-ld.enable = true;
    programs.nix-ld.libraries = with pkgs; [
      glib
      nss
      nspr
      atk
      at-spi2-atk
      at-spi2-core
      dbus
      cups
      expat
      cairo
      pango
      alsa-lib
      libxkbcommon
      libgbm
      libdrm
      mesa
      systemd
      xorg.libX11
      xorg.libXcomposite
      xorg.libXdamage
      xorg.libXext
      xorg.libXfixes
      xorg.libXrandr
      xorg.libxcb
    ];

    # WebKit's lazy module loading needs these resolvable at the
    # session-environment level (the env doesn't propagate into a
    # WebKitWebProcess otherwise — the renderer aborts on first use).
    environment.sessionVariables = {
      GIO_EXTRA_MODULES = "${pkgs.glib-networking}/lib/gio/modules";
      GST_PLUGIN_SYSTEM_PATH_1_0 = lib.concatMapStringsSep ":"
        (p: "${p}/lib/gstreamer-1.0")
        (with pkgs.gst_all_1; [
          gstreamer.out
          gst-plugins-base
          gst-plugins-good
          gst-plugins-bad
          gst-plugins-ugly
          gst-libav
        ]);
    };

    # Sola talks Wayland/DRM directly via Smithay + sctk and uses CEF's
    # GPU subprocess for the kit. Both want a working GBM / EGL stack.
    hardware.graphics.enable = true;

    # The sola binaries have several hardcoded `/opt/sola/*` paths
    # baked into the source (asset lookup, launcher app commands,
    # log destination, cursor theme path). Nix wants everything in
    # the store, so we shim /opt/sola/{bin,share} as symlinks into
    # the package output and create /opt/sola/log as a real writable
    # directory. Activation refuses to clobber a real directory at
    # those paths — useful if you happen to be sharing a box with a
    # `cargo make install` setup.
    system.activationScripts.sola = ''
      mkdir -p /opt/sola /opt/sola/log
      chmod 1777 /opt/sola/log

      for dir in bin share; do
        target="/opt/sola/$dir"
        if [ -L "$target" ] || [ ! -e "$target" ]; then
          ln -sfn ${cfg.package}/$dir "$target"
        else
          echo "sola: /opt/sola/$dir is a real directory; leaving alone" >&2
        fi
      done
    '';
  };
}
