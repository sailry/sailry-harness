#!/usr/bin/env python3
"""Build the pinned, relocatable Office interpreter; never install into user Python."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile

PYTHON = "3.12.13"
UV = "0.10.9"
ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "crates/node-runtime/src/office/runtime/requirements.lock"
TARGETS = {
    "aarch64-apple-darwin": ("macos-aarch64-none", "lib/python3.12/site-packages", "bin/python3.12"),
    "x86_64-apple-darwin": ("macos-x86_64-none", "lib/python3.12/site-packages", "bin/python3.12"),
    "aarch64-unknown-linux-gnu": ("linux-aarch64-gnu", "lib/python3.12/site-packages", "bin/python3.12"),
    "x86_64-unknown-linux-gnu": ("linux-x86_64-gnu", "lib/python3.12/site-packages", "bin/python3.12"),
    "x86_64-pc-windows-msvc": ("windows-x86_64-none", "Lib/site-packages", "python.exe"),
    "aarch64-pc-windows-msvc": ("windows-aarch64-none", "Lib/site-packages", "python.exe"),
}


def run(*args):
    subprocess.run(args, check=True)


def build(destination, target):
    distribution, packages, executable = TARGETS[target]
    manifest = {"version": 1, "python": PYTHON, "target": target,
                "requirements_sha256": hashlib.sha256(LOCK.read_bytes()).hexdigest(),
                "executable": "python/" + executable}
    stamp = destination / "runtime.json"
    if stamp.is_file() and json.loads(stamp.read_text()) == manifest:
        if (destination / manifest["executable"]).is_file():
            return
    if destination.exists() and not stamp.is_file():
        raise SystemExit(f"Refusing to replace an unrecognized directory: {destination}")
    uv = shutil.which("uv")
    if not uv or subprocess.check_output([uv, "--version"], text=True).split()[1] != UV:
        raise SystemExit(f"Office packaging requires uv {UV}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".office-build-", dir=destination.parent) as temp:
        stage = Path(temp)
        download = stage / "download"
        run(uv, "python", "install", "--install-dir", str(download), "--no-bin", "--no-registry",
            f"cpython-{PYTHON}-{distribution}")
        installed = list(download.glob(f"cpython-{PYTHON}-*"))
        if len(installed) != 1:
            raise SystemExit("Expected exactly one standalone Python distribution")
        runtime = stage / "office-runtime"
        runtime.mkdir()
        shutil.move(installed[0], runtime / "python")
        run(uv, "pip", "install", "--target", str(runtime / "python" / packages),
            "--python-version", PYTHON, "--python-platform", target, "--only-binary", ":all:",
            "--require-hashes", "--no-compile-bytecode", "-r", str(LOCK))
        shutil.copy2(LOCK, runtime / "requirements.lock")
        shutil.copy2(LOCK.with_suffix(".in"), runtime / "requirements.in")
        # Wheels and the standalone distribution retain their bundled license files.
        if platform.system() == "Darwin" and target.endswith("apple-darwin"):
            for path in sorted((runtime / "python").rglob("*")):
                if path.is_file() and not path.is_symlink() and (path.suffix in (".so", ".dylib") or path == runtime / "python" / executable):
                    run("codesign", "--force", "--timestamp=none", "--sign", "-", str(path))
        (runtime / "runtime.json").write_text(json.dumps(manifest, indent=2) + "\n")
        if destination.exists():
            shutil.move(destination, stage / "previous")
        shutil.move(runtime, destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    build(args.output.resolve(), args.target)
    if args.archive:
        args.archive.parent.mkdir(parents=True, exist_ok=True)
        temporary = args.archive.with_suffix(".tmp")
        with tarfile.open(temporary, "w:gz") as archive:
            archive.add(args.output, arcname="office-runtime")
        os.replace(temporary, args.archive)
    print(f"Office runtime: {args.output}")


if __name__ == "__main__":
    main()
