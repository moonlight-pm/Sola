#!/usr/bin/env bash
# Drive a single sola-browser run and capture per-process
# CPU / RSS / FPS samples for the WPE engine path.
#
# Usage:
#   bench/run-bench.sh https://slate.auto 30 docs/notes/data/run1
#
# The script kills any other sola-browser processes before starting
# so samples are not contaminated by a leftover window.

set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "usage: $0 <url> <duration_s> [<out_dir>]" >&2
    exit 2
fi

URL="$1"
DURATION="$2"
OUT_DIR="${3:-docs/notes/data/bench-$(date +%Y%m%d-%H%M%S)}"

BIN="target/debug/sola-browser"
if [[ ! -x "$BIN" ]]; then
    # Prefer installed binary when building via cargo make install path.
    if [[ -x /opt/sola/bin/sola-browser ]]; then
        BIN=/opt/sola/bin/sola-browser
    else
        echo "build first: cargo make build sola-browser" >&2
        exit 1
    fi
fi

mkdir -p "$OUT_DIR"
echo "out: $OUT_DIR"
echo "bin: $BIN"
echo "url: $URL"
echo "duration: ${DURATION}s"

pkill -9 sola-browser 2>/dev/null || true
pkill -9 sola-browser-wpe 2>/dev/null || true
pkill -9 sola-browser-cef 2>/dev/null || true
sleep 0.5

# Launch browser with the URL.
"$BIN" "$URL" >"$OUT_DIR/browser.stdout" 2>"$OUT_DIR/browser.stderr" &
BROWSER_PID=$!
echo "browser pid: $BROWSER_PID"

# Sample tree CPU / RSS for duration.
SAMPLES="$OUT_DIR/samples.csv"
echo "t_s,cpu_pct,rss_kib" >"$SAMPLES"
START=$(date +%s)
while true; do
    NOW=$(date +%s)
    ELAPSED=$((NOW - START))
    if (( ELAPSED >= DURATION )); then
        break
    fi
    if ! kill -0 "$BROWSER_PID" 2>/dev/null; then
        echo "browser exited early" >&2
        break
    fi
    # Sum RSS and CPU across process tree (browser + WebKit workers).
    # shellcheck disable=SC2009
    read -r cpu rss < <(
        ps -o %cpu=,rss= --ppid "$BROWSER_PID" -p "$BROWSER_PID" 2>/dev/null \
            | awk '{c+=$1; r+=$2} END {printf "%.1f %d\n", c+0, r+0}'
    )
    echo "${ELAPSED},${cpu:-0},${rss:-0}" >>"$SAMPLES"
    sleep 1
done

kill "$BROWSER_PID" 2>/dev/null || true
wait "$BROWSER_PID" 2>/dev/null || true
pkill -9 sola-browser 2>/dev/null || true
pkill -9 sola-browser-wpe 2>/dev/null || true
pkill -9 sola-browser-cef 2>/dev/null || true

echo "done → $OUT_DIR"
