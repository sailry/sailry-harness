#!/usr/bin/env python3
"""Append one product's verified packages to its published version release."""

import argparse
from pathlib import Path
import os
import sys

sys.path.insert(0, str(Path(__file__).parent / "package"))
import publication
import versions

MACOS = ("aarch64-apple-darwin", "x86_64-apple-darwin")
LINUX = ("aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu")


def desktop_files(target, adhoc=False):
    suffix = "-adhoc" if adhoc else ""
    archive = f"Sailry-{target}{suffix}"
    return {archive + ".dmg", archive + ".zip", f"SHA256SUMS-desktop-{target}{suffix}"} | (
        set() if adhoc else {archive + ".zip.update.json"})


def filenames(component, signed_arm=False):
    if component == "desktop":
        return set().union(*(desktop_files(target) for target in MACOS))
    if component == "desktop-adhoc":
        return set().union(*(desktop_files(target, adhoc=not (signed_arm and target == MACOS[0]))
                             for target in MACOS))
    return {"install-host.sh", "SHA256SUMS-installer"} | {
        name for target in (*MACOS, *LINUX) for name in (
            f"sailry-host-{target}.tar.gz", f"SHA256SUMS-{target}")}


def verify_assets(component, tag, assets, signed_arm=False):
    versions.parse(tag.removeprefix("v"))
    if not tag.startswith("v") or {path.name for path in assets} != filenames(component, signed_arm):
        raise ValueError(f"The complete {component.title()} asset set for this tag is required")
    if any(not path.is_file() or path.is_symlink() for path in assets):
        raise ValueError("Release assets must be regular files")


def publish(component, tag, assets, repository, commit, client=None, signed_arm=False):
    client = client or publication.GitHub()
    verify_assets(component, tag, assets, signed_arm)
    metadata = publication.release(client, repository, tag, commit, published=True)
    publication.append(client, repository, tag, metadata, assets)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", choices=("desktop", "desktop-adhoc", "host"), required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--signed-arm", action="store_true")
    arguments = parser.parse_args()
    if arguments.signed_arm and arguments.component != "desktop-adhoc":
        parser.error("Signed ARM preservation only applies to ad-hoc Desktop packages")
    commit = publication.tagged_commit(arguments.tag, arguments.source)
    assets = sorted(arguments.directory.iterdir())
    publish(arguments.component, arguments.tag, assets,
            os.environ["GITHUB_REPOSITORY"], commit, signed_arm=arguments.signed_arm)


if __name__ == "__main__":
    main()
