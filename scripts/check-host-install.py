#!/usr/bin/env python3
"""Verify release installation and systemd controls on an isolated Linux CI runner."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
UNIT = Path("/etc/systemd/system/sailry-host.service")
COMMAND = Path("/usr/local/bin/sailry")


def run(*arguments, environment=None, capture=False):
    return subprocess.run([str(argument) for argument in arguments], env=environment,
                          check=True, capture_output=capture, text=True, timeout=60)


def ready(home, previous=None):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        active = subprocess.run(["systemctl", "is-active", "--quiet", "sailry-host.service"])
        pid = run("systemctl", "show", "--property=MainPID", "--value", "sailry-host.service", capture=True).stdout.strip()
        profile = home / ".sailry-host"
        if (active.returncode == 0 and pid not in {"0", "", previous}
                and (profile / "control.sock").is_socket()
                and (profile / "bootstrap.ticket").is_file()):
            return pid
        time.sleep(0.1)
    raise RuntimeError("Installed Host did not become ready")


def check(directory):
    if sys.platform != "linux" or os.geteuid() != 0 or not Path("/run/systemd/system").is_dir():
        raise ValueError("An isolated root-owned Linux systemd runner is required")
    existing = subprocess.run(["systemctl", "cat", "sailry-host.service"], capture_output=True)
    if UNIT.exists() or UNIT.is_symlink() or COMMAND.exists() or COMMAND.is_symlink() or existing.returncode == 0:
        raise ValueError("Refusing to replace an existing Host service or command")
    metadata = json.loads((directory / "host/build.json").read_text())
    version, target = metadata["application_version"], metadata["target"]
    archive = f"sailry-host-{version}-{target}.tar.gz"
    for name in (archive, f"SHA256SUMS-{target}"):
        if not (directory / name).is_file():
            raise ValueError("A complete Host release package is required")
    home = Path(tempfile.mkdtemp(prefix="sailry-systemd-check-"))
    bin = home / ".fixture/bin"
    assets = home / ".fixture/assets"
    bin.mkdir(parents=True)
    assets.mkdir()
    # Replace only downloads with exact package bytes; systemd and Host are real.
    shutil.copyfile(ROOT / "crates/node-runtime/tests/ssh/install/download.sh", bin / "curl")
    (bin / "curl").chmod(0o755)
    for name in (archive, f"SHA256SUMS-{target}"):
        (assets / name).symlink_to(directory / name)
    environment = {**os.environ, "HOME": str(home), "PATH": f"{bin}:{os.environ['PATH']}"}
    try:
        run("bash", ROOT / "scripts/install-host.sh", "--version", version, environment=environment)
        pid = ready(home)
        output = run(COMMAND, "version", environment=environment, capture=True).stdout.strip()
        if output != f"Sailry Host {version}":
            raise RuntimeError("Installed Host version does not match")
        run(COMMAND, "status", environment=environment, capture=True)
        marker = home / ".sailry-host/fixture-data"
        marker.write_bytes(b"preserve")
        run(COMMAND, "restart", environment=environment)
        ready(home, pid)
        run(COMMAND, "stop", environment=environment)
        if (home / ".sailry-host/control.sock").exists():
            raise RuntimeError("Stopped Host left its control socket")
        run(COMMAND, "start", environment=environment)
        pid = ready(home)
        run(COMMAND, "update", "--version", version, environment=environment)
        ready(home, pid)
        if marker.read_bytes() != b"preserve":
            raise RuntimeError("Host update changed existing profile data")
        backups = list((home / ".local/lib").glob(".sailry-previous-*"))
        if len(backups) != 1 or not (backups[0] / "sailry-host").is_file():
            raise RuntimeError("Host update did not preserve previous program files")
        print("Host installation, systemd controls and profile-preserving update verified")
    finally:
        if UNIT.is_file() and str(home / ".local/lib/sailry") in UNIT.read_text():
            run("systemctl", "disable", "--now", "sailry-host.service")
            UNIT.unlink()
            run("systemctl", "daemon-reload")
        if COMMAND.is_symlink() and COMMAND.readlink() == home / ".local/lib/sailry/sailry":
            COMMAND.unlink()
        shutil.rmtree(home)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    check(parser.parse_args().directory.resolve(strict=True))


if __name__ == "__main__":
    main()
