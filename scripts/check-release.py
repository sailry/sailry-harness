#!/usr/bin/env python3
"""Admit exact-tag product builds and verify published-release follow-ups."""

import argparse
import json
import os
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).parent / "package"))
import publication


def require_context(tag):
    if os.environ.get("GITHUB_REF_TYPE") == "tag" and os.environ.get("GITHUB_REF_NAME") == tag:
        return
    if (os.environ.get("GITHUB_REF") == "refs/heads/main"
            and os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch"):
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
        if event.get("inputs", {}).get("tag") == tag:
            return
    raise ValueError("Use the matching version tag or dispatch its exact tag from main")


def wait_ready(client, repository, commit, component):
    deadline = time.monotonic() + 5 * 60 * 60
    while True:
        ready = publication.ready_run(client, repository, commit, component)
        if ready is not None:
            checked = client.api("GET", f"repos/{repository}/actions/runs/{ready}")
            if (checked.get("id") != ready or checked.get("head_sha") != commit
                    or checked.get("head_branch") != "main"
                    or checked.get("event") not in {"push", "workflow_dispatch"}):
                raise ValueError("The ready Checks run does not match the release source")
            if checked.get("status") == "completed" and checked.get("conclusion") == "success":
                return ready
        payload = client.api("GET", f"repos/{repository}/actions/workflows/ci.yml/runs?head_sha={commit}&per_page=100")
        runs = [run for run in payload["workflow_runs"]
                if run.get("head_sha") == commit and run.get("head_branch") == "main"
                and run.get("event") in {"push", "workflow_dispatch"}]
        required = {*publication.PREREQUISITES[component], f"{component.title()} ready", "Rust strict lint"}
        pending = False
        for run in runs:
            if run.get("status") == "completed":
                continue
            jobs = client.api("GET", f"repos/{repository}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")["jobs"]
            blocked = any(job.get("name") in required and job.get("status") == "completed"
                          and job.get("conclusion") in {"failure", "cancelled", "timed_out", "action_required"}
                          for job in jobs)
            pending |= not blocked
        if runs and not pending:
            raise ValueError(f"The exact-source Checks ended without a successful full run and {component.title()} readiness")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("The exact-source release gate did not become ready within five hours")
        print(f"Waiting for successful full Checks and {component.title()} readiness at exact source {commit}", flush=True)
        time.sleep(min(60, remaining))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="Exact candidate checkout when using separate workflow tooling")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--component", choices=("desktop", "host", "mobile"))
    parser.add_argument("--published", action="store_true")
    parser.add_argument("--wait-ready", action="store_true")
    parser.add_argument("--event", type=Path)
    args = parser.parse_args()
    require_context(args.tag)
    repository = os.environ["GITHUB_REPOSITORY"]
    client = publication.GitHub()
    commit = publication.tagged_commit(args.tag, root=args.source)
    if args.wait_ready:
        if args.component != "desktop" or args.published:
            parser.error("Waiting is only supported before initial Desktop publication")
        wait_ready(client, repository, commit, args.component)
    if args.component:
        run = publication.readiness(client, repository, commit, args.component)
        print(f"Verified {args.component.title()} ready in Checks run {run}")
    metadata = publication.release(client, repository, args.tag, commit, published=args.published)
    if args.component == "host" and metadata.get("immutable"):
        raise ValueError("Host assets cannot be appended to an immutable release")
    if args.event:
        if not args.published:
            raise ValueError("A publication event requires a published release")
        args.event.write_text(json.dumps(publication.event(metadata, repository)) + "\n")


if __name__ == "__main__":
    main()
