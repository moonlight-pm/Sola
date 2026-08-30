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

    installRelease = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Install the published Sola release tarball onto PATH and symlink
        /opt/sola/{bin,share} into that package.

        Set to false on developer machines that run `cargo make install`:
        the module still provides patched River, nix-ld (CEF), GPU, fonts,
        and kvm udev, but leaves /opt/sola as real directories for local
        binaries. See CONTRIBUTING.md.
      '';
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
      cfg.riverPackage

      # Sola runtime tooling.
      patchelf            # cargo make install-cef RUNPATH patch
      libxkbcommon        # pkg-config (smithay-client-toolkit) + dlopen
      wayland             # iced/winit dlopen libwayland-client via RUNPATH
      xwayland            # River looks up the Xwayland binary on PATH
      xdg-utils           # xdg-open / xdg-mime for default-browser routing
      desktop-file-utils  # update-desktop-database

      # GStreamer / glib-networking historically pulled for WebKitGTK
      # (apocrypha sola-app stack). Kept for media / TLS helpers used
      # by other packages in this list; iced sola-kit apps do not need
      # WebKitGTK itself.
      glib-networking
      gst_all_1.gstreamer
      gst_all_1.gst-plugins-base
      gst_all_1.gst-plugins-good
      gst_all_1.gst-plugins-bad
      gst_all_1.gst-plugins-ugly
      gst_all_1.gst-libav
    ] ++ lib.optional cfg.installRelease cfg.package;

    # Defaults every Sola app's font roles reach for. Missing families
    # silently fall back through fontconfig and look wrong — see
    # docs/manual/distribution.md.
    fonts.packages = with pkgs; [
      inter
      jetbrains-mono
    ];

    # CEF (sola-browser / sola-wrapper) needs ~26 transitive native
    # libs at runtime. We collate them via nix-ld's dispatch dir and
    # patch libcef.so's DT_RUNPATH to point at it — see
    # docs/vault/Distribution.md. cargo make install-cef patches the
    # cache tree; the release package's libcef.so ships pre-patched.
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
    # GPU subprocess for the browser. Both want a working GBM / EGL stack.
    hardware.graphics.enable = true;

    # Menubar Bluetooth popover talks to org.bluez on the system bus
    # (sola-shell, in-process). Without bluetoothd the kernel can still
    # expose hci0 while the chip stays hidden. No blueman — Sola owns
    # the UI. Experimental unlocks Battery1 on more HID / LE devices.
    hardware.bluetooth.enable = true;
    hardware.bluetooth.powerOnBoot = true;
    hardware.bluetooth.settings.General.Experimental = true;

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

    # Binaries hardcode `/opt/sola/{bin,share,log}`.
    #
    # installRelease=true: symlink bin/share at the store package; leave
    # a real directory alone (cargo make install / shared box).
    #
    # installRelease=false: never reference cfg.package (that derivation
    # fetches the GitHub tarball). Create real directories so
    # `cargo make install` can write. Replace leftover store *symlinks*
    # from a previous release install; do not touch a real directory.
    system.activationScripts.sola =
      if cfg.installRelease then ''
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
      '' else ''
        mkdir -p /opt/sola /opt/sola/log
        chmod 1777 /opt/sola/log

        for dir in bin share; do
          target="/opt/sola/$dir"
          if [ -L "$target" ]; then
            echo "sola: replacing store symlink $target with a real directory" >&2
            rm -f "$target"
          fi
          mkdir -p "$target"
        done
      '';
  };
}
