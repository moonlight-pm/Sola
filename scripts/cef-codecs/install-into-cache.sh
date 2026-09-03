#!/usr/bin/env bash
# Copy a codecs-enabled CEF minimal distrib into ~/.cache/sola/cef-<ver>/
# and re-run the NixOS patchelf step. Safe to run after build.sh finishes.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_DIR="${CEF_BUILD_DIR:-$HOME/.cache/sola/cef-build}"
VER="$(tr -d '\n' < "$ROOT/cef-version")"
DEST="${CEF_CACHE:-$HOME/.cache/sola/cef-$VER}"

distrib=$(find "$BUILD_DIR/src/chromium/src/cef/binary_distrib" \
  -maxdepth 2 -type d -name 'cef_binary_*_linux64_minimal' \
  | head -1 || true)
if [[ -z "$distrib" ]]; then
  echo "no linux64_minimal distrib under $BUILD_DIR" >&2
  echo "looked at $BUILD_DIR/src/chromium/src/cef/binary_distrib" >&2
  ls -la "$BUILD_DIR/src/chromium/src/cef/binary_distrib" 2>/dev/null || true
  exit 1
fi

echo "[cef-codecs] source $distrib"
echo "[cef-codecs] dest   $DEST"

# Keep a copy of the no-codecs tree once, then replace in place so
# sola-browser build.rs still resolves ~/.cache/sola/cef-<ver>.
if [[ -d "$DEST/Release" && ! -d "${DEST}.no-codecs" ]]; then
  echo "[cef-codecs] backing up $DEST -> ${DEST}.no-codecs"
  mv "$DEST" "${DEST}.no-codecs"
fi

rm -rf "$DEST"
mkdir -p "$DEST"
# Copy the extracted minimal tree (Release/, Resources/, cmake/, …).
cp -a "$distrib"/. "$DEST"/

# cargo make install-cef no-ops when libcef.so exists — patch here.
NIX_LD=/run/current-system/sw/share/nix-ld/lib
OPENGL=/run/opengl-driver/lib
if command -v patchelf >/dev/null; then
  for so in "$DEST"/Release/lib*.so "$DEST"/Release/lib*.so.*; do
    [[ -f "$so" ]] || continue
    echo "[cef-codecs] patchelf $so"
    patchelf --add-rpath "\$ORIGIN:${NIX_LD}:${OPENGL}" "$so" || true
  done
fi

# Resources next to libcef (DIR_MODULE).
if [[ -d "$DEST/Resources" ]]; then
  for e in "$DEST/Resources"/*; do
    name=$(basename "$e")
    dest="$DEST/Release/$name"
    if [[ ! -e "$dest" ]]; then
      ln -s "../Resources/$name" "$dest"
      echo "[cef-codecs] link $dest"
    fi
  done
fi

echo "[cef-codecs] installed $DEST"
echo "[cef-codecs] next: cargo make install browser --release"
