#!/usr/bin/env python3
"""Run a CI phase with live process evidence and an unchanged exit status."""

import os
from pathlib import Path
import subprocess
import sys
import time


def windows_snapshot():
    script = """
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$names = 'cargo.exe', 'cargo-nextest.exe', 'rustc.exe', 'sccache.exe',
    'clang.exe', 'clang-cl.exe', 'cl.exe', 'link.exe', 'lld-link.exe',
    'rust-lld.exe', 'ld.exe', 'ld64.lld.exe'
$filter = ($names | ForEach-Object { "Name = '$_'" }) -join ' OR '
$now = Get-Date
Get-CimInstance -ClassName Win32_Process -Filter $filter |
    Sort-Object ProcessId |
    Select-Object @{Name='PID'; Expression={$_.ProcessId}},
        @{Name='PPID'; Expression={$_.ParentProcessId}},
        @{Name='ELAPSED(s)'; Expression={
            if ($_.CreationDate) { [math]::Round(($now - $_.CreationDate).TotalSeconds) }
        }},
        @{Name='CPU(s)'; Expression={
            [math]::Round(([double]$_.KernelModeTime + [double]$_.UserModeTime) / 1e7, 1)
        }},
        @{Name='RSS(KiB)'; Expression={[math]::Round([double]$_.WorkingSetSize / 1024)}},
        @{Name='COMMAND'; Expression={$_.Name}} |
    Format-Table -AutoSize |
    Out-String -Width 200
"""
    result = subprocess.run(
        ["powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=10, check=True,
    )
    print("RSS is KiB; CPU time is cumulative seconds:", flush=True)
    print(result.stdout.strip() or "No build processes found", flush=True)


def progress(elapsed):
    print(f"CI phase still running after {elapsed:.0f}s", flush=True)
    try:
        if sys.platform == "win32":
            windows_snapshot()
            return
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
    except (OSError, subprocess.SubprocessError) as error:
        print(f"Process snapshot unavailable: {error}", flush=True)
        detail = getattr(error, "stderr", None) or getattr(error, "stdout", None)
        if isinstance(detail, bytes):
            detail = detail.decode("utf-8", errors="replace")
        if detail:
            print(detail.strip(), flush=True)


def run(command, env=None):
    start = time.monotonic()
    print("CI phase started", flush=True)
    with subprocess.Popen(command, env=env) as process:
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
    arguments = sys.argv[1:]
    environment = None
    if arguments[:1] == ["--parsers"]:
        import ci
        environment = ci.parser_environment(Path.cwd(), None, libraries=False)
        arguments = arguments[1:]
    if len(arguments) < 2 or arguments[0] != "--":
        raise SystemExit("Usage: ci-run.py [--parsers] -- <command> [arguments...]")
    raise SystemExit(run(arguments[1:], environment))
