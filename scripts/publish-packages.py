#!/usr/bin/env python3
"""Attach verified package assets to a version's draft without publishing it."""

import argparse
import json
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).parent / "package"))
import versions


def invoke(*arguments):
    return subprocess.run(["gh", *arguments], capture_output=True, text=True)


def draft(tag, notes, assets):
    versions.parse(tag.removeprefix("v"))
    if not tag.startswith("v") or not assets:
        raise ValueError("A version tag and verified package assets are required")
    query = ("release", "view", tag, "--json", "isDraft,tagName")
    release = invoke(*query)
    if release.returncode:
        created = invoke("release", "create", tag, "--verify-tag", "--draft",
                         "--title", f"Sailry Harness {tag[1:]}", "--notes-file", str(notes),
                         *versions.release_flags(tag[1:]))
        # Other platform publishers may have created this same draft concurrently.
        release = invoke(*query)
        if release.returncode:
            raise RuntimeError(created.stderr.strip() or release.stderr.strip() or "Release draft is unavailable")
    metadata = json.loads(release.stdout)
    if not metadata.get("isDraft") or metadata.get("tagName") != tag:
        raise ValueError("Refusing to change a published release")
    # Existing assets are not replaced: a rerun must use a new version or remove
    # its conflicting draft assets explicitly after review.
    result = invoke("release", "upload", tag, *(str(asset) for asset in assets))
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "Release asset upload failed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--notes", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    arguments = parser.parse_args()
    assets = sorted(path for path in arguments.directory.iterdir() if path.is_file())
    draft(arguments.tag, arguments.notes, assets)


if __name__ == "__main__":
    main()
