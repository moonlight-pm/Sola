# Live installer session: multi-user → systemd kiosk on tty1 → cage + sola-install.
#
# Guest binaries are patchelf'd in sola-from-stage.nix so cage can exec them.
# sola-install runs standalone (no river/bus) under cage.
#
# Critical race we fix here: simpledrm paints Plymouth early, then virtio-gpu
# claims the real display a few seconds later. If cage binds simpledrm first,
# the compositor dies when simpledrm unbinds → text console / getty. Wait for
# a non-simple DRM device (and drop simpledrm) before starting cage.

{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.sola;
  installBin =
    if cfg.enable then "${cfg.package}/bin/sola-install" else "/opt/sola/bin/sola-install";
  packageShare = if cfg.enable then "${cfg.package}/share" else "/opt/sola/share";

  # Root oneshot: wait for virtio (or any non-simple) DRM, unload simpledrm.
  gpuReadyScript = pkgs.writeShellScript "sola-gpu-ready" ''
    set -euo pipefail
    log() {
      echo "sola-gpu-ready: $*" | ${pkgs.util-linux}/bin/logger -t sola-gpu-ready || true
      echo "sola-gpu-ready: $*" > /dev/ttyS0 2>/dev/null || true
    }

    log "waiting for non-simple DRM…"
    ready=""
    found_real=0
    for i in $(${pkgs.coreutils}/bin/seq 1 150); do
      for card in /sys/class/drm/card[0-9]; do
        [ -d "$card" ] || continue
        name=$(${pkgs.coreutils}/bin/basename "$card")
        [ -e "/dev/dri/$name" ] || continue
        drv=""
        if [ -L "$card/device/driver" ]; then
          drv=$(${pkgs.coreutils}/bin/basename "$(${pkgs.coreutils}/bin/readlink -f "$card/device/driver")")
        fi
        case "$drv" in
          ""|*simple*) continue ;;
        esac
        ready="/dev/dri/$name"
        found_real=1
        log "ready $ready (driver=$drv) after ~$((i * 100))ms"
        break 2
      done
      ${pkgs.coreutils}/bin/sleep 0.1
    done

    if [ -z "$ready" ]; then
      # Fall back to any card so the kiosk can still try.
      for n in /dev/dri/card*; do
        if [ -e "$n" ]; then
          ready="$n"
          log "fallback $ready (timed out waiting for non-simple)"
          break
        fi
      done
    fi

    if [ -z "$ready" ]; then
      log "FATAL: no /dev/dri/card* after wait"
      exit 1
    fi

    # Only drop simpledrm once a real GPU is present — otherwise we'd remove
    # the only DRM device and cage has nowhere to paint.
    if [ "$found_real" = "1" ] && ${pkgs.kmod}/bin/lsmod | ${pkgs.gnugrep}/bin/grep -q '^simpledrm'; then
      log "unloading simpledrm (real GPU is $ready)"
      ${pkgs.kmod}/bin/rmmod simpledrm 2>/dev/null || log "rmmod simpledrm failed (ok if in use)"
    fi

    ${pkgs.coreutils}/bin/mkdir -p /run/sola
    echo "$ready" > /run/sola/drm-device
    log "wrote /run/sola/drm-device → $ready"
  '';

  sessionScript = pkgs.writeShellScript "sola-install-session" ''
    set -euo pipefail

    log() {
      echo "sola-install-session: $*" | ${pkgs.util-linux}/bin/logger -t sola-install || true
      echo "sola-install-session: $*" > /dev/console 2>/dev/null || true
      echo "sola-install-session: $*" > /dev/ttyS0 2>/dev/null || true
    }

    log "begin uid=$(id -u) tty=$(tty 2>/dev/null || echo '?')"

    ${pkgs.plymouth}/bin/plymouth quit 2>/dev/null || true

    export HOME="''${HOME:-/home/sola-install}"
    export XDG_RUNTIME_DIR="''${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
    if [ ! -d "$XDG_RUNTIME_DIR" ]; then
      ${pkgs.coreutils}/bin/mkdir -p "$XDG_RUNTIME_DIR"
      ${pkgs.coreutils}/bin/chmod 700 "$XDG_RUNTIME_DIR"
    fi

    export SOLA_INSTALL_STANDALONE=1
    # Never attach bus/shell under the kiosk (no menubar / launcher / switcher).
    unset SOLA_INSTALL_USE_BUS || true
    export SOLA_NO_SELF_WATCH=1
    export WGPU_BACKEND="''${WGPU_BACKEND:-gl}"
    export LIBGL_ALWAYS_SOFTWARE="''${LIBGL_ALWAYS_SOFTWARE:-1}"
    export WLR_RENDERER="''${WLR_RENDERER:-pixman}"
    export WINIT_UNIX_BACKEND=wayland
    export XDG_SESSION_TYPE=wayland
    export XDG_CURRENT_DESKTOP=cage
    # Reduce accidental shell-ish shortcuts if any client inherits them.
    export SOLA_INSTALL_KIOSK=1
    export XDG_DATA_DIRS="${packageShare}:/opt/sola/share:''${XDG_DATA_DIRS:-/run/current-system/sw/share}"
    export RUST_BACKTRACE="''${RUST_BACKTRACE:-1}"
    export RUST_LOG="''${RUST_LOG:-sola_install=info,wgpu_hal=warn,wgpu_core=warn}"

    if [ -r /run/sola/drm-device ]; then
      export WLR_DRM_DEVICES="$(${pkgs.coreutils}/bin/cat /run/sola/drm-device)"
      log "WLR_DRM_DEVICES=$WLR_DRM_DEVICES"
    fi

    bin="${installBin}"
    if [ ! -x "$bin" ]; then
      bin="/opt/sola/bin/sola-install"
    fi
    if [ ! -x "$bin" ]; then
      log "FATAL: sola-install missing"
      sleep 3600
      exit 1
    fi

    # Do not run the binary with --help — iced apps may start a full GUI
    # attempt. Check interpreter path instead when patchelf is available.
    if command -v patchelf >/dev/null 2>&1; then
      interp=$(patchelf --print-interpreter "$bin" 2>/dev/null || true)
      if [ -n "$interp" ] && [ ! -e "$interp" ]; then
        log "FATAL: missing dynamic linker $interp for $bin"
        sleep 3600
        exit 1
      fi
    fi
    log "binary ok: $bin"

    # Keep the product path alive: cage exits when the client dies (or when
    # DRM disappears). Always re-launch; never fall through to a text getty.
    attempt=0
    while true; do
      attempt=$((attempt + 1))
      log "starting cage + $bin (attempt $attempt)"
      set +e
      # cage: kiosk compositor for one app only (no shell / launcher / switcher).
      #   -d  no client-side decorations
      #   omit -s  so VT switching (Ctrl+Alt+F*) is not allowed
      ${pkgs.dbus}/bin/dbus-run-session -- \
        ${pkgs.cage}/bin/cage -d -- "$bin" \
        > /tmp/sola-install-cage.log 2>&1
      ec=$?
      set -e
      log "cage exited $ec — last log lines:"
      ${pkgs.coreutils}/bin/tail -n 40 /tmp/sola-install-cage.log \
        > /dev/ttyS0 2>/dev/null || true
      ${pkgs.coreutils}/bin/tail -n 40 /tmp/sola-install-cage.log \
        | ${pkgs.util-linux}/bin/logger -t sola-install-cage || true
      ${pkgs.coreutils}/bin/sleep 1
    done
  '';
