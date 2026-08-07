#!/usr/bin/env bash
# Quick local preview of the Plymouth flower animation — no ISO/qcow build.
#
# Usage:
#   ./preview.sh              # 48 frames @ 360px → /tmp/ply-preview/
#   ./preview.sh 32 240       # fewer/smaller frames for a faster loop
#   OUT=/tmp/foo ./preview.sh
#
# Opens the HTML preview in the default browser when possible.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
NFRAMES="${1:-72}"
SIZE="${2:-360}"
OUT="${OUT:-/tmp/ply-preview}"
# Match theme: ~50 ms/frame → ~3.6 s period (ripple then long rest).
MS_PER_FRAME="${MS_PER_FRAME:-50}"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing dependency: $1" >&2
    exit 1
  }
}

need python3
need rsvg-convert

if ! python3 -c 'from PIL import Image' 2>/dev/null; then
  echo "missing Python package: Pillow (pip install pillow / nix: python3Packages.pillow)" >&2
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT"

echo ">>> generating $NFRAMES frames @ ${SIZE}px → $OUT"
python3 "$ROOT/gen-frames.py" "$ROOT/petals.json" "$OUT" "$NFRAMES" "$SIZE"

# HTML flipbook (always works; graphite backdrop matches boot field).
HTML="$OUT/index.html"
{
  cat <<EOF
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Sola Plymouth preview</title>
  <style>
    :root { color-scheme: dark; }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      display: grid;
      place-items: center;
      background: #0c0e12;
      font: 13px/1.4 ui-sans-serif, system-ui, sans-serif;
      color: #8b94a8;
    }
    .stage {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 1.25rem;
    }
    .mark {
      width: min(72vw, ${SIZE}px);
      height: min(72vw, ${SIZE}px);
      image-rendering: auto;
    }
    .mark img {
      display: none;
      width: 100%;
      height: 100%;
    }
    .mark img.on { display: block; }
    kbd {
      font: inherit;
      color: #e9ecf2;
      background: #151922;
      border: 1px solid #2a3344;
      border-radius: 4px;
      padding: 0.1em 0.4em;
    }
    a { color: #3dd6f5; }
  </style>
</head>
<body>
  <div class="stage">
    <div class="mark" id="mark" aria-label="Sola boot mark animation"></div>
    <div>
      <span id="meta">${NFRAMES} frames · ${MS_PER_FRAME} ms · loop</span>
      · <a id="giflink" href="preview.gif" hidden>GIF</a>
    </div>
    <div>Space pause · <kbd>←</kbd><kbd>→</kbd> step · click stage to pause</div>
  </div>
  <script>
    const N = ${NFRAMES};
    const MS = ${MS_PER_FRAME};
    const mark = document.getElementById("mark");
    const imgs = [];
    for (let i = 0; i < N; i++) {
      const img = new Image();
      img.src = "frame-" + String(i).padStart(2, "0") + ".png";
      img.alt = "";
      mark.appendChild(img);
      imgs.push(img);
    }
    let i = 0, playing = true, timer = null;
    function show(n) {
      imgs[i].classList.remove("on");
      i = ((n % N) + N) % N;
      imgs[i].classList.add("on");
      document.getElementById("meta").textContent =
        "frame " + String(i).padStart(2, "0") + " / " + N + (playing ? " · playing" : " · paused");
    }
    function tick() {
      if (!playing) return;
      show(i + 1);
      timer = setTimeout(tick, MS);
    }
    function play() {
      playing = true;
      clearTimeout(timer);
      timer = setTimeout(tick, MS);
      show(i);
    }
    function pause() {
      playing = false;
      clearTimeout(timer);
      show(i);
    }
    imgs[0].classList.add("on");
    play();
    window.addEventListener("keydown", (e) => {
      if (e.code === "Space") {
        e.preventDefault();
        playing ? pause() : play();
      } else if (e.code === "ArrowRight") {
        pause();
        show(i + 1);
      } else if (e.code === "ArrowLeft") {
        pause();
        show(i - 1);
      }
    });
    mark.addEventListener("click", () => (playing ? pause() : play()));
  </script>
</body>
</html>
EOF
} >"$HTML"

# Optional GIF (ImageMagick) for Slack / quick share.
if command -v magick >/dev/null 2>&1 || command -v convert >/dev/null 2>&1; then
  echo ">>> assembling preview.gif"
  # Delay is centiseconds in GIF.
  DELAY_CS=$(( (MS_PER_FRAME + 5) / 10 ))
  if [ "$DELAY_CS" -lt 1 ]; then DELAY_CS=1; fi
  # Composite onto graphite so transparency doesn't flash pure black in some viewers.
  BG="#0c0e12"
  if command -v magick >/dev/null 2>&1; then
    magick -delay "$DELAY_CS" -loop 0 -dispose Background \
      -background "$BG" "$OUT"/frame-*.png -layers Flatten \
      "$OUT/preview.gif" 2>/dev/null \
      || magick -delay "$DELAY_CS" -loop 0 "$OUT"/frame-*.png "$OUT/preview.gif"
  else
    convert -delay "$DELAY_CS" -loop 0 -dispose Background \
      -background "$BG" "$OUT"/frame-*.png -layers Flatten \
      "$OUT/preview.gif" 2>/dev/null \
      || convert -delay "$DELAY_CS" -loop 0 "$OUT"/frame-*.png "$OUT/preview.gif"
  fi
  # Reveal GIF link in HTML
  if [ -f "$OUT/preview.gif" ]; then
    sed -i 's/id="giflink" href="preview.gif" hidden/id="giflink" href="preview.gif"/' "$HTML" 2>/dev/null \
      || sed -i '' 's/id="giflink" href="preview.gif" hidden/id="giflink" href="preview.gif"/' "$HTML" 2>/dev/null \
      || true
  fi
else
  echo ">>> skip GIF (no ImageMagick convert/magick)"
fi

echo "✓ preview ready"
echo "  HTML: file://$HTML"
if [ -f "$OUT/preview.gif" ]; then
  echo "  GIF:  $OUT/preview.gif"
fi

# Open browser when possible (skip headless/CI).
if [ -z "${IMPECCABLE_QUESTION_DISABLED:-}" ] && [ -z "${CI:-}" ]; then
  if command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$HTML" >/dev/null 2>&1 || true
  elif command -v open >/dev/null 2>&1; then
    open "$HTML" || true
  fi
fi
