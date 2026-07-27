#!/usr/bin/env bash
# Install sola-kvm-mac as a GUI-session LaunchAgent on ember (macOS).
# Run this from a normal Terminal.app / iTerm session after login — not SSH
# alone, so Accessibility TCC can attach to a GUI process tree.
set -euo pipefail

LABEL="com.sola.kvm-mac"
BIN_DEFAULT="/opt/sola/bin/sola-kvm-mac"
PLIST_SRC="$(cd "$(dirname "$0")/.." && pwd)/LaunchAgents/${LABEL}.plist"
PLIST_DST="${HOME}/Library/LaunchAgents/${LABEL}.plist"
LOG_DIR="${HOME}/Library/Logs"

usage() {
  cat <<EOF
Usage: $0 [--bin /path/to/sola-kvm-mac] [--bind 0.0.0.0:4242]

Installs ${LABEL} into ~/Library/LaunchAgents for the current GUI user.
EOF
}

BIN="$BIN_DEFAULT"
BIND="0.0.0.0:4242"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin) BIN="$2"; shift 2 ;;
    --bind) BIND="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: install-launchagent.sh must run on macOS (ember)" >&2
  exit 1
fi

if [[ ! -x "$BIN" ]]; then
  echo "error: binary not executable: $BIN" >&2
  echo "Build on ember first:" >&2
  echo "  cd apps/sola-kvm-mac && cargo build --release" >&2
  echo "  cp target/release/sola-kvm-mac $BIN" >&2
  exit 1
fi

mkdir -p "${HOME}/Library/LaunchAgents" "$LOG_DIR"
UID_NUM="$(id -u)"

# Generate a user-specific plist from the template paths.
cat >"$PLIST_DST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${BIN}</string>
    <string>listen</string>
    <string>--bind</string>
    <string>${BIND}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Interactive</string>
  <key>StandardOutPath</key>
  <string>${LOG_DIR}/sola-kvm-mac.out.log</string>
  <key>StandardErrorPath</key>
  <string>${LOG_DIR}/sola-kvm-mac.err.log</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>RUST_LOG</key>
    <string>info</string>
  </dict>
</dict>
</plist>
EOF

# Prefer modern launchctl bootstrap (gui domain) over legacy load.
if launchctl print "gui/${UID_NUM}/${LABEL}" &>/dev/null; then
  launchctl bootout "gui/${UID_NUM}/${LABEL}" || true
fi
launchctl bootstrap "gui/${UID_NUM}" "$PLIST_DST"
launchctl enable "gui/${UID_NUM}/${LABEL}"
launchctl kickstart -k "gui/${UID_NUM}/${LABEL}"

echo "installed ${LABEL}"
echo "  binary: ${BIN}"
echo "  bind:   ${BIND}"
echo "  plist:  ${PLIST_DST}"
echo "  logs:   ${LOG_DIR}/sola-kvm-mac.{out,err}.log"
echo
echo "Grant Accessibility:"
echo "  System Settings → Privacy & Security → Accessibility → enable sola-kvm-mac"
echo "Then from novus:"
echo "  sola-kvm send-test --to <ember-ip>:4242"
