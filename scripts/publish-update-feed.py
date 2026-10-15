#!/usr/bin/env python3
"""Verify a published release and atomically advance authenticated update feeds."""

import argparse
import base64
from dataclasses import dataclass
import json
import os
from pathlib import Path
import subprocess
import sys
from urllib.parse import quote

sys.path.insert(0, str(Path(__file__).parent / "package"))
import versions

SIGNING_KEY = "SAILRY_UPDATE_SIGNING_KEY"


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
    if bool(release.get("prerelease")) != preview:
        raise ValueError("Release channel does not match its semantic version")
    return Publication(tag, version, preview)


class GitHub:
    def api(self, method, endpoint, payload=None):
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
            raise RuntimeError("GitHub update-feed API request failed")
        return json.loads(result.stdout)


def read_feeds(client, repository, paths, stage):
    metadata = client.api("GET", f"repos/{repository}")
    branch = metadata["default_branch"]
    ref_path = f"repos/{repository}/git/ref/heads/{quote(branch, safe='')}"
    ref = client.api("GET", ref_path)
    head = ref["object"]["sha"]
    commit = client.api("GET", f"repos/{repository}/git/commits/{head}")
    feeds = {}
    for path in paths:
        endpoint = f"repos/{repository}/contents/{quote(path, safe='/')}?ref={head}"
        content = client.api("GET", endpoint)
        encoded = "".join(content["content"].split())
        data = base64.b64decode(encoded, validate=True)
        output = stage / f"current-{Path(path).name}"
        output.write_bytes(data)
        feeds[path] = output
    return branch, head, commit["tree"]["sha"], feeds


def commit_feeds(client, repository, branch, head, tree, changes, tag):
    entries = []
    for path, contents in sorted(changes.items()):
        blob = client.api("POST", f"repos/{repository}/git/blobs", {
            "content": base64.b64encode(contents).decode("ascii"),
            "encoding": "base64",
        })
        entries.append({"path": path, "mode": "100644", "type": "blob", "sha": blob["sha"]})
    if not entries:
        return False
    updated = client.api("POST", f"repos/{repository}/git/trees", {
        "base_tree": tree,
        "tree": entries,
    })
    commit = client.api("POST", f"repos/{repository}/git/commits", {
        "message": f"release: publish update feeds for {tag}",
        "tree": updated["sha"],
        "parents": [head],
    })
    client.api("PATCH", f"repos/{repository}/git/refs/heads/{quote(branch, safe='')}", {
        "sha": commit["sha"],
        "force": False,
    })
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
    current,
    output,
    signing_key,
):
    result = subprocess.run([
        str(binary), "merge-feed",
        "--assets", str(assets),
        "--channel", channel,
        "--feed", str(current),
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
    channels = {"update-v1-preview.json": "experience"}
    if not release.preview:
        channels["update-v1.json"] = "stable"
    arguments.stage.mkdir(mode=0o700, parents=True, exist_ok=False)
    branch, head, tree, feeds = read_feeds(
        client, arguments.repository, channels, arguments.stage
    )
    changes = {}
    for path, channel in channels.items():
        output = arguments.stage / f"next-{path}"
        merge(
            arguments.binary,
            arguments.keys,
            arguments.assets,
            arguments.repository,
            release,
            channel,
            feeds[path],
            output,
            signing_key,
        )
        contents = output.read_bytes()
        if contents != feeds[path].read_bytes():
            changes[path] = contents
    changed = commit_feeds(
        client, arguments.repository, branch, head, tree, changes, release.tag
    )
    print("Update feeds advanced" if changed else "Update feeds are already current")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--event", type=Path, required=True)
    parser.add_argument("--keys", type=Path, required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--stage", type=Path, required=True)
    arguments = parser.parse_args()
    signing_key = os.environ.pop(SIGNING_KEY, None)
    if not signing_key:
        raise SystemExit("The update signing secret is not configured")
    publish(arguments, signing_key)


if __name__ == "__main__":
    main()
