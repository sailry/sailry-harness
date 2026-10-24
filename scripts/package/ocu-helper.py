#!/usr/bin/env python3
"""Build and stage the pinned native computer-use helper, never a runtime download."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import tarfile
import tempfile
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[2]
LOCK = Path(__file__).with_name("ocu-helper.lock.json")
APP = "Sailry Computer Use.app"
RECEIPT = "sailry-ocu.json"
SPEC = importlib.util.spec_from_file_location("onnx_runtime", Path(__file__).with_name("onnx-runtime.py"))
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)


def run(*command, **options):
    subprocess.run([str(value) for value in command], check=True, **options)


def specification(target):
    lock = json.loads(LOCK.read_text())
    if lock["version"] != 1 or re.fullmatch(r"[0-9a-f]{40}", lock["revision"]) is None:
        raise ValueError("Invalid computer-use source revision")
    if target not in lock["targets"]:
        raise ValueError(f"Unsupported computer-use target: {target}")
    patches = []
    for patch in lock["patches"]:
        path = LOCK.parent / patch["path"]
        if not path.resolve().is_relative_to(LOCK.parent.resolve()):
            raise ValueError("Computer-use patch escapes its source directory")
        NATIVE.ordinary(path)
        if NATIVE.digest(path) != patch["sha256"]:
            raise ValueError("Computer-use patch checksum mismatch")
        patches.append(path)
    return lock, lock["targets"][target], patches


def toolchain(asset):
    if asset["platform"] == "macos":
        if platform.system() != "Darwin":
            raise ValueError("The macOS helper requires a macOS builder")
        result = subprocess.check_output(["xcrun", "swift", "--version"], text=True)
        match = re.search(r"Swift version (\d+)\.(\d+)", result)
        if match is None or tuple(map(int, match.groups())) < (6, 2):
            raise ValueError("The computer-use helper requires Swift 6.2 or newer")
    else:
        result = subprocess.check_output(["go", "version"], text=True)
        match = re.search(r"go(\d+)\.(\d+)", result)
        if match is None or tuple(map(int, match.groups())) < (1, 22):
            raise ValueError("The Windows helper requires Go 1.22 or newer")
        run("node", "--version")
    return result.strip()


def archive(lock, cache):
    source = lock["source"]
    path = cache / "archives" / (source["sha256"] + ".tar.gz")
    if path.exists() or path.is_symlink():
        NATIVE.verify(path, source)
        return path.read_bytes()
    with urlopen(source["url"], timeout=120) as response:
        data = response.read(source["size"] + 1)
    if len(data) != source["size"] or hashlib.sha256(data).hexdigest() != source["sha256"]:
        raise ValueError("Computer-use source archive checksum mismatch")
    NATIVE.atomic(path, data)
    return data


def source_tree(lock, patches, cache, identity):
    directory = cache / "sources" / identity
    marker = directory / ".sailry-source.json"
    if directory.exists():
        if directory.is_symlink() or not marker.is_file() or json.loads(marker.read_text()) != lock:
            raise ValueError("Existing computer-use source cache must be inspected before reuse")
        return directory
    directory.mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(archive(lock, cache)), mode="r:gz") as bundle:
        prefix = f"open-computer-use-{lock['revision']}/"
        members = bundle.getmembers()
        if any(entry.name != prefix.rstrip("/") and not entry.name.startswith(prefix) for entry in members):
            raise ValueError("Computer-use source archive has an unexpected root")
        for entry in members:
            entry.name = "" if entry.name == prefix.rstrip("/") else entry.name.removeprefix(prefix)
            if entry.name:
                bundle.extract(entry, directory, filter="data")
    run("git", "init", "--quiet", directory)
    for patch in patches:
        run("git", "apply", "--check", patch, cwd=directory)
        run("git", "apply", patch, cwd=directory)
    NATIVE.atomic(marker, (json.dumps(lock, sort_keys=True) + "\n").encode())
    return directory


def executable(directory, asset):
    if asset["platform"] == "macos":
        return directory / APP / "Contents/MacOS/OpenComputerUse"
    return directory / "open-computer-use.exe"


def architecture(path, asset):
    NATIVE.ordinary(path)
    if asset["platform"] == "macos":
        run("lipo", "-verify_arch", asset["arch"], path)
        return
    with path.open("rb") as binary:
        if binary.read(2) != b"MZ":
            raise ValueError("Computer-use helper is not a Windows executable")
        binary.seek(0x3c)
        offset = struct.unpack("<I", binary.read(4))[0]
        binary.seek(offset)
        if binary.read(4) != b"PE\0\0":
            raise ValueError("Computer-use helper has an invalid PE header")
        machine = struct.unpack("<H", binary.read(2))[0]
    if machine != {"amd64": 0x8664, "arm64": 0xaa64}[asset["arch"]]:
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


def build(target, cache, development=False):
    lock, asset, patches = specification(target)
    compiler = toolchain(asset)
    inputs = {"lock": lock, "target": target, "development": development,
              "compiler": compiler, "icon": NATIVE.digest(ROOT / "assets/branding/Sailry.icns")}
    identity = hashlib.sha256(json.dumps(inputs, sort_keys=True).encode()).hexdigest()
    output = cache / "artifacts" / identity
    receipt = output / RECEIPT
    if receipt.is_file():
        expected = json.loads(receipt.read_text())
        if expected["inputs"] != inputs or expected["files"] != inventory(output):
            raise ValueError("Computer-use artifact cache checksum mismatch")
        architecture(executable(output, asset), asset)
        return output
    if output.exists():
        raise ValueError("Incomplete computer-use artifact cache must be inspected before reuse")
    source = source_tree(lock, patches, cache, identity)
    output.mkdir(parents=True)
    if asset["platform"] == "macos":
        environment = dict(os.environ, OPEN_COMPUTER_USE_CODESIGN_MODE="none")
        run("bash", source / "scripts/build-open-computer-use-app.sh", "debug" if development else "release",
            "--arch", asset["arch"], cwd=source, env=environment)
        shutil.copytree(source / "dist" / APP, output / APP)
        resources = output / APP / "Contents/Resources"
        shutil.copyfile(ROOT / "assets/branding/Sailry.icns", resources / "OpenComputerUse.icns")
        shutil.copyfile(source / "LICENSE", resources / "open-computer-use-LICENSE")
    else:
        run("bash", source / "scripts/build-open-computer-use-windows.sh", "--arch", asset["arch"],
            "--out-dir", output / "build", cwd=source)
        shutil.move(output / "build" / asset["arch"] / "open-computer-use.exe", output)
        shutil.rmtree(output / "build")
        shutil.copyfile(source / "LICENSE", output / "open-computer-use-LICENSE")
        shutil.copyfile(source / "THIRD_PARTY_NOTICES.md", output / "open-computer-use-NOTICE")
    architecture(executable(output, asset), asset)
    NATIVE.atomic(receipt, (json.dumps({"version": 1, "inputs": inputs, "files": inventory(output)},
                                     indent=2) + "\n").encode())
    return output


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


def relocate(binary):
    NATIVE.ordinary(binary)
    commands = subprocess.check_output(["otool", "-l", str(binary)], text=True)
    paths = re.findall(r"cmd LC_RPATH\s+cmdsize \d+\s+path (.+?) \(offset \d+\)", commands)
    for path in dict.fromkeys(paths):
        if re.fullmatch(r"/.+\.xctoolchain/usr/lib/swift(?:-\d+(?:\.\d+)*)?/macosx", path):
            # Swift also records its build toolchain, while runtime dylibs use /usr/lib/swift.
            run("install_name_tool", "-delete_rpath", path, binary)


def sign(output, identity, timestamp=False):
    app = output / APP
    binary = app / "Contents/MacOS/OpenComputerUse"
    NATIVE.ordinary(binary)
    options = ["--force", "--sign", identity, "--timestamp" if timestamp else "--timestamp=none"]
    if identity != "-":
        options.extend(["--options", "runtime"])
    run("codesign", *options, "--preserve-metadata=entitlements", binary)
    run("codesign", *options, "--preserve-metadata=entitlements", app)
    run("codesign", "--verify", "--deep", "--strict", app)


def copy(source, destination):
    source, destination = Path(source), Path(destination)
    NATIVE.ordinary(source)
    if destination.is_symlink():
        raise ValueError("Computer-use helper destination is a symlink")
    pending = None
    try:
        with tempfile.NamedTemporaryFile(prefix=".ocu-", dir=destination.parent, delete=False) as temporary:
            pending = Path(temporary.name)
        shutil.copy2(source, pending)
        # A persistent app agent can still map the previous signed executable.
        # Publish a new inode before signing; never overwrite its mapped pages.
        os.replace(pending, destination)
    finally:
        if pending is not None:
            pending.unlink(missing_ok=True)
    return str(destination)


def stage(target, output, cache, development=False, identity=None):
    source = build(target, cache, development)
    asset = specification(target)[1]
    output.mkdir(parents=True, exist_ok=True)
    if output.is_symlink():
        raise ValueError("Computer-use helper destination is a symlink")
    receipt = receipt_path(output)
    receipt.parent.mkdir(parents=True, exist_ok=True)
    if receipt != output / RECEIPT:
        (output / RECEIPT).unlink(missing_ok=True)
    for path in source.iterdir():
        destination = receipt if path.name == RECEIPT else output / path.name
        if destination.is_symlink():
            raise ValueError("Computer-use helper destination is a symlink")
        if path.is_dir():
            shutil.copytree(path, destination, dirs_exist_ok=True, copy_function=copy)
        else:
            copy(path, destination)
    binary = executable(output, asset)
    if asset["platform"] == "macos":
        relocate(binary)
    if identity is not None:
        if asset["platform"] != "macos":
            raise ValueError("Computer-use helper signing requires macOS")
        sign(output, identity)
    finalize(output)
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True, help="Destination helpers directory")
    parser.add_argument("--cache", type=Path, default=ROOT / "target/ocu-helper-cache")
    parser.add_argument("--development", action="store_true")
    parser.add_argument("--sign")
    arguments = parser.parse_args()
    path = stage(arguments.target, arguments.output.absolute(), arguments.cache.absolute(),
                 arguments.development, arguments.sign)
    print(f"Computer-use helper: {path}")


if __name__ == "__main__":
    main()
