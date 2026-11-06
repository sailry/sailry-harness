#!/usr/bin/env python3
"""Start product builds once exact-source Checks compile, then publish the passing version."""

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


# Desktop and Host build once Checks compile the tests; their uploads wait for the published version.
BUILDS = ("release.yml", "release-host.yml")
COMPILED = "Test build"
# TestFlight upload and the public changelog are publications themselves.
FOLLOW_UPS = ("release-mobile.yml", "changelog.yml")


def dispatch(client, repository, tag, workflows):
    failed = []
    for workflow in workflows:
        inputs = {"tag": tag, **({"platform": "both", "upload_testflight": "true"}
                                 if workflow == "release-mobile.yml" else {})}
        try:
            client.api("POST", f"repos/{repository}/actions/workflows/{workflow}/dispatches",
                       {"ref": "main", "inputs": inputs})
        except RuntimeError:
            print(f"::warning::{workflow} could not be dispatched for {tag}", flush=True)
            failed.append(workflow)
    return failed


def queue_checks(client, repository, tag, commit):
    """Find successful full Checks, or queue them once; returns runs that cannot become ready."""
    ready = publication.ready_run(client, repository, commit, "checks", tag=tag)
    if ready is not None:
        return ready, set()
    runs = publication.ci_reuse.pages(
        client, f"repos/{repository}/actions/workflows/ci.yml/runs?head_sha={commit}", "workflow_runs")
    matching = [run for run in runs if publication.checks_run(run, commit, tag)]
    completed = {run["id"] for run in matching if run.get("status") == "completed"}
    if len(completed) == len(matching):
        client.api("POST", f"repos/{repository}/actions/workflows/ci.yml/dispatches", {"ref": tag})
        print(f"Queued full Checks for {tag} at exact source {commit}", flush=True)
    return None, completed


def require_compiled(client, repository, tag, commit, queued):
    """Wait until Checks compile the tests, so release runners never build an uncompilable source."""
    ready, completed = queued
    if ready is None:
        publication.wait_job(client, repository, commit, COMPILED, tag=tag, ignored_runs=completed)


def require_checks(client, repository, tag, commit, queued):
    ready, completed = queued
    if ready is not None:
        return ready
    return publication.wait_ready(client, repository, commit, "checks", tag=tag, ignored_runs=completed)


def report_flaky(client, repository, run_id, environment):
    """Repeat the Checks run's flaky-test annotations where the version entry is reviewed."""
    jobs = publication.ci_reuse.pages(
        client, f"repos/{repository}/actions/runs/{run_id}/jobs?filter=latest", "jobs")
    flaky = []
    for job in jobs:
        annotations = client.api("GET", f"repos/{repository}/check-runs/{job['id']}/annotations?per_page=100")
        flaky += [f"{job['name']}: {item.get('message', '')}" for item in annotations or []
                  if item.get("title") == "Flaky test"]
    for item in flaky:
        print(f"::warning title=Flaky test::{item}", flush=True)
    if flaky and environment.get("GITHUB_STEP_SUMMARY"):
        lines = ["### Flaky tests in release Checks", "", f"Checks run {run_id} passed only after retries:", ""]
        with Path(environment["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as output:
            output.write("\n".join(lines + [f"- {item}" for item in flaky]) + "\n\n")
    return flaky


def start(tag, source, tooling, environment, client=None):
    commit = candidate(tag, source, tooling, environment)
    repository = environment["GITHUB_REPOSITORY"]
    client = client or publication.GitHub()
    metadata = publication.release(client, repository, tag, commit)
    if metadata is not None and metadata.get("immutable"):
        raise ValueError("Product packages require a mutable version release")
    queued = queue_checks(client, repository, tag, commit)
    require_compiled(client, repository, tag, commit, queued)
    failed = dispatch(client, repository, tag, BUILDS)
    checked = require_checks(client, repository, tag, commit, queued)
    metadata = publication.release(client, repository, tag, commit)
    if metadata is not None and metadata.get("immutable"):
        raise ValueError("Product packages require a mutable version release")
    print(f"Verified full Checks for exact source {commit} in run {checked}", flush=True)
    report_flaky(client, repository, checked, environment)
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
    failed += dispatch(client, repository, tag, FOLLOW_UPS)
    if failed:
        raise RuntimeError("The version release is published; workflow dispatch failed: " + ", ".join(failed))
    print(f"Published {tag} at source {commit}; Desktop and Host build since admission; queued Mobile and changelog")
    return metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="Exact tagged product checkout when using separate tooling")
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    start(args.tag, args.source or versions.ROOT, versions.ROOT, os.environ)


if __name__ == "__main__":
    main()
