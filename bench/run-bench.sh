#!/usr/bin/env bash
# Drive a single sola-browser-{wpe,cef} run and capture per-process
# CPU/RSS, GPU utilization/memory, and the shader-thread FPS counter
# at 1 Hz. Outputs three CSVs + a meta.json into the chosen directory.
#
# Usage:
#   bench/run-bench.sh <engine> <url> <duration_s> [<out_dir>]
#
# Examples:
#   bench/run-bench.sh wpe https://slate.auto 30
#   bench/run-bench.sh cef https://slate.auto 30 docs/notes/data/run1
#
# The script kills any other sola-browser processes before starting
# so the measurement isn't contaminated by leftovers. It DOES NOT
# touch sola-shell, sola-river, or other workspace services.

set -euo pipefail

if [[ $# -lt 3 ]]; then
    echo "usage: $0 <wpe|cef> <url> <duration_s> [<out_dir>]" >&2
    exit 2
fi

ENGINE="$1"
URL="$2"
DURATION="$3"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${4:-docs/notes/data/${TS}_${ENGINE}}"

case "$ENGINE" in
    wpe) BIN="crates/sola-browser-wpe/target/debug/sola-browser-wpe" ;;
    cef) BIN="crates/sola-browser-cef/target/debug/sola-browser-cef" ;;
    *)   echo "engine must be wpe or cef" >&2; exit 2 ;;
esac

if [[ ! -x "$BIN" ]]; then
    echo "binary not found: $BIN" >&2
    echo "build first: cargo make build sola-browser-${ENGINE}" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"
PROCS_CSV="$OUT_DIR/procs.csv"
GPU_CSV="$OUT_DIR/gpu.csv"
FPS_CSV="$OUT_DIR/fps.csv"
META="$OUT_DIR/meta.json"
APP_LOG="$OUT_DIR/app.log"

# Pre-flight: kill any lingering browser procs.
pkill -9 sola-browser-wpe 2>/dev/null || true
pkill -9 sola-browser-cef 2>/dev/null || true
pkill -9 WPEWebProcess 2>/dev/null || true
pkill -9 WPENetworkProce 2>/dev/null || true
sleep 1

# Mark a position in the workspace log so we only scrape FPS lines
# from this run, not historical ones.
LOG_FILE="${HOME}/Workspace/Sola/dev/null"  # default if /opt/sola/log not present
if [[ -f /opt/sola/log/sola.log ]]; then
    LOG_FILE=/opt/sola/log/sola.log
fi
LOG_MARK_BEFORE=$(wc -l < "$LOG_FILE" 2>/dev/null || echo 0)

START_EPOCH=$(date +%s)
START_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# Launch and capture pid. The browser binaries take the URL as
# their first positional argv (default falls back to a hardcoded
# constant in the binary, but we want the bench to control it).
"$BIN" "$URL" > "$APP_LOG" 2>&1 &
ROOT_PID=$!

# Initialize CSV headers.
echo "t_s,pid,ppid,cpu_pct,rss_kib,comm" > "$PROCS_CSV"
echo "t_s,util_pct,mem_used_mib" > "$GPU_CSV"

# Walk the process tree rooted at ROOT_PID and emit one row per pid.
# `pgrep` exits 1 when there are no matches and would kill the script
# under `set -e -o pipefail`, so every pgrep is suffixed with `|| true`.
sample_procs () {
    local t_s=$1
    local pids
    pids=$( { pgrep -P "$ROOT_PID" 2>/dev/null || true; } | tr '\n' ' ')
    local all_pids="$ROOT_PID"
    local frontier="$pids"
    while [[ -n "$frontier" ]]; do
        all_pids="$all_pids $frontier"
        local next=""
        for p in $frontier; do
            local kids
            kids=$( { pgrep -P "$p" 2>/dev/null || true; } | tr '\n' ' ')
            next="$next $kids"
        done
        frontier="$(echo "$next" | xargs || true)"
    done
    for pid in $all_pids; do
        if [[ -d /proc/$pid ]]; then
            local row
            row=$(ps -p "$pid" -o pid=,ppid=,%cpu=,rss=,comm= 2>/dev/null | sed 's/^ *//' | tr -s ' ' ',' || true)
            [[ -n "$row" ]] && echo "${t_s},${row}" >> "$PROCS_CSV"
        fi
    done
}

sample_gpu () {
    local t_s=$1
    if command -v nvidia-smi >/dev/null 2>&1; then
        # One sample, GPU 0, util.gpu + mem.used.
        local line
        line=$(nvidia-smi --query-gpu=utilization.gpu,memory.used \
                          --format=csv,noheader,nounits -i 0 2>/dev/null | tr -d ' ')
        [[ -n "$line" ]] && echo "${t_s},${line}" >> "$GPU_CSV"
    fi
}

# Sample loop.
END_EPOCH=$((START_EPOCH + DURATION))
while [[ $(date +%s) -lt $END_EPOCH ]]; do
    if ! kill -0 "$ROOT_PID" 2>/dev/null; then
        echo "root pid $ROOT_PID died early at t=$(($(date +%s) - START_EPOCH))s" >&2
        break
    fi
    T_S=$(($(date +%s) - START_EPOCH))
    sample_procs "$T_S"
    sample_gpu "$T_S"
    sleep 1
done

# Teardown.
kill -TERM "$ROOT_PID" 2>/dev/null || true
sleep 1
pkill -9 sola-browser-wpe 2>/dev/null || true
pkill -9 sola-browser-cef 2>/dev/null || true
pkill -9 WPEWebProcess 2>/dev/null || true
pkill -9 WPENetworkProce 2>/dev/null || true

END_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)
LOG_MARK_AFTER=$(wc -l < "$LOG_FILE" 2>/dev/null || echo 0)

# Extract FPS samples from the workspace log between marks. Each
# `shader fps` line is one fps sample. Use the file timestamp +
# our START_EPOCH to compute t_s. Lines look like:
#   2026-05-21T12:34:56.789012Z  INFO [browser-wpe] sola... fps="59.8" shader fps
echo "t_s,fps" > "$FPS_CSV"
if [[ "$LOG_MARK_AFTER" -gt "$LOG_MARK_BEFORE" ]]; then
    sed -n "$((LOG_MARK_BEFORE + 1)),${LOG_MARK_AFTER}p" "$LOG_FILE" \
      | grep "shader fps" \
      | grep "browser-${ENGINE}" \
      | while IFS= read -r line; do
            # Strip ANSI escapes if any.
            clean=$(echo "$line" | sed 's/\x1b\[[0-9;]*m//g')
            ts_iso=$(echo "$clean" | awk '{print $1}')
            fps=$(echo "$clean" | sed -nE 's/.*fps="?([0-9.]+)"?.*/\1/p')
            if [[ -n "$ts_iso" && -n "$fps" ]]; then
                # Convert ISO to epoch (GNU date).
                line_epoch=$(date -d "$ts_iso" +%s 2>/dev/null || true)
                if [[ -n "$line_epoch" ]]; then
                    echo "$((line_epoch - START_EPOCH)),${fps}" >> "$FPS_CSV"
                fi
            fi
        done
fi

# Meta.
cat > "$META" <<EOF
{
  "engine": "${ENGINE}",
  "url": "${URL}",
  "duration_s": ${DURATION},
  "start_iso": "${START_ISO}",
  "end_iso": "${END_ISO}",
  "binary": "${BIN}",
  "host": "$(hostname)",
  "kernel": "$(uname -r)"
}
EOF

echo "wrote: $OUT_DIR"
ls -la "$OUT_DIR"
