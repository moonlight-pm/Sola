#!/usr/bin/env bash
# Build a codecs-enabled CEF (H.264/AAC) via podman Ubuntu 22.04.
# Chromium's public tarball (cef-builds.spotifycdn.com) ships without
# proprietary codecs. Output lands in ~/.cache/sola/cef-build/ and is
# installed over ~/.cache/sola/cef-<version>/ by install-into-cache.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_DIR="${CEF_BUILD_DIR:-$HOME/.cache/sola/cef-build}"
IMAGE="${CEF_BUILD_IMAGE:-docker.io/library/ubuntu:22.04}"
LOG="${BUILD_DIR}/build.log"

mkdir -p "$BUILD_DIR"
cp -f "$ROOT/scripts/cef-codecs/inner.sh" "$BUILD_DIR/inner.sh"
chmod +x "$BUILD_DIR/inner.sh"

# Cap at half the host cores (desk stays usable). Override with CEF_BUILD_CPUS.
HOST_CPUS="$(nproc)"
CPUS="${CEF_BUILD_CPUS:-$(( HOST_CPUS / 2 ))}"
if [[ "$CPUS" -lt 1 ]]; then CPUS=1; fi

echo "[cef-codecs] build dir $BUILD_DIR"
echo "[cef-codecs] log $LOG"
echo "[cef-codecs] image $IMAGE"
echo "[cef-codecs] cpus $CPUS / $HOST_CPUS (nice 19)"

exec podman run --rm \
  --name sola-cef-codecs \
  --cpus="$CPUS" \
  --cpu-shares=512 \
  --network=host \
  --shm-size=4g \
  --security-opt seccomp=unconfined \
  -v "$BUILD_DIR:/build:Z" \
  -e DEBIAN_FRONTEND=noninteractive \
  -e GN_DEFINES \
  -e GN_OUT_CONFIGS \
  -e CEF_CHECKOUT \
  -e CEF_BRANCH \
  "$IMAGE" \
  bash /build/inner.sh
