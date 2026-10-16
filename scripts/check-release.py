#!/usr/bin/env python3
"""Admit exact-tag product builds and verify published-release follow-ups."""

import argparse
import json
import os
from pathlib import Path
import sys

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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="Exact candidate checkout when using separate workflow tooling")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--component", choices=("desktop", "host", "mobile"))
    parser.add_argument("--published", action="store_true")
    parser.add_argument("--event", type=Path)
    args = parser.parse_args()
    require_context(args.tag)
    repository = os.environ["GITHUB_REPOSITORY"]
    client = publication.GitHub()
    commit = publication.tagged_commit(args.tag, root=args.source)
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
