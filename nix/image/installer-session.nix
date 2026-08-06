# Live installer session: multi-user → systemd kiosk on tty1 → cage + sola-install.
#
# Guest binaries are patchelf'd in sola-from-stage.nix so cage can exec them.
# sola-install runs standalone (no river/bus) under cage.

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
    export SOLA_NO_SELF_WATCH=1
    export WGPU_BACKEND="''${WGPU_BACKEND:-gl}"
    export LIBGL_ALWAYS_SOFTWARE="''${LIBGL_ALWAYS_SOFTWARE:-1}"
    export WLR_RENDERER="''${WLR_RENDERER:-pixman}"
    export WINIT_UNIX_BACKEND=wayland
    export XDG_SESSION_TYPE=wayland
    export XDG_CURRENT_DESKTOP=cage
    export XDG_DATA_DIRS="/opt/sola/share:''${XDG_DATA_DIRS:-/run/current-system/sw/share}"

    bin="${installBin}"
    if [ ! -x "$bin" ]; then
      bin="/opt/sola/bin/sola-install"
    fi
    if [ ! -x "$bin" ]; then
      log "FATAL: sola-install missing"
      sleep 3600
      exit 1
    fi

    # Prove the guest can exec the binary before cage.
    if ! "$bin" --help >/dev/null 2>&1 && ! ${pkgs.coreutils}/bin/test -x "$bin"; then
      log "FATAL: cannot exec $bin"
      ${pkgs.coreutils}/bin/ls -la "$bin" > /dev/ttyS0 2>/dev/null || true
      sleep 3600
      exit 1
    fi
    log "binary ok: $bin"

    log "starting cage + $bin"
    if ! ${pkgs.dbus}/bin/dbus-run-session -- \
        ${pkgs.cage}/bin/cage -s -- "$bin" \
        > /tmp/sola-install-cage.log 2>&1
    then
      log "cage exited $? — last log lines:"
      ${pkgs.coreutils}/bin/tail -n 50 /tmp/sola-install-cage.log \
        > /dev/ttyS0 2>/dev/null || true
      sleep 5
      exit 1
    fi
    log "cage exited 0"
  '';
in
{
  users.users.sola-install = {
    isNormalUser = true;
    description = "Sola installer live session";
    group = "users";
    home = "/home/sola-install";
    createHome = true;
    extraGroups = [
      "video"
      "input"
      "wheel"
    ];
  };

  # Free tty1 for the kiosk.
  systemd.services."getty@tty1".enable = false;

  systemd.services.sola-install-kiosk = {
    description = "Sola installer (cage + sola-install)";
    wantedBy = [ "multi-user.target" ];
    # Do NOT After=multi-user.target — that deadlocks (Wants + After same target
    # → unit never starts → black screen after Plymouth).
    after = [
      "systemd-user-sessions.service"
      "seatd.service"
    ];
    wants = [ "seatd.service" ];
    serviceConfig = {
      User = "sola-install";
      Group = "users";
      SupplementaryGroups = [
        "video"
        "input"
      ];
      PAMName = "login";
      TTYPath = "/dev/tty1";
      TTYReset = "yes";
      TTYVHangup = "yes";
      TTYVTDisallocate = "yes";
      StandardInput = "tty";
      StandardOutput = "journal";
      StandardError = "journal";
      UtmpIdentifier = "tty1";
      ExecStart = "${sessionScript}";
      Restart = "on-failure";
      RestartSec = "3";
      Environment = [
        "XDG_SESSION_TYPE=wayland"
        "XDG_SESSION_CLASS=user"
      ];
    };
  };

  hardware.graphics.enable = true;
  services.seatd.enable = true;

  environment.systemPackages = with pkgs; [
    cage
    mesa
    mesa-demos
    libglvnd
    plymouth
  ];

  # Serial keep a normal getty for recovery (not product path).
  services.getty.autologinUser = lib.mkForce null;
  services.getty.helpLine = lib.mkForce "";
  environment.etc."issue".text = lib.mkForce "";

  systemd.defaultUnit = "multi-user.target";
}
