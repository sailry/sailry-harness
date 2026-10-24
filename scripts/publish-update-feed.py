#!/usr/bin/env python3
"""Attach an authenticated exact-version update catalog to its published release."""

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).parent / "package"))
import versions
import publication as release_api

SIGNING_KEY = "SAILRY_UPDATE_SIGNING_KEY"
TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin")
UPDATER_ASSETS = {name for target in TARGETS for name in (
    f"Sailry-{target}.zip", f"Sailry-{target}.zip.update.json",
)}
FORMAL_ASSETS = UPDATER_ASSETS | {name for target in TARGETS for name in (
    f"Sailry-{target}.dmg", f"SHA256SUMS-desktop-{target}",
)}


@dataclass(frozen=True)
class Publication:
    tag: str
    version: str
    preview: bool


def publication(event, repository):
    release = event.get("release", {})
    tag = release.get("tag_name", "")
    version = tag.removeprefix("v") if tag.startswith("v") else ""
    match = versions.parse(version)
    preview = match.group("preview") is not None
    if event.get("action") != "published" or release.get("draft") is not False:
        raise ValueError("Only a published non-draft release can update feeds")
    if event.get("repository", {}).get("full_name") != repository:
        raise ValueError("Published release belongs to another repository")
    if release.get("prerelease") and not preview:
        raise ValueError("Release channel does not match its semantic version")
    return Publication(tag, version, preview)


class GitHub(release_api.GitHub):
    def api(self, method, endpoint, payload=None, missing=False):
        command = ["gh", "api", "--method", method, endpoint]
        data = None
        if payload is not None:
            command.extend(["--input", "-"])
            data = json.dumps(payload)
        result = subprocess.run(
            command,
            input=data,
            capture_output=True,
            text=True,
            env=child_environment(),
        )
        if result.returncode:
            if missing and "HTTP 404" in result.stderr:
                return None
            raise RuntimeError("GitHub update-catalog API request failed")
        return json.loads(result.stdout) if result.stdout.strip() else None

    def upload(self, repository, tag, path):
        result = subprocess.run(["gh", "release", "upload", tag, str(path), "--repo", repository],
                                capture_output=True, text=True, env=child_environment())
        if result.returncode:
            raise RuntimeError("The signed update catalog could not be uploaded")


def canonical_assets(directory):
    paths = list(directory.iterdir())
    if ({path.name for path in paths} != UPDATER_ASSETS
            or any(not path.is_file() or path.is_symlink() for path in paths)):
        raise ValueError("Update feeds require only the two canonical signed ZIPs and metadata")


def verify_source(arguments, release, client):
    commit = release_api.tagged_commit(release.tag, root=arguments.source)
    if release_api.remote_commit(client, arguments.repository, release.tag) != commit:
        raise ValueError("The published product tag has changed")
    return commit


def formal_assets(inventory, repository, tag):
    selected = {name: [asset for asset in inventory if asset.get("name") == name]
                for name in FORMAL_ASSETS}
    if any(not assets for assets in selected.values()):
        return None
    if any(len(assets) != 1 or assets[0].get("state") != "uploaded"
           or not isinstance(assets[0].get("size"), int) or assets[0]["size"] <= 0
           for assets in selected.values()):
        raise ValueError("The published formal Desktop asset set is invalid")
    for name in FORMAL_ASSETS:
        expected = f"https://github.com/{repository}/releases/download/{tag}/{name}"
        if selected[name][0].get("browser_download_url") != expected:
            raise ValueError("The signed updater asset does not belong to the exact published tag")
    return selected


