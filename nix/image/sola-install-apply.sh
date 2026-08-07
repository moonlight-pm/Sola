#!/usr/bin/env bash
# Privileged whole-disk apply for sola-install.
#
# Usage: sola-install-apply --disk /dev/vdb --username alice --system /nix/store/...-nixos-system-sola
#
# Progress lines on stdout (parsed by the wizard):
#   PROGRESS <index> <label>
#   ERROR <message>
#   DONE

set -euo pipefail

DISK=""
USERNAME=""
SYSTEM=""
PROGRESS_FILE="${SOLA_INSTALL_PROGRESS:-/run/sola/install-progress}"

log() {
  echo "sola-install-apply: $*" >&2
  echo "sola-install-apply: $*" > /dev/ttyS0 2>/dev/null || true
}

progress() {
  local idx="$1"
  shift
  local label="$*"
  echo "PROGRESS $idx $label"
  mkdir -p "$(dirname "$PROGRESS_FILE")"
  printf '%s\t%s\n' "$idx" "$label" > "$PROGRESS_FILE"
  log "[$idx] $label"
}

die() {
  echo "ERROR $*"
  log "ERROR: $*"
  exit 1
}

while [ $# -gt 0 ]; do
  case "$1" in
    --disk) DISK="$2"; shift 2 ;;
    --username) USERNAME="$2"; shift 2 ;;
    --system) SYSTEM="$2"; shift 2 ;;
    -h|--help)
      echo "usage: sola-install-apply --disk DEV --username NAME --system STORE_PATH"
      exit 0
      ;;
    *) die "unknown arg: $1" ;;
  esac
done

[ "$(id -u)" = "0" ] || die "must run as root (sudo)"
[ -n "$DISK" ] || die "missing --disk"
[ -n "$USERNAME" ] || die "missing --username"
[ -n "$SYSTEM" ] || die "missing --system"
[ -b "$DISK" ] || die "not a block device: $DISK"
[ -e "$SYSTEM" ] || die "system path missing: $SYSTEM"

# Username rules (mirror crates/sola-install username.rs lightly).
if ! echo "$USERNAME" | grep -Eq '^[a-z_][a-z0-9_-]*$'; then
  die "invalid username: $USERNAME"
fi
if [ "${#USERNAME}" -gt 32 ]; then
  die "username too long"
fi
case "$USERNAME" in
  root|sola-install|nobody|daemon) die "reserved username: $USERNAME" ;;
esac

# Resolve live root's parent disk — never wipe that.
live_src=$(findmnt -n -o SOURCE / 2>/dev/null || true)
live_disk=""
if [ -n "$live_src" ]; then
  # SOURCE may be /dev/vda2 or /dev/disk/by-label/...
  live_src=$(readlink -f "$live_src" 2>/dev/null || echo "$live_src")
  if [ -b "$live_src" ]; then
    pk=$(lsblk -no PKNAME "$live_src" 2>/dev/null | head -1 || true)
    if [ -n "$pk" ]; then
      live_disk="/dev/$pk"
    else
      # Whole disk mounted? unusual.
      live_disk="$live_src"
    fi
  fi
fi
live_disk=$(readlink -f "$live_disk" 2>/dev/null || echo "$live_disk")
target=$(readlink -f "$DISK" 2>/dev/null || echo "$DISK")

if [ -n "$live_disk" ] && [ "$target" = "$live_disk" ]; then
  die "refusing to erase the live system disk ($target)"
fi

# Refuse if any partition of target is mounted.
if lsblk -no MOUNTPOINT "$DISK" 2>/dev/null | grep -q '[^[:space:]]'; then
  die "disk has mounted partitions: $DISK"
fi

part_names() {
  local d="$1"
  case "$d" in
    *[0-9]) echo "${d}p1" "${d}p2" ;; # nvme0n1, mmcblk0
    *) echo "${d}1" "${d}2" ;;
  esac
}

# shellcheck disable=SC2046
set -- $(part_names "$DISK")
P1="$1"
P2="$2"

progress 0 "Preparing disk…"

