#!/usr/bin/env python3
"""Generate Plymouth frames: rotating conical cyan gradient under flower mask.

The five-petal mark is an alpha *mask*. Paint is a smooth circular (conic)
gradient in neon-cyan tones that rotates clockwise. Petals show continuous
shades, not flat per-petal fills — no chunky steps.
"""

from __future__ import annotations

import json
import math
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

# SVG viewBox from flower.svg
VIEW = "-25 -32 561 561"

# Conic color stops around the ring (t=0 = peak / “noon”, increases clockwise).
# Soft lobe: peak neon → mid cyan → *light* trough (not near-black) → back.
# Keeps motion readable without a heavy dark wedge.
STOPS: list[tuple[float, tuple[int, int, int]]] = [
    (0.00, (92, 255, 255)),  # #5cffff peak neon
    (0.12, (40, 235, 245)),
    (0.25, (0, 212, 232)),  # #00d4e8
    (0.38, (0, 180, 200)),
    (0.50, (0, 130, 148)),  # light trough (was near-black; lifted)
    (0.62, (0, 165, 184)),
    (0.75, (0, 200, 220)),
    (0.88, (50, 240, 250)),
    (1.00, (92, 255, 255)),  # seamless wrap
]


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def sample_stops(t: float) -> tuple[int, int, int]:
    t = t % 1.0
    for i in range(len(STOPS) - 1):
        t0, c0 = STOPS[i]
        t1, c1 = STOPS[i + 1]
        if t0 <= t <= t1:
            u = 0.0 if t1 == t0 else (t - t0) / (t1 - t0)
            # Smoothstep for softer transitions between stops
            u = u * u * (3.0 - 2.0 * u)
            return (
                int(lerp(c0[0], c1[0], u)),
                int(lerp(c0[1], c1[1], u)),
                int(lerp(c0[2], c1[2], u)),
            )
    return STOPS[-1][1]


def flower_mask_svg(petals: list[str]) -> str:
    """White petals on black — alpha mask source for rsvg."""
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
    """Luminance of white-on-black mask → alpha channel (L mode)."""
    im = Image.open(png_path).convert("RGB")
    # White petals → high L; black field → 0
    return im.convert("L")


def conical_gradient(size: int, rotation_rad: float) -> Image.Image:
    """Full-frame conic gradient; angle 0 at top, + clockwise, rotated by rotation_rad."""
    img = Image.new("RGB", (size, size))
    px = img.load()
    cx = (size - 1) * 0.5
    cy = (size - 1) * 0.5
    two_pi = 2.0 * math.pi
    for y in range(size):
        dy = cy - y  # screen y down → north component
        for x in range(size):
            dx = x - cx
            # 0 at top, increasing clockwise
            ang = math.atan2(dx, dy)
            t = ((ang + rotation_rad) % two_pi) / two_pi
            px[x, y] = sample_stops(t)
    return img


def main() -> None:
    petals_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    # Many frames → smooth rotation (not 5 petal steps).
    nframes = int(sys.argv[3]) if len(sys.argv) > 3 else 36
    size = int(sys.argv[4]) if len(sys.argv) > 4 else 360

    petals: list[str] = json.loads(petals_path.read_text())
    if len(petals) != 5:
        raise SystemExit(f"expected 5 petals, got {len(petals)}")

    out_dir.mkdir(parents=True, exist_ok=True)

    # One mask for all frames.
    mask_path = out_dir / "_mask.png"
    render_svg_png(flower_mask_svg(petals), mask_path, size)
    mask = load_alpha_mask(mask_path)
    mask_path.unlink(missing_ok=True)

    print(
        f"conical gradient under flower mask: {nframes} frames @ {size}px",
        file=sys.stderr,
    )

    for frame in range(nframes):
        # One full clockwise revolution over nframes.
        rotation = (frame / nframes) * 2.0 * math.pi
        grad = conical_gradient(size, rotation)
        # Composite: RGB from gradient, A from flower silhouette.
        rgba = grad.convert("RGBA")
        rgba.putalpha(mask)
        out = out_dir / f"frame-{frame:02d}.png"
        rgba.save(out, "PNG")
        if frame == 0 or frame == nframes // 4 or frame == nframes - 1:
            print(f"  wrote {out.name} (rot={math.degrees(rotation):.1f}°)", file=sys.stderr)

    print(f"wrote {nframes} frames to {out_dir}", file=sys.stderr)


if __name__ == "__main__":
    main()
