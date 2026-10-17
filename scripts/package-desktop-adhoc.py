#!/usr/bin/env python3
"""Package public ad-hoc Desktop downloads separately from signed updates."""

import argparse
import hashlib
import os
from pathlib import Path
import stat
import subprocess
import tempfile


TARGETS = {"aarch64-apple-darwin", "x86_64-apple-darwin"}


def environment():
    excluded = ("APPLE_", "SAILRY_UPDATE_", "TAURI_SIGNING_", "NOTARY")
    values = {name: value for name, value in os.environ.items()
              if not name.startswith(excluded)
              and name not in {"KEYCHAIN_PASSWORD", "MACOS_SIGNING_IDENTITY"}}
    values.update(SAILRY_SIGNING="ad-hoc", SAILRY_NOTARIZE="0")
    return values


def regular(path):
    return stat.S_ISREG(path.lstat().st_mode) and path.stat().st_size > 0


def package(source, target):
    if target not in TARGETS:
        raise ValueError("Ad-hoc Desktop packaging requires a supported macOS target")
    source = source.resolve(strict=True)
    result = subprocess.run(["bash", str(source / "scripts/package-macos.sh"), "release", "desktop"],
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
    arguments = parser.parse_args()
    print(f"Ad-hoc package: {package(arguments.source, arguments.target)}")


if __name__ == "__main__":
    main()
