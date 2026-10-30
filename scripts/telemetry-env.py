#!/usr/bin/env python3
"""Load private local telemetry build inputs without evaluating shell code."""
import os
from pathlib import Path
import shlex
import sys

KEYS = {"SAILRY_SENTRY_DSN", "SAILRY_TELEMETRY_KEY"}


def environment(root, inherited):
    result = dict(inherited)
    source = root / ".env.telemetry"
    if not source.is_file():
        return result
    for line in source.read_text().splitlines():
        name, separator, value = line.strip().removeprefix("export ").partition("=")
        if not separator or name not in KEYS or name in result:
            continue
        parts = shlex.split(value, comments=True)
        if len(parts) > 1:
            raise ValueError("Invalid local telemetry environment value")
        result[name] = parts[0] if parts else ""
    return result


def main():
    command = sys.argv[1:]
    if not command:
        raise SystemExit("Usage: telemetry-env.py <command> [arguments]")
    values = environment(Path(__file__).resolve().parent.parent, os.environ)
    os.execvpe(command[0], command, values)


if __name__ == "__main__":
    main()
