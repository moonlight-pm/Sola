# Loginless desktop: tty1 → sola as the username chosen at install.
#
# Apply writes /etc/sola/install-user. This unit ensures the account exists,
# then runs sola as that user via runuser (not setpriv/nested bash — that
# path was exiting immediately with no user-session logs).

{
  config,
  lib,
  pkgs,
  ...
}:

let
  sessionScript = pkgs.writeShellScript "sola-desktop-session" ''
    set -euo pipefail

    log() {
      echo "sola-desktop: $*" | ${pkgs.util-linux}/bin/logger -t sola-desktop || true
      echo "sola-desktop: $*" > /dev/ttyS0 2>/dev/null || true
      echo "sola-desktop: $*" > /dev/console 2>/dev/null || true
    }

    user_file=/etc/sola/install-user
    if [ ! -r "$user_file" ]; then
      log "FATAL: missing $user_file — install incomplete"
      sleep 3600
      exit 1
    fi
    user=$(${pkgs.coreutils}/bin/tr -d '[:space:]' < "$user_file")
    if [ -z "$user" ]; then
      log "FATAL: empty install-user"
      sleep 3600
      exit 1
    fi

    log "session for user=$user"

    id_bin=${pkgs.coreutils}/bin/id
    useradd_bin=${pkgs.shadow}/bin/useradd
    usermod_bin=${pkgs.shadow}/bin/usermod
    passwd_bin=${pkgs.shadow}/bin/passwd
    groupadd_bin=${pkgs.shadow}/bin/groupadd
    runuser_bin=${pkgs.util-linux}/bin/runuser

    ensure_group() {
      local g="$1"
      if ! ${pkgs.gnugrep}/bin/grep -q "^''${g}:" /etc/group 2>/dev/null; then
        log "creating group $g"
        "$groupadd_bin" "$g" 2>/dev/null || true
      fi
    }
    # DRM / seatd / input
    ensure_group wheel
    ensure_group video
    ensure_group input
    ensure_group render
    ensure_group seat

    shell=/run/current-system/sw/bin/bash
    if [ ! -x "$shell" ]; then
      shell=${pkgs.bashInteractive}/bin/bash
    fi

    if ! "$id_bin" "$user" >/dev/null 2>&1; then
      log "creating user $user"
      set +e
      out=$("$useradd_bin" -m -U -G wheel,video,input,render,seat -s "$shell" "$user" 2>&1)
      ec=$?
      if [ "$ec" -ne 0 ]; then
        log "useradd failed ec=$ec: $out"
        out=$("$useradd_bin" -m -G wheel,video,input,render,seat -s "$shell" "$user" 2>&1)
        ec=$?
      fi
      set -e
      if [ "$ec" -ne 0 ] && ! "$id_bin" "$user" >/dev/null 2>&1; then
        log "FATAL: could not create user: $out"
        sleep 3600
        exit 1
      fi
      "$passwd_bin" -d "$user" 2>/dev/null || true
      log "user created uid=$("$id_bin" -u "$user")"
    else
      log "user already exists uid=$("$id_bin" -u "$user") gid=$("$id_bin" -g "$user")"
      "$usermod_bin" -aG wheel,video,input,render,seat "$user" 2>/dev/null || true
    fi

    ${pkgs.plymouth}/bin/plymouth quit 2>/dev/null || true

    uid=$("$id_bin" -u "$user")
    gid=$("$id_bin" -g "$user")
    home=$(${pkgs.gawk}/bin/awk -F: -v u="$user" '$1==u {print $6; exit}' /etc/passwd)
    if [ -z "''${home:-}" ]; then
      home="/home/$user"
    fi
    ${pkgs.coreutils}/bin/mkdir -p "$home"
    ${pkgs.coreutils}/bin/chown "$uid:$gid" "$home"

    runtime="/run/user/$uid"
    ${pkgs.coreutils}/bin/mkdir -p "$runtime"
    ${pkgs.coreutils}/bin/chown "$uid:$gid" "$runtime"
    ${pkgs.coreutils}/bin/chmod 700 "$runtime"

    # Bring up systemd --user for this uid so sola-session can use
    # `systemd-run --user --scope` (and apps get a normal user bus).
    # Loginless seats never hit logind, so this would otherwise never start.
    if [ ! -S "$runtime/systemd/private" ]; then
      log "starting user@$uid.service"
      ${pkgs.systemd}/bin/systemctl start "user@$uid.service" 2>/dev/null || true
      ${pkgs.systemd}/bin/loginctl enable-linger "$user" 2>/dev/null || true
      for _i in $(${pkgs.coreutils}/bin/seq 1 50); do
        [ -S "$runtime/systemd/private" ] && break
        ${pkgs.coreutils}/bin/sleep 0.1
      done
      if [ -S "$runtime/systemd/private" ]; then
        log "user systemd ready at $runtime/systemd/private"
      else
        log "WARN: user systemd still missing — sola-session will direct-spawn apps"
      fi
    fi

    # Writable logs for the seat user (activation may have created this as root).
    ${pkgs.coreutils}/bin/mkdir -p /opt/sola/log
    ${pkgs.coreutils}/bin/chmod 1777 /opt/sola/log 2>/dev/null || true

    bin=/opt/sola/bin/sola
    if [ ! -x "$bin" ]; then
      bin=/run/current-system/sw/bin/sola
    fi
    if [ ! -x "$bin" ]; then
      log "FATAL: sola binary missing (checked /opt/sola/bin/sola and system path)"
      ${pkgs.coreutils}/bin/ls -la /opt/sola/bin 2>/dev/null | ${pkgs.coreutils}/bin/head -n 30 > /dev/ttyS0 || true
      sleep 3600
      exit 1
    fi

    drm=""
    if [ -r /run/sola/drm-device ]; then
      drm=$(${pkgs.coreutils}/bin/cat /run/sola/drm-device)
    fi

    # Soft GL under QEMU; hardware elsewhere unless overridden.
    softgl="''${LIBGL_ALWAYS_SOFTWARE:-}"
    if [ -z "$softgl" ]; then
      if [ -e /sys/class/dmi/id/product_name ] \
        && ${pkgs.gnugrep}/bin/grep -qi qemu /sys/class/dmi/id/product_name 2>/dev/null; then
        softgl=1
      else
        softgl=0
      fi
    fi

    session_log="$home/sola-desktop.log"
    : > "$session_log"
    ${pkgs.coreutils}/bin/chown "$uid:$gid" "$session_log"

    log "starting $bin as $user (uid=$uid gid=$gid home=$home softgl=$softgl drm=''${drm:-none})"
    log "session log: $session_log"

    # Environment for river + sola (path must include /opt/sola/bin for
    # resolve_binary("river") fallbacks; preferred is same-dir as sola).
    export HOME="$home"
    export USER="$user"
    export LOGNAME="$user"
    export XDG_RUNTIME_DIR="$runtime"
    export XDG_SESSION_TYPE=tty
    export XDG_SESSION_CLASS=user
    export PATH="/opt/sola/bin:/run/current-system/sw/bin:''${PATH:-/usr/bin:/bin}"
    export WGPU_BACKEND="''${WGPU_BACKEND:-gl}"
    export LIBGL_ALWAYS_SOFTWARE="$softgl"
    export WLR_RENDERER="''${WLR_RENDERER:-pixman}"
    export LIBSEAT_BACKEND=seatd
    export RUST_BACKTRACE="''${RUST_BACKTRACE:-1}"
    export RUST_LOG="''${RUST_LOG:-info}"
    if [ -n "$drm" ]; then
      export WLR_DRM_DEVICES="$drm"
    fi

    # runuser (not setpriv): keeps supplementary groups (video/seat) via -u.
    # Stay in the foreground so systemd tracks sola; log exit to serial.
    set +e
    "$runuser_bin" -u "$user" -- \
      env \
        HOME="$home" \
        USER="$user" \
        LOGNAME="$user" \
        XDG_RUNTIME_DIR="$runtime" \
        XDG_SESSION_TYPE=tty \
        XDG_SESSION_CLASS=user \
        PATH="/opt/sola/bin:/run/current-system/sw/bin" \
        WGPU_BACKEND="$WGPU_BACKEND" \
        LIBGL_ALWAYS_SOFTWARE="$softgl" \
        WLR_RENDERER="$WLR_RENDERER" \
        LIBSEAT_BACKEND=seatd \
        RUST_BACKTRACE="$RUST_BACKTRACE" \
        RUST_LOG="$RUST_LOG" \
        WLR_DRM_DEVICES="''${drm:-}" \
        "$bin" \
      >>"$session_log" 2>&1
    ec=$?
    set -e

    log "sola exited $ec — last log lines:"
    ${pkgs.coreutils}/bin/tail -n 80 "$session_log" > /dev/ttyS0 2>/dev/null || true
    ${pkgs.coreutils}/bin/tail -n 80 "$session_log" | ${pkgs.util-linux}/bin/logger -t sola-desktop || true

    # Non-zero → systemd Restart=on-failure. Sleep a beat so serial is readable.
    if [ "$ec" -ne 0 ]; then
      ${pkgs.coreutils}/bin/sleep 2
    fi
    exit "$ec"
  '';

  gpuReadyScript = pkgs.writeShellScript "sola-desktop-gpu-ready" ''
    set -euo pipefail
    log() {
      echo "sola-desktop-gpu: $*" > /dev/ttyS0 2>/dev/null || true
    }
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
        break 2
      done
      ${pkgs.coreutils}/bin/sleep 0.1
    done
    if [ -z "$ready" ]; then
      for n in /dev/dri/card*; do
        [ -e "$n" ] || continue
        ready="$n"
        break
      done
    fi
    if [ -z "$ready" ]; then
      log "FATAL: no DRM"
      exit 1
    fi
    if [ "$found_real" = "1" ] && ${pkgs.kmod}/bin/lsmod | ${pkgs.gnugrep}/bin/grep -q '^simpledrm'; then
      ${pkgs.kmod}/bin/rmmod simpledrm 2>/dev/null || true
    fi
    ${pkgs.coreutils}/bin/mkdir -p /run/sola
    echo "$ready" > /run/sola/drm-device
    log "drm=$ready"
  '';