# Drop any old signatures and repartition GPT: ESP + root.
wipefs -a "$DISK" 2>/dev/null || true
sgdisk --zap-all "$DISK"
sgdisk -n1:0:+512M -t1:EF00 -c1:SOLA-ESP "$DISK"
sgdisk -n2:0:0 -t2:8300 -c2:sola-root "$DISK"
partprobe "$DISK" 2>/dev/null || true
udevadm settle 2>/dev/null || sleep 1

# Wait for partition nodes.
for i in $(seq 1 50); do
  [ -b "$P1" ] && [ -b "$P2" ] && break
  sleep 0.1
done
[ -b "$P1" ] || die "partition missing: $P1"
[ -b "$P2" ] || die "partition missing: $P2"

mkfs.vfat -F32 -n SOLA-ESP "$P1"
mkfs.ext4 -F -L sola-root "$P2"

progress 1 "Mounting…"

mkdir -p /mnt
mount "$P2" /mnt
mkdir -p /mnt/boot
mount "$P1" /mnt/boot

cleanup() {
  umount -R /mnt 2>/dev/null || true
}
trap cleanup EXIT

progress 2 "Writing system…"

# Offline install from a prebuilt toplevel already in the installer store.
export NIXOS_INSTALL_BOOTLOADER=1
set +e
nixos-install \
  --root /mnt \
  --system "$SYSTEM" \
  --no-root-passwd \
  --no-channel-copy \
  --no-write-lock-file \
  > /tmp/sola-nixos-install.log 2>&1
ni_ec=$?
set -e
if [ "$ni_ec" -ne 0 ]; then
  tail -n 40 /tmp/sola-nixos-install.log > /dev/ttyS0 2>/dev/null || true
  die "nixos-install failed (exit $ni_ec; see /tmp/sola-nixos-install.log)"
fi
log "nixos-install ok"

progress 3 "Creating user…"

mkdir -p /mnt/etc/sola
printf '%s\n' "$USERNAME" > /mnt/etc/sola/install-user
chmod 644 /mnt/etc/sola/install-user

# Create the user inside the target so first boot has a home dir.
# Use coreutils `id` (shadow does not ship id — that bug caused post-install
# session restart loops).
if nixos-enter --root /mnt -- /run/current-system/sw/bin/id "$USERNAME" >/dev/null 2>&1 \
  || nixos-enter --root /mnt -- id "$USERNAME" >/dev/null 2>&1; then
  log "user $USERNAME already exists in target"
else
  set +e
  nixos-enter --root /mnt -- useradd -m -U -G wheel,video,input,render,seat \
    -s /run/current-system/sw/bin/bash "$USERNAME" \
    > /tmp/sola-useradd.log 2>&1
  ua=$?
  if [ "$ua" -ne 0 ]; then
    nixos-enter --root /mnt -- useradd -m -U -G wheel,video,input,render \
      -s /run/current-system/sw/bin/bash "$USERNAME" \
      > /tmp/sola-useradd.log 2>&1
    ua=$?
  fi
  if [ "$ua" -ne 0 ]; then
    nixos-enter --root /mnt -- useradd -m \
      -s /run/current-system/sw/bin/bash "$USERNAME" \
      > /tmp/sola-useradd.log 2>&1
    ua=$?
  fi
  set -e
  if [ "$ua" -ne 0 ]; then
    tail -n 20 /tmp/sola-useradd.log > /dev/ttyS0 2>/dev/null || true
    # Non-fatal: sola-desktop will try again on first boot.
    log "WARN: useradd failed (ec=$ua); desktop session will retry"
  else
    nixos-enter --root /mnt -- passwd -d "$USERNAME" || true
    log "created user $USERNAME in target"
  fi
fi

progress 4 "Installing bootloader…"
# nixos-install normally installs the bootloader; re-run switch if needed.
if [ -x /mnt/nix/var/nix/profiles/system/bin/switch-to-configuration ]; then
  nixos-enter --root /mnt -- /nix/var/nix/profiles/system/bin/switch-to-configuration boot \
    || log "switch-to-configuration boot returned non-zero (may be ok)"
fi

progress 5 "Finishing…"
sync
trap - EXIT
umount -R /mnt
sync

progress 6 "Done"
echo "DONE"
log "install complete for $USERNAME on $DISK"
exit 0
