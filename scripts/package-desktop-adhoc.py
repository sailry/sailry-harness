#!/usr/bin/env python3
"""Package public ad-hoc Desktop downloads separately from signed updates."""

import argparse
import hashlib
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile


TARGETS = {"aarch64-apple-darwin", "x86_64-apple-darwin"}


def environment():
    excluded = ("APPLE_", "SAILRY_UPDATE_", "TAURI_SIGNING_", "NOTARY")
    values = {name: value for name, value in os.environ.items()
              if not name.startswith(excluded)
              and name not in {"KEYCHAIN_PASSWORD", "MACOS_SIGNING_IDENTITY", "SENTRY_AUTH_TOKEN"}}
    values.update(SAILRY_SIGNING="ad-hoc", SAILRY_NOTARIZE="0")
    return values


def regular(path):
    return stat.S_ISREG(path.lstat().st_mode) and path.stat().st_size > 0


def prepare(source, target):
    values = environment()
    host = subprocess.run(["rustc", "-vV"], cwd=source, env=values,
                          capture_output=True, text=True, check=True)
    if f"host: {target}" not in host.stdout.splitlines():
        raise ValueError("Local Desktop distribution requires the native macOS target")
    directory = Path(values.get("CARGO_TARGET_DIR", "target"))
    if not directory.is_absolute():
        directory = source / directory
    directory = directory.resolve()
    values.update(CARGO_TARGET_DIR=str(directory), CARGO_BUILD_TARGET=target,
                  CARGO_PROFILE_RELEASE_DEBUG="line-tables-only", CARGO_PROFILE_RELEASE_STRIP="none")
    subprocess.run([sys.executable, str(source / "scripts/ci-run.py"), "--parsers", "--",
                    "cargo", "build", "--locked", "--release", "--timings", "--target", target,
                    "-p", "sailry-desktop", "-p", "sailry-release"], cwd=source, env=values, check=True)
    build = directory / target / "release"
    symbols = directory / "debug-symbols/desktop" / target
    command = [sys.executable, str(source / "scripts/package/debug-symbols.py")]
    subprocess.run(command + ["collect", "--binary", str(build / "sailry-desktop"),
                              "--target", target, "--output", str(symbols)],
                   cwd=source, env=values, check=True)
    subprocess.run(command + ["upload", "--symbols", str(symbols)], cwd=source,
                   env={**values, "SENTRY_AUTH_TOKEN": os.environ.get("SENTRY_AUTH_TOKEN", "")},
                   check=True)
    return build


def package(source, target, build=False):
    if target not in TARGETS:
        raise ValueError("Ad-hoc Desktop packaging requires a supported macOS target")
    source = source.resolve(strict=True)
    command = ["bash", str(source / "scripts/package-macos.sh"), "release", "desktop"]
    if build:
        command.append(str(prepare(source, target)))
    result = subprocess.run(command,
                            cwd=source, env=environment(), stdout=subprocess.PIPE, text=True,
                            check=True)
    print(result.stdout, end="")
    outputs = [line.removeprefix("Local package: ") for line in result.stdout.splitlines()
               if line.startswith("Local package: ")]
    if len(outputs) != 1:
        raise ValueError("Desktop packaging must report exactly one output directory")
    directory = Path(outputs[0])
    if (not directory.is_absolute() or directory.is_symlink()
            or directory.resolve(strict=True).parent != (source / "dist").resolve(strict=True)):
        raise ValueError("Desktop packaging must produce a real directory directly under dist")
    names = {f"Sailry-{target}.dmg", f"Sailry-{target}.zip", "SHA256SUMS", "Sailry.app"}
    if ({entry.name for entry in directory.iterdir()} != names
            or not (directory / "Sailry.app").is_dir()
            or (directory / "Sailry.app").is_symlink()
            or any(not regular(directory / name) for name in names - {"Sailry.app"})):
        raise ValueError("Ad-hoc packaging must contain only the target App, DMG, ZIP and checksums")
    output = Path(tempfile.mkdtemp(prefix="desktop-adhoc-", dir=source / "dist"))
    rows = []
    for extension in ("dmg", "zip"):
        name = f"Sailry-{target}-adhoc.{extension}"
        path = output / name
        (directory / f"Sailry-{target}.{extension}").rename(path)
        with path.open("rb") as file:
            digest = hashlib.file_digest(file, "sha256").hexdigest()
        rows.append(f"{digest}  {name}\n")
    (output / f"SHA256SUMS-desktop-{target}-adhoc").write_text("".join(rows))
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--target", choices=sorted(TARGETS), required=True)
    parser.add_argument("--build", action="store_true",
                        help="Build locally with line tables, collect and upload symbols before packaging")
    arguments = parser.parse_args()
    print(f"Ad-hoc package: {package(arguments.source, arguments.target, arguments.build)}")


if __name__ == "__main__":
    main()
