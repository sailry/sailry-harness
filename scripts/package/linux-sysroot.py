#!/usr/bin/env python3
"""Prepare pinned X11 link inputs for Linux Hosts without installing system packages."""

import argparse
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
from urllib.request import urlopen

LOCK = Path(__file__).with_name("linux-sysroot.lock.json")
TARGETS = {"x86_64-unknown-linux-gnu": "amd64", "aarch64-unknown-linux-gnu": "arm64"}


def extract(data, expected, ar, destination):
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("Linux sysroot package checksum mismatch")
    with tempfile.TemporaryDirectory(prefix="sailry-deb-") as directory:
        package = Path(directory) / "package.deb"
        package.write_bytes(data)
        members = subprocess.check_output([ar, "t", str(package)], text=True).splitlines()
        member = next(name for name in members if name.startswith("data.tar."))
        content = subprocess.check_output([ar, "p", str(package), member])
        with tarfile.open(fileobj=io.BytesIO(content)) as archive:
            archive.extractall(destination, filter="data")


def prepare(target, output, ar):
    digest = hashlib.sha256(LOCK.read_bytes()).hexdigest()
    stamp = output / "sysroot.json"
    manifest = {"version": 1, "target": target, "lock_sha256": digest}
    if stamp.is_file() and json.loads(stamp.read_text()) == manifest:
        return
    if output.exists() and not stamp.is_file():
        raise ValueError(f"Refusing to replace an unrecognized sysroot: {output}")
    lock = json.loads(LOCK.read_text())
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".sysroot-", dir=output.parent) as directory:
        stage = Path(directory)
        root = stage / "root"
        root.mkdir()
        for package in lock["packages"][TARGETS[target]]:
            with urlopen(lock["source"] + package["filename"], timeout=120) as response:
                extract(response.read(), package["sha256"], ar, root)
        (root / "sysroot.json").write_text(json.dumps(manifest, indent=2) + "\n")
        if output.exists():
            shutil.move(output, stage / "previous")
        shutil.move(root, output)
    print(f"Linux link sysroot: {target}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ar", required=True)
    args = parser.parse_args()
    prepare(args.target, args.output.resolve(), args.ar)


if __name__ == "__main__":
    main()
