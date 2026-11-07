#!/usr/bin/env python3
"""Stage the pinned official agent-desktop computer-use helper, never a runtime download."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import re
import shutil
import struct
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
LOCK = Path(__file__).with_name("agent-desktop.lock.json")
LICENSE = ROOT / "third_party_licenses/agent-desktop-Apache-2.0.txt"
ICON = ROOT / "assets/branding/Sailry.icns"
APP = "Sailry Computer Use.app"
RECEIPT = "sailry-agent-desktop.json"
# Existing Accessibility and Screen Recording grants belong to this bundle identity.
IDENTIFIER = "dev.sailry.computer"
NAME = "Sailry Computer Use"
SPEC = importlib.util.spec_from_file_location("onnx_runtime", Path(__file__).with_name("onnx-runtime.py"))
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)


def run(*command, **options):
    subprocess.run([str(value) for value in command], check=True, **options)


def specification(target):
    lock = json.loads(LOCK.read_text())
    if lock["version"] != 1 or re.fullmatch(r"[0-9a-f]{40}", lock["revision"]) is None:
        raise ValueError("Invalid computer-use helper revision")
    if target not in lock["targets"]:
        raise ValueError(f"Unsupported computer-use target: {target}")
    return lock, lock["targets"][target]


def extract(data, asset):
    """Return every pinned member; the archive may contain nothing else."""
    result = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        members = archive.getmembers()
        if sorted(member.name for member in members) != sorted(file["member"] for file in asset["files"]):
            raise ValueError("Computer-use archive has unexpected entries")
        for file in asset["files"]:
            name = file["name"]
            if name != PurePosixPath(name).name or "\\" in name:
                raise ValueError("Computer-use helper file name is invalid")
            member = archive.getmember(file["member"])
            if not member.isfile() or member.size != file["size"]:
                raise ValueError(f"Computer-use archive entry is invalid: {name}")
            with archive.extractfile(member) as stream:
                content = stream.read(file["size"] + 1)
            if len(content) != file["size"] or hashlib.sha256(content).hexdigest() != file["sha256"]:
                raise ValueError(f"Computer-use helper checksum mismatch: {name}")
            result[name] = content
    return result


def layout(asset):
    """Paths of the staged files relative to the helpers directory."""
    files = {}
    for file in asset["files"]:
        name = file["name"]
        # agent-desktop finds its platform helper beside its own executable.
        files[name] = f"{APP}/Contents/MacOS/{name}" if asset["platform"] == "macos" else name
    if asset["platform"] == "macos":
        files["Info.plist"] = f"{APP}/Contents/Info.plist"
        files["icon"] = f"{APP}/Contents/Resources/Sailry.icns"
        files["license"] = f"{APP}/Contents/Resources/agent-desktop-LICENSE"
    else:
        files["license"] = "agent-desktop-LICENSE"
    return files


def info(lock, development=False):
    return {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleDisplayName": NAME + (" (Dev)" if development else ""),
        "CFBundleExecutable": "agent-desktop",
        "CFBundleIconFile": "Sailry.icns",
        "CFBundleIdentifier": IDENTIFIER + (".dev" if development else ""),
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": NAME,
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": lock["release"],
        "CFBundleVersion": lock["release"],
        "LSMinimumSystemVersion": "13.0",
        "LSUIElement": True,
        "NSHighResolutionCapable": True,
    }


def architecture(path, asset):
    NATIVE.ordinary(path)
    if asset["platform"] == "macos":
        run("lipo", path, "-verify_arch", asset["arch"])
        return
    with path.open("rb") as binary:
        if asset["platform"] == "windows":
            if binary.read(2) != b"MZ":
                raise ValueError("Computer-use helper is not a Windows executable")
            binary.seek(0x3c)
            binary.seek(struct.unpack("<I", binary.read(4))[0])
            if binary.read(4) != b"PE\0\0":
                raise ValueError("Computer-use helper has an invalid PE header")
            machine = struct.unpack("<H", binary.read(2))[0]
            expected = {"amd64": 0x8664, "arm64": 0xaa64}[asset["arch"]]
        else:
            header = binary.read(20)
            if len(header) != 20 or header[:4] != b"\x7fELF" or header[4] != 2 or header[5] != 1:
                raise ValueError("Computer-use helper is not a 64-bit Linux executable")
            machine = struct.unpack("<H", header[18:20])[0]
            expected = {"x86_64": 62, "aarch64": 183}[asset["arch"]]
    if machine != expected:
        raise ValueError("Computer-use helper architecture does not match its target")


def inventory(directory):
    files = []
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise ValueError("Computer-use artifact contains a symbolic link")
        if path.is_file() and path.name != RECEIPT:
            files.append({"path": path.relative_to(directory).as_posix(),
                          "size": path.stat().st_size, "sha256": NATIVE.digest(path)})
    return files


def receipt_path(output):
    if output.name == "Helpers" and output.parent.name == "Contents":
        return output.parent / "Resources" / RECEIPT
    return output / RECEIPT


def finalize(output):
    receipt = receipt_path(output)
    if receipt.is_file():
        value = json.loads(receipt.read_text())
        value["files"] = inventory(output)
        NATIVE.atomic(receipt, (json.dumps(value, indent=2) + "\n").encode())


def publish(path, content, executable=False):
    """Publish a new inode so a running helper never sees its mapped pages change."""
    if path.is_symlink():
        raise ValueError("Computer-use helper destination is a symlink")
    path.parent.mkdir(parents=True, exist_ok=True)
    pending = None
    try:
        with tempfile.NamedTemporaryFile(prefix=".agent-desktop-", dir=path.parent, delete=False) as temporary:
            pending = Path(temporary.name)
            temporary.write(content)
        os.chmod(pending, 0o755 if executable else 0o644)
        os.replace(pending, path)
    finally:
        if pending is not None:
            pending.unlink(missing_ok=True)


def sign(output, identity, timestamp=False):
    app = output / APP
    options = ["--force", "--sign", identity, "--timestamp" if timestamp else "--timestamp=none"]
    if identity != "-":
        options.extend(["--options", "runtime"])
    # Nested code is signed before the bundle that seals it.
    for path in sorted((app / "Contents/MacOS").iterdir()):
        if path.name != "agent-desktop":
            NATIVE.ordinary(path)
            run("codesign", *options, path)
    run("codesign", *options, app)
    run("codesign", "--verify", "--deep", "--strict", app)


def stage(target, output, cache, development=False, identity=None):
    lock, asset = specification(target)
    if output.is_symlink():
        raise ValueError("Computer-use helper destination is a symlink")
    output.mkdir(parents=True, exist_ok=True)
    content = extract(NATIVE.archive_bytes(lock, asset, cache), asset)
    content["license"] = LICENSE.read_bytes()
    if asset["platform"] == "macos":
        app = output / APP
        if app.is_symlink():
            raise ValueError("Computer-use helper destination is a symlink")
        # Rebuild the bundle so no earlier helper's code remains under its seal.
        shutil.rmtree(app, ignore_errors=True)
        content["Info.plist"] = plistlib.dumps(info(lock, development))
        content["icon"] = ICON.read_bytes()
    executables = {file["name"] for file in asset["files"]}
    paths = layout(asset)
    for name, relative in paths.items():
        publish(output / relative, content[name], name in executables)
    for name in executables:
        architecture(output / paths[name], asset)
    if identity is not None:
        if asset["platform"] != "macos":
            raise ValueError("Computer-use helper signing requires macOS")
        sign(output, identity)
    receipt = receipt_path(output)
    if receipt != output / RECEIPT:
        (output / RECEIPT).unlink(missing_ok=True)
    value = {"version": 1, "release": lock["release"], "revision": lock["revision"],
             "target": target, "development": development, "files": []}
    NATIVE.atomic(receipt, (json.dumps(value, indent=2) + "\n").encode())
    finalize(output)
    return output / paths[asset["executable"]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True, help="Destination helpers directory")
    parser.add_argument("--cache", type=Path, default=ROOT / "target/agent-desktop-cache")
    parser.add_argument("--development", action="store_true")
    parser.add_argument("--sign")
    arguments = parser.parse_args()
    path = stage(arguments.target, arguments.output.absolute(), arguments.cache.absolute(),
                 arguments.development, arguments.sign)
    print(f"Computer-use helper: {path}")


if __name__ == "__main__":
    main()
