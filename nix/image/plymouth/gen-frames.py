#!/usr/bin/env python3
"""Generate Plymouth frames: a cyan shade gradient walking clockwise.

Five petals form a ring. Each frame paints a fixed 5-step cyan ladder onto
the petals in clockwise order, then the peak advances one petal. Result: a
uniform circular gradient rotating clockwise — never independent flicker.
"""

from __future__ import annotations

import json
import math
import re
import subprocess
import sys
from pathlib import Path

# SVG viewBox center for flower.svg (-25 -32 561 561)
CX = -25 + 561 / 2  # 255.5
CY = -32 + 561 / 2  # 248.5

# Neon cyan ladder assigned along the ring from the peak:
#   clockwise_distance 0 → hottest peak
#   clockwise_distance 4 → deepest teal (just before peak wraps)
CYAN_LADDER = [
    "#5cffff",  # 0 peak neon
    "#00d4e8",  # 1
    "#0090a0",  # 2 mid
    "#005560",  # 3
    "#0a1e22",  # 4 deep (high contrast so the walk is obvious)
]


def petal_centroid(path_d: str) -> tuple[float, float]:
    nums = [float(x) for x in re.findall(r"[+-]?(?:\d+\.?\d*|\.\d+)", path_d)]
    xs, ys = nums[0::2], nums[1::2]
    return (sum(xs) / len(xs), sum(ys) / len(ys))


def clockwise_order(petals: list[str]) -> list[int]:
    """Petal indices sorted clockwise starting from the top-most petal.

    Screen coords (y down): angle 0 at top, increasing clockwise via
    atan2(east, north) = atan2(x - cx, cy - y).
    """
    scored: list[tuple[float, int]] = []
    for i, d in enumerate(petals):
        x, y = petal_centroid(d)
        ang = math.atan2(x - CX, CY - y)  # 0 = top, + = clockwise
        if ang < 0:
            ang += 2 * math.pi
        scored.append((ang, i))
    scored.sort()
    return [i for _, i in scored]


def main() -> None:
    petals_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    # Default 5 frames = one full revolution (peak on each petal once).
    nframes = int(sys.argv[3]) if len(sys.argv) > 3 else 5
    size = int(sys.argv[4]) if len(sys.argv) > 4 else 360

    petals: list[str] = json.loads(petals_path.read_text())
    if len(petals) != 5:
        raise SystemExit(f"expected 5 petals, got {len(petals)}")

    order = clockwise_order(petals)
    print(f"clockwise petal indices (from top): {order}", file=sys.stderr)

    # Sanity: print which physical petal is which slot
    labels = ["top", "top-right", "bottom-right", "bottom-left", "top-left"]
    for slot, petal_i in enumerate(order):
        print(f"  slot {slot} ({labels[slot]}): petal[{petal_i}]", file=sys.stderr)

    out_dir.mkdir(parents=True, exist_ok=True)

    for frame in range(nframes):
        # Peak advances one clockwise slot per frame, wrapping after 5.
        # With nframes != 5, still one revolution over the full sequence.
        peak_slot = (frame * 5) // nframes  # integer 0..4 for nframes multiple of 5
        if nframes % 5 != 0:
            peak_slot = frame % 5

        fills: list[str] = [""] * 5
        for slot, petal_i in enumerate(order):
            # Clockwise distance from the peak petal to this petal (0..4).
            # slot 0 at peak → brightest; next clockwise → next shade; etc.
            dist = (slot - peak_slot) % 5
            fills[petal_i] = CYAN_LADDER[dist]

        # Debug: shade assignment for this frame
        ring = " → ".join(
            f"{labels[s]}={CYAN_LADDER[(s - peak_slot) % 5]}" for s in range(5)
        )
        print(f"frame {frame:02d} peak=slot{peak_slot}: {ring}", file=sys.stderr)

        paths = [
            f'<path fill="{fills[p]}" d="{petals[p]}"/>' for p in range(5)
        ]
        svg = f"""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="-25 -32 561 561">
{chr(10).join(paths)}
</svg>
"""
        svg_path = out_dir / f"frame-{frame:02d}.svg"
        png_path = out_dir / f"frame-{frame:02d}.png"
        svg_path.write_text(svg)
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
        svg_path.unlink()

    print(f"wrote {nframes} frames to {out_dir}", file=sys.stderr)


if __name__ == "__main__":
    main()
