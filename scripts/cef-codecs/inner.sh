#!/usr/bin/env bash
# Runs inside Ubuntu 22.04 (podman). Builds CEF 147 with H.264/AAC.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
export GN_DEFINES="${GN_DEFINES:-is_official_build=true proprietary_codecs=true ffmpeg_branding=Chrome symbol_level=0 blink_symbol_level=0 chrome_pgo_phase=0 use_vaapi=false}"
export CEF_ARCHIVE_FORMAT="${CEF_ARCHIVE_FORMAT:-tar.bz2}"
# Only generate the release x64 ninja tree (skip Debug_GN_*).
export GN_OUT_CONFIGS="${GN_OUT_CONFIGS:-Release_GN_x64}"

SRC=/build/src
mkdir -p "$SRC"
cd /build

echo "[cef-codecs] apt"
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
  ca-certificates curl git python3 python3-requests \
  lsb-release file sudo xz-utils bzip2 unzip pkg-config \
  build-essential python-is-python3 \
  libcups2-dev libgtk-3-dev libnss3-dev libxss-dev \
  libasound2-dev libgbm-dev libdrm-dev libxkbcommon-dev \
  libpango1.0-dev libatk1.0-dev libatk-bridge2.0-dev \
  libx11-xcb-dev libxtst-dev libpulse-dev \
  flex bison gperf libpci-dev libcurl4-gnutls-dev \
  libkrb5-dev libcap-dev libudev-dev libffi-dev libssl-dev

# Chromium gn exec_scripts (cups-config, pkg-config, …) need the host
# toolchain packages. install-build-deps is the canonical list.
if [[ -f /build/src/chromium/src/build/install-build-deps.py ]]; then
  echo "[cef-codecs] chromium install-build-deps.py"
  python3 /build/src/chromium/src/build/install-build-deps.py \
    --no-prompt --no-chromeos-fonts --unsupported || true
fi

# Chromium's install-build-deps wants a git identity for some hooks.
git config --global user.email "sola-cef@localhost" || true
git config --global user.name "sola-cef" || true
git config --global --add safe.directory '*' || true

if [[ ! -f /build/automate-git.py ]]; then
  echo "[cef-codecs] fetching automate-git.py"
  curl -fsSL -o /build/automate-git.py \
    https://raw.githubusercontent.com/chromiumembedded/cef/7727/tools/automate/automate-git.py
fi

if [[ ! -d /build/depot_tools/.git ]]; then
  echo "[cef-codecs] cloning depot_tools"
  git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git /build/depot_tools
fi
# cipd/tar of node sometimes lands without +x (PermissionError in node.py).
if [[ -e /build/src/chromium/src/third_party/node/linux/node-linux-x64/bin/node ]]; then
  echo "[cef-codecs] chmod +x third_party/node"
  chmod +x /build/src/chromium/src/third_party/node/linux/node-linux-x64/bin/* || true
fi

echo "[cef-codecs] depot_tools ensure_bootstrap"
# Creates python3_bin_reldir.txt so `gn` can run. update_depot_tools
# alone is not enough on a fresh clone.
(cd /build/depot_tools && ./ensure_bootstrap)

# Pin matches workspace cef-version (147.0.10+gd58e84d+chromium-147.0.7727.118).
CEF_CHECKOUT="${CEF_CHECKOUT:-d58e84d17dd3f646c906ac633156cd0ec46638e9}"
CEF_BRANCH="${CEF_BRANCH:-7727}"

export PATH="/build/depot_tools:${PATH}"
OUT=/build/src/chromium/src/out/Release_GN_x64

if [[ -f "$OUT/args.gn" ]]; then
  echo "[cef-codecs] resume ninja (existing $OUT)"
  # Parallel gen_xproto can leave 0-byte xproto.h; ninja then compiles
  # against an empty header (missing x11::Window).
  if [[ -d "$OUT/gen/ui/gfx/x" ]]; then
    find "$OUT/gen/ui/gfx/x" -size 0 \( -name '*.h' -o -name '*.cc' \) -print -delete || true
  fi
  rm -f "$OUT"/obj/ui/gfx/x/build_xprotos/*.o || true
  # Ubuntu 22.04 libva is too old for Chromium 147's AV1 VAAPI encoder
  # (no refresh_frame_flags). We want ffmpeg software H.264/AAC anyway.
  if ! grep -q '^use_vaapi=' "$OUT/args.gn"; then
    printf '\nuse_vaapi=false\n' >> "$OUT/args.gn"
  fi
  (cd /build/src/chromium/src && gn gen out/Release_GN_x64 && autoninja -C out/Release_GN_x64 cefsimple chrome_sandbox)
  mkdir -p /build/src/chromium/src/cef/binary_distrib
  python3 /build/src/chromium/src/cef/tools/make_distrib.py \
    --output-dir=/build/src/chromium/src/cef/binary_distrib \
    --allow-partial --ninja-build --x64-build --minimal --no-docs --no-symbols
else
  EXTRA=(--no-chromium-history --force-build)
  if [[ -e /build/src/chromium/src/chrome/VERSION ]]; then
    echo "[cef-codecs] existing chromium checkout — --no-update"
    EXTRA=(--no-update --force-build)
  fi
  echo "[cef-codecs] GN_DEFINES=$GN_DEFINES"
  echo "[cef-codecs] GN_OUT_CONFIGS=$GN_OUT_CONFIGS"
  echo "[cef-codecs] checkout=$CEF_CHECKOUT branch=$CEF_BRANCH"
  echo "[cef-codecs] starting automate-git.py (hours)"
  python3 /build/automate-git.py \
    --download-dir="$SRC" \
    --depot-tools-dir=/build/depot_tools \
    --branch="$CEF_BRANCH" \
    --checkout="$CEF_CHECKOUT" \
    --x64-build \
    --no-debug-build \
    --build-target=cefsimple \
    --minimal-distrib-only \
    --no-distrib-docs \
    --no-distrib-symbols \
    "${EXTRA[@]}"
fi

echo "[cef-codecs] automate-git finished"
ls -la "$SRC/chromium/src/cef/binary_distrib/" || true
find "$SRC/chromium/src/cef/binary_distrib" -name 'cef_binary_*linux64_minimal*' | head
echo DONE
