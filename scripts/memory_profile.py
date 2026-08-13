#!/usr/bin/env python

# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to
# deal in the Software without restriction, including without limitation the
# rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
# sell copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:

# The above copyright notice and this permission notice shall be included in
# all copies or substantial portions of the Software.

# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
# FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
# IN THE SOFTWARE.

"""Measure peak memory for a command using the Linux proc filesystem."""

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Sequence


@dataclass
class Memory:
    """Memory counters reported by Linux, in KiB."""

    rss_kib: int = 0
    hwm_kib: int = 0
    anonymous_kib: int = 0
    file_kib: int = 0
    swap_kib: int = 0
    pss_kib: int = 0
    private_kib: int = 0

    def include(self, sample: Memory) -> None:
        """Update every counter to its observed maximum."""
        for name in self.__dataclass_fields__:
            setattr(self, name, max(getattr(self, name), getattr(sample, name)))


@dataclass
class Run:
    """One measured command invocation."""

    elapsed_seconds: float
    exit_code: int
    samples: int
    peak: Memory


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--cwd",
        type=Path,
        help="working directory for the measured command",
    )
    parser.add_argument(
        "--interval-ms",
        type=float,
        default=5.0,
        help="sampling interval in milliseconds (default: 5)",
    )
    parser.add_argument(
        "--repeat",
        type=int,
        default=1,
        help="number of command invocations (default: 1)",
    )
    parser.add_argument(
        "--json",
        type=Path,
        help="also write the complete result as JSON",
    )
    parser.add_argument(
        "command",
        nargs=argparse.REMAINDER,
        help="command to measure, conventionally preceded by --",
    )
    return parser.parse_args()


def read_key_values(path: Path) -> dict[str, int]:
    """Read KiB counters from a proc status-like file."""
    values: dict[str, int] = {}
    try:
        contents = path.read_text(encoding="ascii")
    except (FileNotFoundError, PermissionError, ProcessLookupError):
        return values

    for line in contents.splitlines():
        name, separator, value = line.partition(":")
        if not separator:
            continue
        fields = value.split()
        if fields and fields[0].isdigit():
            values[name] = int(fields[0])
    return values


def sample_process(pid: int) -> Memory:
    """Read current and high-water memory counters for one process."""
    status = read_key_values(Path("/proc") / str(pid) / "status")
    rollup = read_key_values(Path("/proc") / str(pid) / "smaps_rollup")
    return Memory(
        rss_kib=status.get("VmRSS", 0),
        hwm_kib=status.get("VmHWM", 0),
        anonymous_kib=status.get("RssAnon", 0),
        file_kib=status.get("RssFile", 0),
        swap_kib=status.get("VmSwap", 0),
        pss_kib=rollup.get("Pss", 0),
        private_kib=(
            rollup.get("Private_Clean", 0) + rollup.get("Private_Dirty", 0)
        ),
    )


def measure(
    command: Sequence[str], *, cwd: Path | None, interval: float
) -> Run:
    """Run a command and sample its process memory until completion."""
    started = time.monotonic()
    process = subprocess.Popen(command, cwd=cwd)
    peak = Memory()
    samples = 0

    while process.poll() is None:
        peak.include(sample_process(process.pid))
        samples += 1
        time.sleep(interval)

    # VmHWM survives short spikes between samples and is retained until exit.
    # This last read can still succeed briefly while the process is a zombie.
    peak.include(sample_process(process.pid))
    elapsed = time.monotonic() - started
    return Run(elapsed, process.returncode, samples, peak)


def mebibytes(kibibytes: float) -> str:
    """Format a KiB measurement as MiB."""
    return f"{kibibytes / 1024:.2f} MiB"


def print_run(index: int, run: Run) -> None:
    """Print one compact human-readable result."""
    print(
        f"run {index}: peak RSS {mebibytes(run.peak.hwm_kib)}, "
        f"peak PSS {mebibytes(run.peak.pss_kib)}, "
        f"peak private {mebibytes(run.peak.private_kib)}, "
        f"elapsed {run.elapsed_seconds:.3f}s, exit {run.exit_code}"
    )


def main() -> int:
    """Measure the requested command."""
    args = parse_args()
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        raise SystemExit("a command is required after --")
    if args.repeat < 1:
        raise SystemExit("--repeat must be at least 1")
    if args.interval_ms <= 0:
        raise SystemExit("--interval-ms must be greater than zero")
    if not Path("/proc/self/status").exists():
        raise SystemExit("memory_profile.py currently requires Linux /proc")

    runs: list[Run] = []
    for index in range(1, args.repeat + 1):
        run = measure(
            command,
            cwd=args.cwd,
            interval=args.interval_ms / 1000,
        )
        runs.append(run)
        print_run(index, run)
        if run.exit_code != 0:
            break

    summary = {
        "command": command,
        "cwd": str(args.cwd.resolve()) if args.cwd else None,
        "runs": [asdict(run) for run in runs],
        "median": {
            "peak_rss_kib": statistics.median(run.peak.hwm_kib for run in runs),
            "peak_pss_kib": statistics.median(run.peak.pss_kib for run in runs),
            "peak_private_kib": statistics.median(
                run.peak.private_kib for run in runs
            ),
            "elapsed_seconds": statistics.median(
                run.elapsed_seconds for run in runs
            ),
        },
    }
    if len(runs) > 1:
        print(
            f"median: peak RSS {mebibytes(summary['median']['peak_rss_kib'])}, "
            f"peak PSS {mebibytes(summary['median']['peak_pss_kib'])}, "
            "peak private "
            f"{mebibytes(summary['median']['peak_private_kib'])}, "
            f"elapsed {summary['median']['elapsed_seconds']:.3f}s"
        )
    if args.json:
        args.json.write_text(
            json.dumps(summary, indent=2) + "\n",
            encoding="utf-8",
        )

    return max(run.exit_code for run in runs)


if __name__ == "__main__":
    sys.exit(main())
