#!/usr/bin/env python3
"""Summarize one or more bench runs into a single markdown report.

Usage:
    bench/summarize.py <run_dir> [<run_dir> ...] [-o <out.md>]

Each <run_dir> must contain procs.csv, gpu.csv, fps.csv, and meta.json
in the format emitted by bench/run-bench.sh. Runs are compared side
by side (good for wpe-vs-cef on the same URL).
"""

from __future__ import annotations

import argparse
import csv
import json
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


@dataclass
class Run:
    name: str          # short engine name, e.g. "wpe"
    url: str
    duration_s: int
    start_iso: str
    procs: list[dict]  # one dict per row of procs.csv
    gpu:   list[dict]
    fps:   list[dict]


def load_run(path: Path) -> Run:
    with open(path / "meta.json") as f:
        meta = json.load(f)
    return Run(
        name=meta["engine"],
        url=meta["url"],
        duration_s=int(meta["duration_s"]),
        start_iso=meta["start_iso"],
        procs=list(csv.DictReader(open(path / "procs.csv"))),
        gpu=list(csv.DictReader(open(path / "gpu.csv"))),
        fps=list(csv.DictReader(open(path / "fps.csv"))),
    )


def tree_cpu_pct(rows: Iterable[dict]) -> list[float]:
    """Return one value per sampled second: sum of %cpu across the
    whole process tree at that second."""
    by_t: dict[int, float] = {}
    for r in rows:
        t = int(r["t_s"])
        # ps's %cpu is a *running average* since process start, so
        # summing across pids at one t_s gives "total CPU% across
        # the tree, averaged since launch". Not ideal — interval
        # CPU would need /proc/<pid>/stat sampling — but it's a
        # useful first-cut.
        by_t[t] = by_t.get(t, 0.0) + float(r["cpu_pct"])
    return [by_t[t] for t in sorted(by_t)]


def tree_rss_mib(rows: Iterable[dict]) -> list[float]:
    by_t: dict[int, float] = {}
    for r in rows:
        t = int(r["t_s"])
        by_t[t] = by_t.get(t, 0.0) + float(r["rss_kib"]) / 1024.0
    return [by_t[t] for t in sorted(by_t)]


def proc_count(rows: Iterable[dict]) -> list[int]:
    by_t: dict[int, int] = {}
    for r in rows:
        t = int(r["t_s"])
        by_t[t] = by_t.get(t, 0) + 1
    return [by_t[t] for t in sorted(by_t)]


def col(rows: Iterable[dict], key: str) -> list[float]:
    return [float(r[key]) for r in rows]


def stats_line(values: list[float], unit: str = "") -> str:
    if not values:
        return f"– (no samples)"
    return (
        f"min {min(values):.1f}{unit} · "
        f"median {statistics.median(values):.1f}{unit} · "
        f"max {max(values):.1f}{unit} · "
        f"n={len(values)}"
    )


def render_run(r: Run) -> str:
    cpu = tree_cpu_pct(r.procs)
    rss = tree_rss_mib(r.procs)
    pc  = proc_count(r.procs)
    fps = col(r.fps, "fps") if r.fps else []
    gpu_util = col(r.gpu, "util_pct") if r.gpu else []
    gpu_mem  = col(r.gpu, "mem_used_mib") if r.gpu else []
    return (
        f"### {r.name} — `{r.url}` ({r.duration_s}s)\n"
        f"\n"
        f"| metric              | values                                                          |\n"
        f"| ------------------- | --------------------------------------------------------------- |\n"
        f"| tree CPU%           | {stats_line(cpu, '%')} |\n"
        f"| tree RSS (MiB)      | {stats_line(rss)} |\n"
        f"| process count       | {stats_line([float(x) for x in pc])} |\n"
        f"| shader FPS          | {stats_line(fps)} |\n"
        f"| GPU util% (whole)   | {stats_line(gpu_util, '%')} |\n"
        f"| GPU mem MiB (whole) | {stats_line(gpu_mem)} |\n"
        f"\n"
        f"*Start:* {r.start_iso}\n"
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("runs", nargs="+", type=Path)
    ap.add_argument("-o", "--out", type=Path, default=None,
                    help="Write to this path instead of stdout.")
    args = ap.parse_args()

    runs = [load_run(p) for p in args.runs]
    parts = ["# WPE vs CEF — bench summary\n"]
    for r in runs:
        parts.append(render_run(r))
    body = "\n".join(parts)

    if args.out:
        args.out.write_text(body)
        print(f"wrote {args.out}", file=sys.stderr)
    else:
        sys.stdout.write(body)
    return 0


if __name__ == "__main__":
    sys.exit(main())
