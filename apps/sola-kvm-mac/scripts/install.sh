#!/usr/bin/env bash
# Build (optional), package as a signed .app, and install the LaunchAgent.
#
# Signing strategy (stable Accessibility across rebuilds):
#   1. Prefer Apple Development identity from login keychain (if usable).
#   2. Else use a dedicated file keychain + self-signed "SolaKvmMac Code Signing"
#      cert (works over SSH; same cert ⇒ TCC grant sticks).
#   Never ad-hoc re-sign for production installs — that changes the CDHash every
#   build and forces Accessibility re-grant.
#
#   ./scripts/install.sh
#   ./scripts/install.sh --no-build
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LABEL="com.sola.kvm-mac"
APP_NAME="SolaKvmMac.app"
APP_DIR="${SOLA_KVM_MAC_APP:-/Applications/${APP_NAME}}"
BIN_NAME="sola-kvm-mac"
BIND="0.0.0.0:4242"
DO_BUILD=1
BUNDLE_ID="com.sola.kvm-mac"
CERT_CN="SolaKvmMac Code Signing"
KC_PATH="${HOME}/Library/Keychains/sola-codesign.keychain-db"
KC_PASS="${SOLA_CODESIGN_KEYCHAIN_PASS:-sola-kvm-mac-local}"

usage() {
  cat <<EOF
Usage: $0 [--no-build] [--bind 0.0.0.0:4242] [--app /Applications/SolaKvmMac.app]

Packages sola-kvm-mac as ${APP_NAME}, codesigns with a stable identity, and
installs LaunchAgent ${LABEL}.

Env:
  CODESIGN_IDENTITY              force an identity name
  SOLA_KVM_MAC_APP               .app install path
  SOLA_CODESIGN_KEYCHAIN_PASS    password for dedicated keychain (default local)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) DO_BUILD=0; shift ;;
    --bind) BIND="$2"; shift 2 ;;
    --app) APP_DIR="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: must run on macOS (ember)" >&2
  exit 1
fi

if [[ "$DO_BUILD" -eq 1 ]]; then
  echo "building release…"
  (cd "$ROOT" && cargo build --release)
fi

SRC="$ROOT/target/release/${BIN_NAME}"
if [[ ! -x "$SRC" ]]; then
  echo "error: missing $SRC — build first or drop --no-build" >&2
  exit 1
fi

ensure_local_signing_cert() {
  if [[ ! -f "$KC_PATH" ]]; then
    security create-keychain -p "$KC_PASS" "$KC_PATH"
  fi
  security unlock-keychain -p "$KC_PASS" "$KC_PATH"
  security set-keychain-settings -t 21600 -u "$KC_PATH" || true

  if security find-identity -v -p codesigning "$KC_PATH" 2>/dev/null | grep -q "$CERT_CN"; then
    return 0
  fi
  # find-identity may show 0 "valid" for self-signed; check certificate presence.
  if security find-certificate -c "$CERT_CN" "$KC_PATH" &>/dev/null; then
    return 0
  fi

  echo "creating self-signed codesign cert «${CERT_CN}» in sola-codesign keychain…"
  local tmp
  tmp="$(mktemp -d)"
  cat >"$tmp/cert.cnf" <<CNF
[req]
distinguished_name = req_distinguished_name
prompt = no
[req_distinguished_name]
CN = ${CERT_CN}
[extensions]
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
basicConstraints = critical, CA:false
CNF
  openssl req -new -newkey rsa:2048 -nodes \
    -keyout "$tmp/key.pem" -out "$tmp/req.pem" -config "$tmp/cert.cnf" >/dev/null 2>&1
  openssl x509 -req -days 3650 -in "$tmp/req.pem" -signkey "$tmp/key.pem" \
    -out "$tmp/cert.pem" -extfile "$tmp/cert.cnf" -extensions extensions >/dev/null 2>&1
  openssl pkcs12 -export -out "$tmp/cert.p12" -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
    -passout pass:"$KC_PASS" -name "$CERT_CN" >/dev/null 2>&1
  security import "$tmp/cert.p12" -k "$KC_PATH" -P "$KC_PASS" \
    -T /usr/bin/codesign -T /usr/bin/security >/dev/null
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KC_PASS" "$KC_PATH" >/dev/null || true
  rm -rf "$tmp"
}

