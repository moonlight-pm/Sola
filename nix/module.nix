{ config, lib, pkgs, riverPackage, ... }:

let
  cfg = config.services.sola;

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

    riverPackage = lib.mkOption {
      type = lib.types.package;
      default = riverPackage;
      description = ''
        River compositor with our carried Xwayland-destroy-state
        patch. Defaults to a river-patched built from this flake's
        pinned nixpkgs (so it works regardless of whether the host
        config is on stable, unstable, or anything in between).
        Override only if you want to manage River yourself.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = with pkgs; [
      cfg.package
      cfg.riverPackage

      # Sola runtime tooling.
      patchelf            # used by sola's CEF housekeeping
      libxkbcommon        # pkg-config used by smithay-client-toolkit
      xdg-utils           # xdg-open / xdg-mime for default-browser routing
      desktop-file-utils  # update-desktop-database

      # GStreamer / glib-networking historically pulled for WebKitGTK
      # (apocrypha sola-app stack). Kept for media / TLS helpers used
      # by other packages in this list; iced sola-kit apps do not need
      # WebKitGTK itself.
      glib-networking     # WebKit TLS
      gst_all_1.gstreamer
      gst_all_1.gst-plugins-base
      gst_all_1.gst-plugins-good
      gst_all_1.gst-plugins-bad
      gst_all_1.gst-plugins-ugly
      gst_all_1.gst-libav
    ];

    # nix-ld keeps a dispatch dir of common native libs for any
    # prebuilt/foreign ELF that needs them (historical CEF path
    # removed; still useful for third-party helpers).
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
      # Flat package names (nixpkgs dropped the `xorg.` set).
      libx11
      libxcomposite
      libxdamage
      libxext
      libxfixes
      libxrandr
      libxcb
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

    # sola-kvm (evdev backend) needs RW on /dev/input/event* for EVIOCGRAB
    # while remote. Re-plug creates *new* nodes that do not inherit old ACLs,
    # so a one-shot setfacl always rots. TAG+="uaccess" lets logind grant the
    # active seat user RW on every (re)plug; GROUP=input is a belt-and-braces
    # path for users who are also in the input group.
    users.groups.input = { };
    services.udev.extraRules = ''
      # sola-kvm: seat user can open input event nodes after re-plug
      SUBSYSTEM=="input", KERNEL=="event[0-9]*", MODE="0660", GROUP="input", TAG+="uaccess"
    '';

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
