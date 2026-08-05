#!/usr/bin/env bash
# One-shot ACL grant for sola-kvm when udev/uaccess is not yet in place.
#
# Correct form (always quote the -m argument; expand username via id):
#   sudo ./grant-input-acl.sh
#
# Wrong forms that produce "Invalid argument near character 3":
#   setfacl -m u:$USER:rw- …     # if $USER is empty → u::rw-
#   setfacl -m u::rw- …          # empty username is invalid
set -euo pipefail

USER_NAME="$(id -un)"
UID_NUM="$(id -u)"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "re-exec under sudo…" >&2
  exec sudo -E env "SUDO_USER=${USER_NAME}" "$0" "$@"
fi

# Prefer the invoking user, not root.
TARGET_USER="${SUDO_USER:-$USER_NAME}"
if [[ "$TARGET_USER" == "root" ]]; then
  echo "error: refuse to ACL as root; run: sudo -u <you> $0  or  sudo $0 from your login" >&2
  exit 1
fi

# Verify the user exists
if ! id "$TARGET_USER" >/dev/null 2>&1; then
  echo "error: unknown user: $TARGET_USER" >&2
  exit 1
fi

# ACL entry: use "rw" (setfacl normalizes to rw-). Keep the whole -m value quoted.
ACL_SPEC="u:${TARGET_USER}:rw"

shopt -s nullglob
nodes=(/dev/input/event[0-9]*)
if [[ ${#nodes[@]} -eq 0 ]]; then
  echo "error: no /dev/input/event* nodes" >&2
  exit 1
fi

echo "granting ${ACL_SPEC} on ${#nodes[@]} event nodes…"
setfacl -m "${ACL_SPEC}" "${nodes[@]}"
echo "ok. sample:"
getfacl -p "${nodes[0]}" | head -12
echo
echo "restart sola-kvm so it re-opens devices:"
echo "  killall sola-kvm   # sola relaunches it"
