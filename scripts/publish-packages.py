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


def filenames(component, version):
    if component == "desktop":
        return {name for target in MACOS for name in (
            f"Sailry-{version}-{target}.dmg", f"Sailry-{version}-{target}.zip",
            f"Sailry-{version}-{target}.zip.update.json", f"SHA256SUMS-desktop-{target}")}
    return {"install-host.sh", "SHA256SUMS-installer"} | {
        name for target in (*MACOS, *LINUX) for name in (
            f"sailry-host-{version}-{target}.tar.gz", f"SHA256SUMS-{target}")}


def digest(path):
    with path.open("rb") as file:
        return "sha256:" + hashlib.file_digest(file, "sha256").hexdigest()


def verify_assets(component, tag, assets):
    versions.parse(tag.removeprefix("v"))
    if not tag.startswith("v") or {path.name for path in assets} != filenames(component, tag[1:]):
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


def publish(component, tag, notes, assets, repository, commit, client=None):
    client = client or publication.GitHub()
    verify_assets(component, tag, assets)
    metadata = publication.release(client, repository, tag, commit, published=component == "host")
    endpoint = f"repos/{repository}/releases"
    if metadata is None:
        if notes is None:
            raise ValueError("Desktop publication requires release notes")
        metadata = client.api("POST", endpoint, {
            "tag_name": tag, "target_commitish": commit, "name": f"Sailry Harness {tag[1:]}",
            "body": notes.read_text(), "draft": True,
            "prerelease": versions.parse(tag[1:]).group("preview") is not None,
        })
    if metadata.get("immutable"):
        raise ValueError("The release is immutable; Host assets require a mutable published release")
    if component == "host" and not filenames("desktop", tag[1:]).issubset(
            {asset["name"] for asset in metadata.get("assets", [])}):
        raise ValueError("Host assets require a complete published Desktop release")
    if component == "desktop" and not metadata.get("draft"):
        existing = {asset["name"] for asset in metadata.get("assets", [])}
        if not filenames(component, tag[1:]).issubset(existing):
            raise ValueError("A published Desktop release cannot acquire missing Desktop assets")
    if component == "desktop" and metadata.get("draft"):
        if notes is None:
            raise ValueError("Desktop publication requires release notes")
        allowed = filenames("desktop", tag[1:]) | filenames("host", tag[1:])
        unexpected = {asset["name"] for asset in metadata.get("assets", [])} - allowed
        if unexpected:
            raise ValueError("Remove unsupported draft assets before publication: " + ", ".join(sorted(unexpected)))
    append(client, repository, tag, metadata, assets)
    if component == "desktop" and metadata.get("draft"):
        client.api("PATCH", f"{endpoint}/{metadata['id']}", {
            "name": f"Sailry Harness {tag[1:]}", "body": notes.read_text(),
            "target_commitish": commit,
            "draft": False,
            "make_latest": "false" if versions.parse(tag[1:]).group("preview") else "true",
        })


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", choices=("desktop", "host"), required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--notes", type=Path)
    parser.add_argument("--directory", type=Path, required=True)
    arguments = parser.parse_args()
    commit = publication.tagged_commit(arguments.tag)
    assets = sorted(arguments.directory.iterdir())
    publish(arguments.component, arguments.tag, arguments.notes, assets,
            os.environ["GITHUB_REPOSITORY"], commit)


if __name__ == "__main__":
    main()