in
{
  # DRM render node access (mesa /dev/dri/renderD*).
  users.groups.render = { };

  users.users.sola-install = {
    isNormalUser = true;
    description = "Sola installer live session";
    group = "users";
    home = "/home/sola-install";
    createHome = true;
    extraGroups = [
      "video"
      "input"
      "render"
      "wheel"
    ];
  };

  # Free tty1 for the kiosk — no text login on the product display.
  systemd.services."getty@tty1".enable = lib.mkForce false;
  systemd.services."autovt@tty1".enable = lib.mkForce false;

  systemd.services.sola-gpu-ready = {
    description = "Wait for installer GPU (non-simple DRM)";
    wantedBy = [ "multi-user.target" ];
    before = [ "sola-install-kiosk.service" ];
    after = [ "systemd-modules-load.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = "${gpuReadyScript}";
    };
  };

  systemd.services.sola-install-kiosk = {
    description = "Sola installer (cage + sola-install)";
    wantedBy = [ "multi-user.target" ];
    # Do NOT After=multi-user.target — that deadlocks (Wants + After same target
    # → unit never starts → black screen after Plymouth).
    after = [
      "sola-gpu-ready.service"
      "systemd-user-sessions.service"
      "seatd.service"
      "plymouth-quit.service"
    ];
    wants = [
      "sola-gpu-ready.service"
      "seatd.service"
    ];
    requires = [ "sola-gpu-ready.service" ];
    conflicts = [
      "getty@tty1.service"
      "autovt@tty1.service"
    ];
    serviceConfig = {
      User = "sola-install";
      Group = "users";
      SupplementaryGroups = [
        "video"
        "input"
        "render"
      ];
      PAMName = "login";
      TTYPath = "/dev/tty1";
      TTYReset = "yes";
      TTYVHangup = "yes";
      # Keep the VT across restarts so we don't flash a login prompt.
      TTYVTDisallocate = "no";
      StandardInput = "tty";
      StandardOutput = "journal";
      StandardError = "journal";
      UtmpIdentifier = "tty1";
      ExecStart = "${sessionScript}";
      # Safety net if the outer loop ever dies.
      Restart = "always";
      RestartSec = "1";
      Environment = [
        "XDG_SESSION_TYPE=wayland"
        "XDG_SESSION_CLASS=user"
      ];
    };
  };

  hardware.graphics.enable = true;
  hardware.bluetooth.enable = true;
  hardware.bluetooth.powerOnBoot = true;
  hardware.bluetooth.settings.General.Experimental = true;
  services.seatd.enable = true;

  environment.systemPackages = with pkgs; [
    cage
    mesa
    mesa-demos
    libglvnd
    plymouth
  ];

  # Serial keeps a normal getty for engineering recovery (mon:stdio).
  # Graphical VTs must not present a login screen on the product path.
  services.getty.autologinUser = lib.mkForce null;
  services.getty.helpLine = lib.mkForce "";
  environment.etc."issue".text = lib.mkForce "";

  systemd.defaultUnit = "multi-user.target";
}