def download(arguments, client=None):
    client = client or GitHub()
    event = json.loads(arguments.event.read_text())
    release = publication(event, arguments.repository)
    verify_source(arguments, release, client)
    selected = formal_assets(event["release"].get("assets", []), arguments.repository, release.tag)
    if selected is None:
        print("Formal Desktop assets are not complete; the update catalog remains unpublished")
        return False
    arguments.assets.mkdir(mode=0o700, parents=True, exist_ok=False)
    command = ["gh", "release", "download", release.tag, "--repo", arguments.repository,
               "--dir", str(arguments.assets)]
    for name in sorted(UPDATER_ASSETS):
        command.extend(["--pattern", name])
    result = subprocess.run(command, capture_output=True, text=True, env=child_environment())
    if result.returncode:
        raise RuntimeError("The canonical signed updater assets could not be downloaded")
    canonical_assets(arguments.assets)
    for path in arguments.assets.iterdir():
        asset = selected[path.name][0]
        if path.stat().st_size != asset["size"]:
            raise ValueError("The downloaded updater asset size differs from its release")
        if asset.get("digest"):
            with path.open("rb") as stream:
                actual = "sha256:" + hashlib.file_digest(stream, "sha256").hexdigest()
            if actual != asset["digest"]:
                raise ValueError("The downloaded updater asset digest differs from its release")
    return True


def child_environment(signing_key=None):
    environment = os.environ.copy()
    environment.pop(SIGNING_KEY, None)
    if signing_key is not None:
        environment[SIGNING_KEY] = signing_key
    return environment


def merge(
    binary,
    keys,
    assets,
    repository,
    publication,
    channel,
    output,
    signing_key,
):
    result = subprocess.run([
        str(binary), "merge-feed",
        "--assets", str(assets),
        "--channel", channel,
        "--keys", str(keys),
        "--output", str(output),
        "--repository", repository,
        "--tag", publication.tag,
    ], capture_output=True, text=True, env=child_environment(signing_key))
    if result.returncode:
        raise RuntimeError("Authenticated update-feed merge failed")


def publish(arguments, signing_key, client=None):
    client = client or GitHub()
    event = json.loads(arguments.event.read_text())
    release = publication(event, arguments.repository)
    verify_source(arguments, release, client)
    canonical_assets(arguments.assets)
    arguments.stage.mkdir(mode=0o700, parents=True, exist_ok=False)
    output = arguments.stage / "update-v1.json"
    merge(arguments.binary, arguments.keys, arguments.assets, arguments.repository,
          release, "experience" if release.preview else "stable", output, signing_key)
    commit = verify_source(arguments, release, client)
    metadata = release_api.release(client, arguments.repository, release.tag, commit, published=True)
    if formal_assets(metadata.get("assets", []), arguments.repository, release.tag) is None:
        raise ValueError("The published formal Desktop assets are no longer complete")
    if metadata.get("immutable"):
        raise ValueError("The signed catalog requires a mutable published release")
    catalogs = [asset for asset in metadata.get("assets", []) if asset.get("name") == output.name]
    if len(catalogs) > 1:
        raise ValueError("The published signed catalog is ambiguous")
    if catalogs:
        existing = catalogs[0]
        with output.open("rb") as stream:
            digest = "sha256:" + hashlib.file_digest(stream, "sha256").hexdigest()
        expected_url = f"https://github.com/{arguments.repository}/releases/download/{release.tag}/{output.name}"
        if (existing.get("browser_download_url") != expected_url
                or existing.get("state") != "uploaded" or existing.get("size") != output.stat().st_size
                or (existing.get("digest") or client.digest(arguments.repository, existing)) != digest):
            raise ValueError("The published signed catalog is immutable and cannot be replaced")
        print("The exact-version signed catalog is already published")
        return
    client.upload(arguments.repository, release.tag, output)
    print("The exact-version signed catalog is published")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--download", action="store_true")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--assets", type=Path, required=True)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--event", type=Path, required=True)
    parser.add_argument("--keys", type=Path)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--stage", type=Path)
    arguments = parser.parse_args()
    if arguments.download:
        ready = download(arguments)
        with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
            output.write(f"ready={str(ready).lower()}\n")
        return
    if not all((arguments.binary, arguments.keys, arguments.stage)):
        parser.error("Feed publication requires --binary, --keys and --stage")
    signing_key = os.environ.pop(SIGNING_KEY, None)
    if not signing_key:
        raise SystemExit("The update signing secret is not configured")
    publish(arguments, signing_key)


if __name__ == "__main__":
    main()
