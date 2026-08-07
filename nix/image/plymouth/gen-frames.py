#!/usr/bin/env python3
"""Generate Plymouth frames: one quiet ripple, rest, repeat.

Design thesis (haiku / Japanese print):
  A single soft wash expands from the hub through the petals, settles to
  still ink, waits, then breathes again. Not a constant churn.

Timeline (fraction of loop, seamless):
  0.00–0.42  ripple travels hub → beyond tips
  0.42–1.00  rest (still mark) — long enough to feel intentional
"""

from __future__ import annotations

import json
import math
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

VIEW = "-25 -32 561 561"

# Quiet woodblock wash (not neon).
PAPER = (0x12, 0x16, 0x1C)
INK = (0x2A, 0x4A, 0x56)
MIST = (0x4A, 0x7A, 0x88)
BREATH = (0x6A, 0x9A, 0xAA)

# Fraction of the loop spent *active* (ripple in motion). Rest is the remainder.
RIPPLE_FRAC = 0.42
# Soft ring width as a fraction of petal tip radius.
RING_SIGMA_FRAC = 0.14
# Peak brightness of the ring (0..1) — keep haiku-quiet.
RING_PEAK = 0.72
# Still-field lift so petals never go pure black at rest.
REST_LIFT = 0.28


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def lerp_rgb(
    c0: tuple[int, int, int], c1: tuple[int, int, int], t: float
) -> tuple[int, int, int]:
    t = max(0.0, min(1.0, t))
    t = t * t * (3.0 - 2.0 * t)
    return (
        int(lerp(c0[0], c1[0], t)),
        int(lerp(c0[1], c1[1], t)),
        int(lerp(c0[2], c1[2], t)),
    )


def ease_out_cubic(u: float) -> float:
    """Natural deceleration as the wash reaches the tips (impeccable timing)."""
    u = max(0.0, min(1.0, u))
    return 1.0 - (1.0 - u) ** 3


def ring_intensity(r: float, r_wave: float, sigma: float) -> float:
    """Soft Gaussian ring centered at r_wave."""
    if sigma < 1.0:
        sigma = 1.0
    d = (r - r_wave) / sigma
    return math.exp(-0.5 * d * d)


def sample_color(
    r: float,
    loop_t: float,
    r_tip: float,
) -> tuple[int, int, int]:
    """
    loop_t in [0,1): full period = one ripple + rest.
    During rest, field is almost still; during ripple, one ring expands out.
    """
    sigma = r_tip * RING_SIGMA_FRAC
    # How far the ring has traveled (0 at hub start → past tips).
    if loop_t < RIPPLE_FRAC:
        u = ease_out_cubic(loop_t / RIPPLE_FRAC)
        # Travel from slightly inside hub to a bit past petal tips so it exits cleanly.
        r_wave = lerp(-sigma * 0.5, r_tip + sigma * 1.8, u)
        ring = ring_intensity(r, r_wave, sigma) * RING_PEAK
        # Soft afterglow just inside the ring (ink already laid).
        if r < r_wave:
            trail = max(0.0, 1.0 - (r_wave - r) / (r_tip * 0.85))
            ring = max(ring, trail * 0.12 * (1.0 - u * 0.5))
    else:
        ring = 0.0

    # Still paper/ink field
    base = lerp_rgb(PAPER, INK, REST_LIFT)
    # Soft vignette at hub (woodblock depth)
    if r_tip > 1.0:
        hub = max(0.0, 1.0 - r / (r_tip * 0.5))
        base = lerp_rgb(base, PAPER, hub * hub * 0.18)

    body = lerp_rgb(INK, MIST, ring)
    col = lerp_rgb(base, body, ring)
    if ring > 0.55:
        col = lerp_rgb(col, BREATH, (ring - 0.55) / 0.45 * 0.28)
    return col


def flower_mask_svg(petals: list[str]) -> str:
    paths = "\n".join(f'<path fill="#ffffff" d="{d}"/>' for d in petals)
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="{VIEW}">
  <rect x="-25" y="-32" width="561" height="561" fill="#000000"/>
  {paths}
</svg>
"""


def render_svg_png(svg_text: str, png_path: Path, size: int) -> None:
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg_text)
        svg_path = Path(f.name)
    try:
        subprocess.check_call(
            [
                "rsvg-convert",
                "-w",
                str(size),
                "-h",
                str(size),
                str(svg_path),
                "-o",
                str(png_path),
            ]
        )
    finally:
        svg_path.unlink(missing_ok=True)


def load_alpha_mask(png_path: Path) -> Image.Image:
    return Image.open(png_path).convert("RGB").convert("L")


def estimate_tip_radius(mask: Image.Image) -> float:
    w, h = mask.size
    cx, cy = (w - 1) * 0.5, (h - 1) * 0.5
    px = mask.load()
    r_max = 0.0
    for y in range(0, h, 2):
        for x in range(0, w, 2):
            if px[x, y] > 32:
                r = math.hypot(x - cx, y - cy)
                if r > r_max:
                    r_max = r
    return max(r_max, min(w, h) * 0.35)


def frame_image(size: int, loop_t: float, r_tip: float) -> Image.Image:
    img = Image.new("RGB", (size, size))
    px = img.load()
    cx = (size - 1) * 0.5
    cy = (size - 1) * 0.5
    for y in range(size):
        dy = y - cy
        for x in range(size):
            dx = x - cx
            r = math.hypot(dx, dy)
            px[x, y] = sample_color(r, loop_t, r_tip)
    return img


def main() -> None:
    petals_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    nframes = int(sys.argv[3]) if len(sys.argv) > 3 else 72
    size = int(sys.argv[4]) if len(sys.argv) > 4 else 360

    petals: list[str] = json.loads(petals_path.read_text())
    if len(petals) != 5:
        raise SystemExit(f"expected 5 petals, got {len(petals)}")

    out_dir.mkdir(parents=True, exist_ok=True)

    mask_path = out_dir / "_mask.png"
    render_svg_png(flower_mask_svg(petals), mask_path, size)
    mask = load_alpha_mask(mask_path)
    mask_path.unlink(missing_ok=True)

    r_tip = estimate_tip_radius(mask)
    n_ripple = max(1, int(nframes * RIPPLE_FRAC))
    n_rest = nframes - n_ripple

    print(
        f"pulse+rest wash: {nframes} frames @ {size}px "
        f"(ripple={n_ripple} rest={n_rest} r_tip={r_tip:.1f})",
        file=sys.stderr,
    )

    for frame in range(nframes):
        loop_t = frame / nframes
        grad = frame_image(size, loop_t, r_tip)
        rgba = grad.convert("RGBA")
        rgba.putalpha(mask)
        out = out_dir / f"frame-{frame:02d}.png"
        rgba.save(out, "PNG")
        if frame in (0, n_ripple // 2, n_ripple, nframes - 1):
            phase = "ripple" if loop_t < RIPPLE_FRAC else "rest"
            print(f"  wrote {out.name} (t={loop_t:.2f} {phase})", file=sys.stderr)

    print(f"wrote {nframes} frames to {out_dir}", file=sys.stderr)


if __name__ == "__main__":
    main()