# Put our keychain first so codesign can find the identity over SSH.
security list-keychains -d user -s "$KC_PATH" login.keychain-db 2>/dev/null \
  || security list-keychains -s "$KC_PATH" login.keychain-db 2>/dev/null \
  || true

try_codesign() {
  local target="$1"
  local identity="$2"
  local keychain="${3:-}"
  if [[ -n "$keychain" ]]; then
    codesign --force --sign "$identity" \
      --identifier "$BUNDLE_ID" \
      --timestamp=none \
      --keychain "$keychain" \
      "$target" 2>/dev/null
  else
    codesign --force --sign "$identity" \
      --identifier "$BUNDLE_ID" \
      --timestamp=none \
      "$target" 2>/dev/null
  fi
}

MACOS_DIR="${APP_DIR}/Contents/MacOS"
mkdir -p "$MACOS_DIR"

cat >"${APP_DIR}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>${BIN_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleName</key>
  <string>Sola KVM Mac</string>
  <key>CFBundleDisplayName</key>
  <string>Sola KVM Mac</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHumanReadableCopyright</key>
  <string>Copyright © Sola</string>
</dict>
</plist>
PLIST

cp -f "$SRC" "${MACOS_DIR}/${BIN_NAME}"
chmod 755 "${MACOS_DIR}/${BIN_NAME}"

EXE="${MACOS_DIR}/${BIN_NAME}"
SIGNED_WITH=""

if [[ -n "${CODESIGN_IDENTITY:-}" ]]; then
  if try_codesign "$EXE" "$CODESIGN_IDENTITY" && try_codesign "$APP_DIR" "$CODESIGN_IDENTITY"; then
    SIGNED_WITH="$CODESIGN_IDENTITY"
  fi
fi

if [[ -z "$SIGNED_WITH" ]]; then
  DEV_ID="$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/.*"\(Apple Development:[^"]*\)"/\1/p' | head -1 || true)"
  if [[ -n "$DEV_ID" ]] && try_codesign "$EXE" "$DEV_ID" && try_codesign "$APP_DIR" "$DEV_ID"; then
    SIGNED_WITH="$DEV_ID"
  fi
fi

if [[ -z "$SIGNED_WITH" ]]; then
  ensure_local_signing_cert
  security unlock-keychain -p "$KC_PASS" "$KC_PATH"
  if try_codesign "$EXE" "$CERT_CN" "$KC_PATH" && try_codesign "$APP_DIR" "$CERT_CN" "$KC_PATH"; then
    SIGNED_WITH="$CERT_CN (sola-codesign keychain)"
  fi
fi

if [[ -z "$SIGNED_WITH" ]]; then
  echo "error: codesign failed (login keychain locked + local cert failed)" >&2
  exit 1
fi

echo "signed with: $SIGNED_WITH"
codesign -dv --verbose=2 "$APP_DIR" 2>&1 | sed -n '1,14p' || true

# Convenience path for docs / CLI (symlink to signed binary when possible).
OPT_LINK="/opt/sola/bin/sola-kvm-mac"
if [[ -d /opt/sola/bin ]] || mkdir -p /opt/sola/bin 2>/dev/null; then
  if ln -sfn "$EXE" "$OPT_LINK" 2>/dev/null \
    || sudo ln -sfn "$EXE" "$OPT_LINK" 2>/dev/null; then
    echo "symlink: $OPT_LINK → $EXE"
  elif cp -f "$EXE" "$OPT_LINK" 2>/dev/null || sudo cp -f "$EXE" "$OPT_LINK" 2>/dev/null; then
    # Copy breaks signature path identity — prefer symlink. Warn if we had to copy.
    echo "warning: copied binary to $OPT_LINK (symlink failed); LaunchAgent uses .app path"
  fi
fi

"$ROOT/scripts/install-launchagent.sh" --bin "$EXE" --bind "$BIND"

echo
echo "Done."
echo "  app:    $APP_DIR"
echo "  binary: $EXE"
echo
echo "Grant Accessibility once (remove stale /opt/sola/bin entries first):"
echo "  System Settings → Privacy & Security → Accessibility"
echo "  Enable «Sola KVM Mac» / ${BUNDLE_ID}"
echo
echo "Rebuilds: re-run ./scripts/install.sh — same signing cert keeps the grant."
