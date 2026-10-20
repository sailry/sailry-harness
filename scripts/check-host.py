#!/usr/bin/env python3
"""Verify an existing Host binary using a fresh private profile and no external service."""

import argparse
import json
from pathlib import Path
import queue
import re
import shutil
import subprocess
import tempfile
import threading


def check_linux(binary):
    with binary.open("rb") as stream:
        if stream.read(4) != b"\x7fELF":
            return
    recorded = set()
    for name in ("onnx-runtime.json", "llama-runtime.json"):
        manifest = binary.parent / name
        if manifest.is_symlink():
            raise ValueError("Runtime manifest must be an ordinary file")
        if manifest.is_file():
            recorded.update(file["name"] for file in json.loads(manifest.read_text())["files"])
    libraries = sorted(binary.parent.glob("*.so*"))
    for library in libraries:
        if library.is_symlink() or not library.is_file():
            raise ValueError(f"Runtime library must be an ordinary file: {library.name}")
        if library.name not in recorded:
            raise ValueError(f"Runtime library has no manifest entry: {library.name}")
    tool = shutil.which("readelf")
    if tool:
        arguments = [tool, "--version-info"]
    else:
        tool = shutil.which("llvm-objdump") or shutil.which("objdump")
        if not tool:
            raise RuntimeError("readelf or objdump is required to verify Linux compatibility")
        arguments = [tool, "-T"]
    for path in [binary, *libraries]:
        output = subprocess.run([*arguments, str(path)], check=True, capture_output=True,
                                text=True).stdout
        versions = [tuple(map(int, match.split(".")))
                    for match in re.findall(r"\bGLIBC_(\d+(?:\.\d+)+)\b", output)]
        if any(version > (2, 34) for version in versions):
            raise ValueError(f"Linux binary requires glibc newer than 2.34: {path.name}")


def check(binary):
    binary = binary.resolve(strict=True)
    check_linux(binary)
    subprocess.run([str(binary), "--version"], check=True)
    subprocess.run([str(binary), "--help"], check=True)
    with tempfile.TemporaryDirectory(prefix="sailry-host-check-") as directory:
        profile = Path(directory) / "profile"
        host = subprocess.Popen([str(binary), "--data-dir", str(profile)],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        lines = queue.Queue()
        def read():
            for line in host.stdout:
                lines.put(line.rstrip())
        threading.Thread(target=read, daemon=True).start()
        try:
            if not lines.get(timeout=30).startswith("Node ready:"):
                raise RuntimeError("Host did not announce readiness")
            if not lines.get(timeout=5).startswith("Link address:"):
                raise RuntimeError("Host did not announce its endpoint")
            if not (profile / "control.sock").is_socket():
                raise RuntimeError("Host control socket is unavailable")
            competing = subprocess.run([str(binary), "--data-dir", str(profile)],
                                       capture_output=True, text=True, timeout=30)
            if competing.returncode == 0 or "a Node already owns profile" not in competing.stderr:
                raise RuntimeError("Host did not enforce exclusive profile ownership")
            host.terminate()
            if host.wait(timeout=30) != 0 or lines.get(timeout=5) != "Node stopped":
                raise RuntimeError("Host did not shut down cleanly")
            if (profile / "control.sock").exists():
                raise RuntimeError("Host left its control socket after shutdown")
        finally:
            if host.poll() is None:
                host.kill()
                host.wait(timeout=10)
            host.stdout.close()
            host.stderr.close()
    print("Host startup, exclusive ownership and shutdown verified")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    check(parser.parse_args().binary)


if __name__ == "__main__":
    main()
