#!/usr/bin/env python3
"""Sign native code in a fresh macOS application and separate Host package."""

import argparse
import importlib.util
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
MAGIC = {
    b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe",
    b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca", b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca",
}

runtime_spec = importlib.util.spec_from_file_location("onnx_runtime", Path(__file__).with_name("onnx-runtime.py"))
runtime = importlib.util.module_from_spec(runtime_spec)
runtime_spec.loader.exec_module(runtime)
python_spec = importlib.util.spec_from_file_location("python_runtime", Path(__file__).with_name("python-runtime.py"))
python_runtime = importlib.util.module_from_spec(python_spec)
python_spec.loader.exec_module(python_runtime)


def run(*args):
    result = subprocess.run([str(arg) for arg in args], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or
                           f"{args[0]} failed with exit code {result.returncode}")
    return result.stdout + result.stderr


def mach_objects(directory):
    objects = []
    for path in directory.rglob("*"):
        if path.is_file() and not path.is_symlink():
            with path.open("rb") as file:
                if file.read(4) in MAGIC:
                    objects.append(path)
    return sorted(objects, key=lambda path: (-len(path.parts), str(path)))


def verify(path, team, deep=False):
    options = ["--deep"] if deep else []
    run("codesign", "--verify", "--strict", *options, path)
    metadata = run("codesign", "--display", "--verbose=4", path)
    if f"TeamIdentifier={team}\n" not in metadata:
        raise RuntimeError(f"Unexpected signing team: {path.name}")
    if "(runtime)" not in metadata or "Timestamp=" not in metadata:
        raise RuntimeError(f"Missing hardened runtime or secure timestamp: {path.name}")


def sign(app, host, target, identity, team, keychain=None):
    if not app.is_dir() or not (host / "sailry-host").is_file():
        raise RuntimeError("Expected a complete Sailry application package")
    options = ["--force", "--sign", identity, "--timestamp", "--options", "runtime"]
    if keychain:
        options.extend(["--keychain", str(keychain)])
    python_runtime.unlock(app / "Contents/MacOS")
    python_runtime.unlock(host)
    objects = mach_objects(app)
    if not objects:
        raise RuntimeError("The application contains no Mach-O code")
    for path in objects:
        run("codesign", *options, *python_runtime.signing_options(path, app / "Contents/MacOS"), path)
        verify(path, team)
    runtime.finalize(target, app / "Contents/MacOS", speech=True)
    python_runtime.finalize(target, app / "Contents/MacOS")
    run("codesign", *options, "--entitlements", ROOT / "apps/desktop/macos/entitlements.plist", app)
    verify(app, team, deep=True)
    # Host is a separate release asset, never a remote deployment payload inside Desktop.
    for path in mach_objects(host):
        run("codesign", *options, *python_runtime.signing_options(path, host), path)
        verify(path, team)
    runtime.finalize(target, host, speech=False)
    python_runtime.finalize(target, host)
    print(f"Developer ID signing verified: {len(objects)} Mach-O files and Sailry.app")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--keychain", type=Path, default=os.environ.get("APPLE_SIGNING_KEYCHAIN"))
    args = parser.parse_args()
    identity = os.environ.get("APPLE_SIGNING_IDENTITY")
    team = os.environ.get("APPLE_TEAM_ID")
    if not identity or not team:
        parser.error("APPLE_SIGNING_IDENTITY and APPLE_TEAM_ID are required")
    sign(args.app, args.host, args.target, identity, team, args.keychain)


if __name__ == "__main__":
    main()
