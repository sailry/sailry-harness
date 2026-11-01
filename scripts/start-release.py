#!/usr/bin/env python3
"""Publish an exact version release before starting independent product builds."""

import argparse
from datetime import datetime, timezone
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).parent / "package"))
import publication
import versions

spec = importlib.util.spec_from_file_location("release_notes", Path(__file__).with_name("release-notes.py"))
notes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notes)


def candidate(tag, source, tooling, environment):
    if (environment.get("GITHUB_EVENT_NAME") != "workflow_dispatch"
            or environment.get("GITHUB_REF") != "refs/heads/main"):
        raise ValueError("Dispatch the exact version tag from main")
    event = json.loads(Path(environment["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    if event.get("inputs", {}).get("tag") != tag:
        raise ValueError("The requested release tag must match its dispatch input")
    workflow = environment.get("WORKFLOW_REVISION", "")
    if not re.fullmatch(r"[0-9a-f]{40}", workflow) or workflow != environment.get("GITHUB_SHA"):
        raise ValueError("Release tooling must match the trusted main workflow commit")
    actual = subprocess.check_output(["git", "-C", str(tooling), "rev-parse", "HEAD"], text=True).strip()
    if actual != workflow:
        raise ValueError("The tooling checkout must match the pinned workflow commit")
    commit = publication.tagged_commit(tag, source)
    result = subprocess.run(["git", "-C", str(tooling), "merge-base", "--is-ancestor", commit, workflow],
                            capture_output=True)
    if result.returncode:
        raise ValueError("The release tag must be an ancestor of the trusted main workflow commit")
    return commit


def latest_flag(tag):
    # GitHub chooses the highest stable version atomically during publication.
    return "false" if versions.parse(tag[1:]).group("preview") else "legacy"


def dispatch(client, repository, tag):
    workflows = (
        ("release.yml", {"tag": tag}),
        ("release-host.yml", {"tag": tag}),
        ("release-mobile.yml", {"tag": tag, "platform": "both", "upload_testflight": "true"}),
        ("changelog.yml", {"tag": tag}),
    )
    failed = []
    for workflow, inputs in workflows:
        try:
            client.api("POST", f"repos/{repository}/actions/workflows/{workflow}/dispatches",
                       {"ref": "main", "inputs": inputs})
        except RuntimeError:
            failed.append(workflow)
    if failed:
        raise RuntimeError("The version release is published; workflow dispatch failed: " + ", ".join(failed))


def start(tag, source, tooling, environment, client=None):
    commit = candidate(tag, source, tooling, environment)
    repository = environment["GITHUB_REPOSITORY"]
    client = client or publication.GitHub()
    metadata = publication.release(client, repository, tag, commit)
    if metadata is not None and metadata.get("immutable"):
        raise ValueError("Product packages require a mutable version release")
    if metadata is None or metadata.get("draft"):
        body = notes.generate(source, tag, f"{environment['GITHUB_SERVER_URL']}/{repository}",
                              datetime.now(timezone.utc).date().isoformat(),
                              (source / "CHANGELOG.md").read_text(encoding="utf-8"))
        payload = {"tag_name": tag, "target_commitish": commit, "name": f"Sailry Harness {tag[1:]}",
                   "body": body, "draft": False, "prerelease": False, "make_latest": latest_flag(tag)}
        endpoint = f"repos/{repository}/releases"
        if metadata is None:
            client.api("POST", endpoint, payload)
        else:
            client.api("PATCH", f"{endpoint}/{metadata['id']}", payload)
    metadata = publication.release(client, repository, tag, commit, published=True)
    if metadata.get("immutable"):
        raise ValueError("Product packages require a mutable version release")
    dispatch(client, repository, tag)
    print(f"Published {tag} at source {commit}; queued Desktop, Host, Mobile and changelog workflows")
    return metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="Exact tagged product checkout when using separate tooling")
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    start(args.tag, args.source or versions.ROOT, versions.ROOT, os.environ)


if __name__ == "__main__":
    main()
