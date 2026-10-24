#!/usr/bin/env python3
"""Publish Desktop once, then append verified Host assets to that same release."""

import argparse
import hashlib
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


def digest(path):
    with path.open("rb") as file:
        return "sha256:" + hashlib.file_digest(file, "sha256").hexdigest()


def desktop_available(names):
    return any(filenames(component, signed_arm).issubset(names)
               for component, signed_arm in (("desktop", False), ("desktop-adhoc", False), ("desktop-adhoc", True)))


def verify_assets(component, tag, assets, signed_arm=False):
    versions.parse(tag.removeprefix("v"))
    if not tag.startswith("v") or {path.name for path in assets} != filenames(component, signed_arm):
        raise ValueError(f"The complete {component.title()} asset set for this tag is required")
    if any(not path.is_file() or path.is_symlink() for path in assets):
        raise ValueError("Release assets must be regular files")


def append(client, repository, tag, metadata, assets):
    existing = {asset["name"]: asset for asset in metadata.get("assets", [])}
    missing = []
    for path in assets:
        asset = existing.get(path.name)
        if asset is None:
            missing.append(path)
            continue
        if asset.get("state") != "uploaded" or asset.get("size") != path.stat().st_size:
            raise ValueError(f"Release asset collision: {path.name}; existing assets cannot be replaced")
        actual = asset.get("digest") or client.digest(repository, asset)
        if actual != digest(path):
            raise ValueError(f"Release asset collision: {path.name}; existing assets cannot be replaced")
    if missing:
        client.upload(tag, missing)


def latest_flag(tag):
    # GitHub chooses the highest stable version atomically during publication.
    return "false" if versions.parse(tag[1:]).group("preview") else "legacy"


def publish(component, tag, notes, assets, repository, commit, client=None, signed_arm=False, append_signed=False):
    client = client or publication.GitHub()
    verify_assets(component, tag, assets, signed_arm)
    metadata = publication.release(client, repository, tag, commit, published=component == "host" or append_signed)
    endpoint = f"repos/{repository}/releases"
    if metadata is None:
        if notes is None:
            raise ValueError("Desktop publication requires release notes")
        metadata = client.api("POST", endpoint, {
            "tag_name": tag, "target_commitish": commit, "name": f"Sailry Harness {tag[1:]}",
            "body": notes.read_text(), "draft": True,
            "prerelease": False,
        })
    if metadata.get("immutable"):
        raise ValueError("The release is immutable; Host assets require a mutable published release")
    existing = {asset["name"] for asset in metadata.get("assets", [])
                if asset.get("state") == "uploaded" and type(asset.get("size")) is int and asset["size"] > 0}
    if (component == "host" or append_signed) and not desktop_available(existing):
        raise ValueError("Host assets require a complete published Desktop release")
    if component.startswith("desktop") and not metadata.get("draft"):
        if not append_signed and not filenames(component, signed_arm).issubset(existing):
            raise ValueError("A published Desktop release cannot acquire missing Desktop assets")
    if component.startswith("desktop") and metadata.get("draft"):
        if notes is None:
            raise ValueError("Desktop publication requires release notes")
        allowed = filenames("desktop") | filenames("desktop-adhoc") | filenames("host")
        unexpected = {asset["name"] for asset in metadata.get("assets", [])} - allowed
        if unexpected:
            raise ValueError("Remove unsupported draft assets before publication: " + ", ".join(sorted(unexpected)))
    append(client, repository, tag, metadata, assets)
    if component.startswith("desktop") and metadata.get("draft"):
        client.api("PATCH", f"{endpoint}/{metadata['id']}", {
            "name": f"Sailry Harness {tag[1:]}", "body": notes.read_text(),
            "target_commitish": commit,
            "draft": False,
            "prerelease": False,
            "make_latest": latest_flag(tag),
        })


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", choices=("desktop", "desktop-adhoc", "host"), required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--notes", type=Path)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--signed-arm", action="store_true")
    parser.add_argument("--append-signed", action="store_true")
    arguments = parser.parse_args()
    if arguments.signed_arm and arguments.component != "desktop-adhoc":
        parser.error("Signed ARM preservation only applies to initial Desktop publication")
    if arguments.append_signed and arguments.component != "desktop":
        parser.error("Formal append requires canonical signed Desktop assets")
    commit = publication.tagged_commit(arguments.tag, arguments.source)
    assets = sorted(arguments.directory.iterdir())
    publish(arguments.component, arguments.tag, arguments.notes, assets,
            os.environ["GITHUB_REPOSITORY"], commit, signed_arm=arguments.signed_arm,
            append_signed=arguments.append_signed)


if __name__ == "__main__":
    main()
