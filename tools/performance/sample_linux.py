#!/usr/bin/env python3
"""Sample Linux process-tree RSS, optional PSS and CPU without reading output."""

import argparse
import json
import math
import os
from pathlib import Path
import statistics
import time


def positive(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be finite and positive")
    return number


def read_process(path):
    # comm can contain spaces and parentheses; fields after its final ')' are fixed.
    fields = (path / "stat").read_text().rsplit(")", 1)[1].split()
    return {
        "pid": int(path.name),
        "ppid": int(fields[1]),
        "ticks": int(fields[11]) + int(fields[12]),
        "start": int(fields[19]),
        "rss": max(0, int(fields[21])) * os.sysconf("SC_PAGE_SIZE"),
    }


def process_tree(root_pid, root_start, proc=Path("/proc")):
    processes = {}
    for path in proc.iterdir():
        if path.name.isdigit():
            try:
                processes[int(path.name)] = read_process(path)
            except (FileNotFoundError, ProcessLookupError):
                continue  # A child may exit while /proc is being enumerated.
    root = processes.get(root_pid)
    if root is None or root["start"] != root_start:
        raise RuntimeError("The target process exited or its PID was reused")
    selected = {root_pid}
    while True:
        descendants = {pid for pid, p in processes.items() if p["ppid"] in selected}
        expanded = selected | descendants
        if expanded == selected:
            return [processes[pid] for pid in sorted(selected)]
        selected = expanded


def read_pss(path, expected_start):
    """Read proportional resident bytes, or None when unavailable/raced."""
    try:
        value = None
        for line in (path / "smaps_rollup").read_text().splitlines():
            if line.startswith("Pss:"):
                value = int(line.split()[1]) * 1024
                break
        if read_process(path)["start"] != expected_start:
            return None
        return value
    except (FileNotFoundError, ProcessLookupError, PermissionError):
        return None


def cpu_delta(previous, current, elapsed, ticks_per_second):
    old = {(p["pid"], p["start"]): p["ticks"] for p in previous}
    # Newly observed processes have no interval baseline; do not count lifetime CPU.
    ticks = sum(
        max(0, p["ticks"] - old[(p["pid"], p["start"])])
        for p in current
        if (p["pid"], p["start"]) in old
    )
    return 100 * ticks / ticks_per_second / elapsed


def percentile(values, fraction):
    if not values:
        raise ValueError("Cannot summarize empty samples")
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)]


def collect(pid, duration, interval, pss=False):
    root = read_process(Path("/proc") / str(pid))
    previous = process_tree(pid, root["start"])
    start = previous_time = time.monotonic()
    samples = []
    while time.monotonic() - start < duration:
        time.sleep(min(interval, max(0, duration - (time.monotonic() - start))))
        current = process_tree(pid, root["start"])
        now = time.monotonic()
        proportional = (
            [read_pss(Path("/proc") / str(p["pid"]), p["start"]) for p in current]
            if pss
            else []
        )
        total_pss = (
            sum(proportional)
            if proportional and all(value is not None for value in proportional)
            else None
        )
        samples.append(
            {
                "elapsed_seconds": now - start,
                "process_count": len(current),
                "rss_bytes": sum(p["rss"] for p in current),
                "pss_bytes": total_pss,
                "cpu_percent_one_core": cpu_delta(
                    previous, current, now - previous_time, os.sysconf("SC_CLK_TCK")
                ),
            }
        )
        previous, previous_time = current, now
    return samples


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--implementation", choices=["tauri", "gpui"], required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--panes", type=int, choices=[1, 4, 8], required=True)
    parser.add_argument("--scenario", choices=["idle", "output"], required=True)
    parser.add_argument("--display", choices=["x11", "wayland"], required=True)
    parser.add_argument("--duration", type=positive, default=30.0)
    parser.add_argument("--interval", type=positive, default=0.25)
    parser.add_argument(
        "--pss",
        action="store_true",
        help="Also read PSS from smaps_rollup; unavailable samples remain null",
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not Path("/proc/self/stat").exists():
        parser.error("This collector requires Linux procfs")
    if args.pid <= 0:
        parser.error("PID must be positive")
    if args.interval > args.duration:
        parser.error("Interval must not exceed duration")
    if args.output.exists():
        parser.error("Output already exists; choose a new report path")
    samples = collect(args.pid, args.duration, args.interval, args.pss)
    proportional = [sample["pss_bytes"] for sample in samples]
    report = {
        "schema_version": 1,
        "implementation": args.implementation,
        "revision": args.revision,
        "panes": args.panes,
        "scenario": args.scenario,
        "display": args.display,
        "pss_requested": args.pss,
        "samples": samples,
        "summary": {
            "pss_complete": all(value is not None for value in proportional),
            "pss_median_bytes": statistics.median(proportional)
            if all(value is not None for value in proportional)
            else None,
            "rss_median_bytes": statistics.median(s["rss_bytes"] for s in samples),
            "rss_p95_bytes": percentile([s["rss_bytes"] for s in samples], 0.95),
            "cpu_mean_percent_one_core": sum(
                s["cpu_percent_one_core"]
                * (
                    s["elapsed_seconds"]
                    - (samples[i - 1]["elapsed_seconds"] if i else 0)
                )
                for i, s in enumerate(samples)
            )
            / samples[-1]["elapsed_seconds"],
        },
        "limitations": [
            "RSS counts shared pages repeatedly; optional PSS apportions them across sharers.",
            "PSS is null when any selected process is inaccessible or exits during sampling.",
            "Short-lived or reparented processes can be missed between samples.",
            "New process CPU is counted only after its first observation.",
            "Pane count, revision, scenario and display are operator-supplied labels.",
            "Does not measure startup, prompt visibility or input-to-paint latency.",
        ],
    }
    with args.output.open("x") as output:
        json.dump(report, output, indent=2)
        output.write("\n")


if __name__ == "__main__":
    main()
