#!/usr/bin/env python3
"""Run a CI phase with live process evidence and an unchanged exit status."""

import os
from pathlib import Path
import subprocess
import sys
import time


def progress(elapsed):
    print(f"CI phase still running after {elapsed:.0f}s", flush=True)
    try:
        result = subprocess.run(
            ["ps", "-axo", "pid,ppid,etime,time,pcpu,rss,comm"],
            capture_output=True, text=True, timeout=10, check=True,
        )
        lines = result.stdout.splitlines()
        print("RSS is KiB; CPU time is cumulative:", flush=True)
        for line in lines:
            if line == lines[0] or Path(line.split()[-1]).name in {
                "cargo", "cargo-nextest", "rustc", "sccache", "clang", "ld", "ld64.lld",
            }:
                print(line, flush=True)
    except (OSError, subprocess.SubprocessError):
        print("Process snapshot unavailable", flush=True)


def run(command):
    start = time.monotonic()
    print("CI phase started", flush=True)
    with subprocess.Popen(command) as process:
        while True:
            try:
                code = process.wait(timeout=60)
                break
            except subprocess.TimeoutExpired:
                progress(time.monotonic() - start)
    elapsed = time.monotonic() - start
    print(f"CI phase finished in {elapsed:.1f}s (exit {code})", flush=True)
    summary = os.getenv("GITHUB_STEP_SUMMARY")
    if summary:
        try:
            with Path(summary).open("a", encoding="utf-8") as output:
                output.write(f"{os.getenv('GITHUB_JOB', 'CI')} phase: {elapsed:.1f}s (exit {code})\n\n")
        except OSError:
            pass  # Reporting must not change the command's result.
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    if len(sys.argv) < 3 or sys.argv[1] != "--":
        raise SystemExit("Usage: ci-run.py -- <command> [arguments...]")
    raise SystemExit(run(sys.argv[2:]))