in
{
  users.groups.render = { };
  users.groups.input = { };
  # seatd ACL group (NixOS seatd grants this group access to the seat).
  users.groups.seat = { };

  environment.systemPackages = with pkgs; [
    bashInteractive
    seatd
  ];

  systemd.services."getty@tty1".enable = lib.mkForce false;
  systemd.services."autovt@tty1".enable = lib.mkForce false;

  systemd.services.sola-desktop-gpu-ready = {
    description = "Wait for desktop GPU (non-simple DRM)";
    wantedBy = [ "multi-user.target" ];
    before = [ "sola-desktop.service" ];
    after = [ "systemd-modules-load.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = "${gpuReadyScript}";
    };
  };

  systemd.services.sola-desktop = {
    description = "Sola desktop (loginless)";
    wantedBy = [ "multi-user.target" ];
    after = [
      "sola-desktop-gpu-ready.service"
      "systemd-user-sessions.service"
      "seatd.service"
      "plymouth-quit.service"
    ];
    wants = [
      "sola-desktop-gpu-ready.service"
      "seatd.service"
    ];
    requires = [ "sola-desktop-gpu-ready.service" ];
    conflicts = [
      "getty@tty1.service"
      "autovt@tty1.service"
    ];
    serviceConfig = {
      TTYPath = "/dev/tty1";
      TTYReset = "yes";
      TTYVHangup = "yes";
      TTYVTDisallocate = "no";
      StandardInput = "tty";
      StandardOutput = "journal";
      StandardError = "journal";
      UtmpIdentifier = "tty1";
      ExecStart = "${sessionScript}";
      Restart = "on-failure";
      RestartSec = "3";
      StartLimitIntervalSec = 120;
      StartLimitBurst = 15;
    };
  };

  hardware.graphics.enable = true;
  services.seatd.enable = true;

  services.getty.autologinUser = lib.mkForce null;
  services.getty.helpLine = lib.mkForce "";
  environment.etc."issue".text = lib.mkForce "";

  systemd.defaultUnit = "multi-user.target";
}
