#!/usr/bin/env python3
"""Create a fresh drag-to-Applications disk image without installing the app."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def run(*arguments):
    subprocess.run([str(argument) for argument in arguments], check=True)


def create(app, output, signing="ad-hoc"):
    if app.name != "Sailry.app" or not app.is_dir() or output.exists():
        raise ValueError("Expected Sailry.app and a new disk image path")
    with tempfile.TemporaryDirectory(prefix="sailry-dmg-", dir=output.parent) as temporary:
        stage = Path(temporary)
        shutil.copytree(app, stage / app.name, symlinks=True)
        (stage / "Applications").symlink_to("/Applications")
        run("hdiutil", "create", "-volname", "Sailry", "-srcfolder", stage,
            "-fs", "HFS+", "-format", "UDZO", output)
    if signing == "developer-id":
        identity = os.environ.get("APPLE_SIGNING_IDENTITY")
        if not identity:
            raise ValueError("APPLE_SIGNING_IDENTITY is required")
        arguments = ["codesign", "--sign", identity, "--timestamp", "--identifier", "ai.sailry.disk-image"]
        if os.environ.get("APPLE_SIGNING_KEYCHAIN"):
            arguments += ["--keychain", os.environ["APPLE_SIGNING_KEYCHAIN"]]
        run(*arguments, output)
        run("codesign", "--verify", "--strict", output)
    run("hdiutil", "verify", output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--signing", choices=("ad-hoc", "developer-id"), default="ad-hoc")
    arguments = parser.parse_args()
    create(arguments.app, arguments.output, arguments.signing)


if __name__ == "__main__":
    main()
