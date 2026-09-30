#!/usr/bin/env python3
"""Build and capture the Phase 6 Rust benchmark as machine-readable JSON.

Usage: python3 scripts/benchmark-phase6.py [--output PATH]
The default output is phase6-benchmark.json in the current directory.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
EXAMPLE = "phase6_bench"


def run_measured(executable: Path) -> tuple[int, str, str, float, int | None, str | None]:
    """Run the example and collect wait4 resource usage where available."""
    start = time.perf_counter()
    if os.name == "nt" or not hasattr(os, "wait4") or not hasattr(os, "fork"):
        proc = subprocess.run([str(executable)], capture_output=True, text=True)
        elapsed = time.perf_counter() - start
        if os.name == "nt":
            reason = "Peak RSS collection is unavailable on Windows with Python stdlib APIs."
        else:
            reason = "This platform's Python runtime does not expose wait4/fork resource collection."
        return proc.returncode, proc.stdout, proc.stderr, elapsed, None, reason

    with tempfile.TemporaryFile() as stdout_file, tempfile.TemporaryFile() as stderr_file:
        pid = os.fork()
        if pid == 0:  # pragma: no cover - child replaces itself
            try:
                os.dup2(stdout_file.fileno(), 1)
                os.dup2(stderr_file.fileno(), 2)
                os.execv(str(executable), [str(executable)])
            except BaseException:
                os._exit(127)

        _, status, usage = os.wait4(pid, 0)
        stdout_file.seek(0)
        stderr_file.seek(0)
        stdout = stdout_file.read().decode("utf-8", errors="replace")
        stderr = stderr_file.read().decode("utf-8", errors="replace")
    elapsed = time.perf_counter() - start
    exit_code = os.waitstatus_to_exitcode(status)
    # ru_maxrss is bytes on macOS and KiB on Linux. Other Unix variants
    # differ, so avoid reporting a guessed conversion.
    if sys.platform == "darwin":
        peak_bytes = int(usage.ru_maxrss)
    elif sys.platform.startswith("linux"):
        peak_bytes = int(usage.ru_maxrss * 1024)
    else:
        peak_bytes = None
    reason = None if peak_bytes is not None else (
        f"Peak RSS units are not normalized for {sys.platform}."
    )
    return exit_code, stdout, stderr, elapsed, peak_bytes, reason


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("phase6-benchmark.json"))
    args = parser.parse_args()

    build_cmd = [
        "cargo", "build", "--locked", "--release", "-p", "archaeodash-analysis",
        "--example", EXAMPLE, "--message-format=json",
    ]
    build = subprocess.run(build_cmd, cwd=ROOT, capture_output=True, text=True)
    build_stdout = build.stdout
    executable: Path | None = None
    for line in build.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == EXAMPLE:
            executable_value = message.get("executable")
            if executable_value:
                executable = Path(executable_value)
    git_revision = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True
    )
    git_dirty = subprocess.run(
        ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, text=True
    )
    result: dict[str, object] = {
        "schema_version": 1,
        "captured_at_utc": datetime.now(timezone.utc).isoformat(),
        "environment": {
            "os": platform.platform(),
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "rustc": None,
            "git_revision": git_revision.stdout.strip() if git_revision.returncode == 0 else None,
            "git_dirty": bool(git_dirty.stdout.strip()) if git_dirty.returncode == 0 else None,
        },
        "build": {
            "command": build_cmd,
            "exit_code": build.returncode,
            "stdout": build_stdout,
            "stderr": build.stderr,
            "timing_included_in_runtime": False,
        },
        "benchmark": None,
    }
    rustc = subprocess.run(["rustc", "--version"], capture_output=True, text=True)
    environment = result["environment"]
    assert isinstance(environment, dict)
    environment["rustc"] = rustc.stdout.strip() if rustc.returncode == 0 else None
    if build.returncode == 0 and executable is not None:
        code, stdout, stderr, elapsed, peak_bytes, unavailable = run_measured(executable)
        benchmark: dict[str, object] = {
            "command": [str(executable)],
            "exit_code": code,
            "elapsed_seconds": elapsed,
            "elapsed_scope": "example process including process startup; release build excluded",
            "stdout": stdout,
            "stderr": stderr,
            "peak_resident_memory_bytes": peak_bytes,
            "peak_resident_memory_scope": "whole child process; may include forked Python launcher overhead before exec",
            "peak_resident_memory_unavailable_reason": unavailable,
        }
        if peak_bytes is None and unavailable is None:
            benchmark["peak_resident_memory_unavailable_reason"] = (
                "This platform's Python runtime does not expose wait4 resource usage."
            )
        result["benchmark"] = benchmark
    elif build.returncode == 0:
        result["benchmark"] = {
            "exit_code": 1,
            "elapsed_seconds": None,
            "stdout": "",
            "stderr": "Cargo build succeeded but emitted no executable path for the example.",
            "peak_resident_memory_bytes": None,
            "peak_resident_memory_unavailable_reason": "The benchmark executable could not be identified.",
        }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(args.output, file=sys.stderr)
    benchmark = result["benchmark"]
    if build.returncode != 0:
        return build.returncode
    assert isinstance(benchmark, dict)
    return int(benchmark["exit_code"])


if __name__ == "__main__":
    raise SystemExit(main())
