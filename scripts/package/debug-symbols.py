#!/usr/bin/env python3
"""Preserve native symbols and optionally upload them to Sentry."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / "scripts/package/sentry-cli.lock.json"
UPLOAD_TIMEOUT = 600


def collect(binary, target, output, optional=False):
    if not binary.is_file():
        if optional:
            return None
        raise ValueError("The native executable is missing")
    output.mkdir(parents=True, exist_ok=True)
    if target.endswith("-windows-msvc"):
        # Cargo preserves crate-name underscores in the embedded PDB reference.
        source = binary.with_name(binary.stem.replace("-", "_") + ".pdb")
        if not source.is_file():
            source = binary.with_suffix(".pdb")
        if not source.is_file():
            if optional:
                return None
            raise ValueError("The native build did not produce a PDB")
        destination = output / source.name
        shutil.copy2(source, destination)
        return destination
    if target.endswith("-apple-darwin"):
        name = binary.name + ".dSYM"
        command = ["dsymutil", str(binary), "-o"]
        inspector = shutil.which("dwarfdump")
        if inspector and (output / name).is_dir() and same_uuids(inspector, binary, output / name):
            # Release packaging and its workflow request the same symbols; dsymutil is slow.
            return output / name
    elif target.endswith("-linux-gnu"):
        name = binary.name + ".debug"
        tool = shutil.which("llvm-objcopy") or shutil.which("objcopy")
        if tool is None:
            if optional:
                return None
            raise ValueError("Native symbol extraction requires llvm-objcopy or objcopy")
        command = [tool, "--only-keep-debug", str(binary)]
        inspector = shutil.which("llvm-dwarfdump")
        if inspector is None and Path(tool).with_name("llvm-dwarfdump").is_file():
            inspector = str(Path(tool).with_name("llvm-dwarfdump"))
    else:
        raise ValueError("Unsupported native symbol target")
    if inspector is None:
        if optional:
            return None
        raise ValueError("Native symbol inspection requires dwarfdump")
    # Keep completed symbols outside the package and preserve them on upload failure.
    with tempfile.TemporaryDirectory(dir=output) as directory:
        staging = Path(directory) / name
        try:
            subprocess.run(command + [str(staging)], capture_output=True, check=True)
            result = subprocess.run([inspector, "--debug-info", "--recurse-depth=0", str(staging)],
                                    capture_output=True, check=True)
        except (FileNotFoundError, subprocess.CalledProcessError):
            if optional:
                return None
            raise ValueError("Native symbol extraction failed") from None
        if not staging.exists() or b"DW_AT_stmt_list" not in result.stdout:
            if optional:
                return None
            raise ValueError("The native build did not produce debug symbols")
        destination = output / name
        if destination.is_dir():
            shutil.rmtree(destination)
        elif destination.exists():
            destination.unlink()
        staging.replace(destination)
        return destination


def same_uuids(inspector, binary, symbols):
    """Whether completed symbols already describe every architecture of this executable."""
    def uuids(path):
        try:
            result = subprocess.run([inspector, "--uuid", str(path)], capture_output=True, check=True)
        except (OSError, subprocess.CalledProcessError):
            return set()
        return set(re.findall(rb"UUID: ([0-9A-F-]{36}) \((\w+)\)", result.stdout))
    expected = uuids(binary)
    return bool(expected) and uuids(symbols) == expected


def cli(cache):
    lock = json.loads(LOCK.read_text())
    system = platform.system()
    machine = platform.machine().lower()
    architecture = {"aarch64": "arm64", "amd64": "x86_64"}.get(machine, machine)
    if system == "Linux" and architecture == "arm64":
        architecture = "aarch64"
    if system == "Windows" and architecture == "arm64":
        architecture = "aarch64"
    name = f"sentry-cli-{system}-{architecture}" + (".exe" if system == "Windows" else "")
    digest = lock["assets"].get(name)
    if digest is None:
        raise ValueError("Unsupported Sentry CLI platform")
    cache.mkdir(parents=True, exist_ok=True)
    destination = cache / name
    if not destination.is_file() or hashlib.sha256(destination.read_bytes()).hexdigest() != digest:
        url = f"https://github.com/getsentry/sentry-cli/releases/download/{lock['version']}/{name}"
        try:
            with urllib.request.urlopen(url, timeout=60) as response:
                data = response.read(64 * 1024 * 1024 + 1)
        except OSError:
            raise ValueError("Sentry CLI download failed") from None
        if len(data) > 64 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != digest:
            raise ValueError("Sentry CLI checksum mismatch")
        with tempfile.NamedTemporaryFile(dir=cache, delete=False) as temporary:
            temporary.write(data)
            staging = Path(temporary.name)
        staging.chmod(0o755)
        staging.replace(destination)
    destination.chmod(0o755)
    return destination


def upload(symbols, environment):
    required = ["SENTRY_AUTH_TOKEN", "SENTRY_ORG", "SENTRY_PROJECT"]
    if not all(environment.get(name) for name in required):
        print("Native symbols preserved; upload credentials are not configured")
        return False
    if not symbols.exists():
        raise ValueError("Native debug symbols are missing")
    try:
        executable = cli(ROOT / "target/tooling/sentry-cli")
    except OSError:
        raise ValueError("Sentry CLI preparation failed; native symbols are preserved") from None
    # The upload token stays in the environment, never argv or product metadata.
    try:
        result = subprocess.run([
            str(executable), "debug-files", "upload",
            "--org", environment["SENTRY_ORG"], "--project", environment["SENTRY_PROJECT"],
            "--no-sources", str(symbols),
        ], env={**environment, "SENTRY_LOG_LEVEL": "error"}, capture_output=True, timeout=UPLOAD_TIMEOUT)
    except subprocess.TimeoutExpired:
        raise ValueError(f"Sentry debug symbol upload timed out after {UPLOAD_TIMEOUT} seconds; "
                         "native symbols are preserved") from None
    except OSError:
        raise ValueError("Sentry CLI could not start; native symbols are preserved") from None
    if result.returncode:
        raise ValueError("Sentry debug symbol upload failed")
    print("Native symbols uploaded")
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    collection = commands.add_parser("collect", help="Preserve existing native debug symbols")
    collection.add_argument("--binary", required=True, type=Path)
    collection.add_argument("--target", required=True)
    collection.add_argument("--output", required=True, type=Path)
    collection.add_argument("--optional", action="store_true", help="Allow builds without debug symbols")
    delivery = commands.add_parser("upload", help="Upload preserved symbols when credentials are configured")
    delivery.add_argument("--symbols", required=True, type=Path)
    arguments = parser.parse_args()
    if arguments.command == "collect":
        symbols = collect(arguments.binary.resolve(), arguments.target,
                          arguments.output, arguments.optional)
        print("Native symbols preserved" if symbols else "Native debug symbols are unavailable")
    else:
        upload(arguments.symbols, dict(os.environ))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        print(str(error) if isinstance(error, ValueError) else "Native symbol operation failed", file=sys.stderr)
        raise SystemExit(1) from None
