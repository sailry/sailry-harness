#!/usr/bin/env python3
"""Reuse verified Linux Host packages while retrying macOS publication."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).parent / "package"))
import publication

TARGETS = ("aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu")


def recover(root, tag, run_id, directory):
    if re.fullmatch(r"[1-9][0-9]*", run_id) is None:
        raise ValueError("Invalid Host producer run")
    commit = publication.tagged_commit(tag, root)
    repository = os.environ["GITHUB_REPOSITORY"]
    client = publication.GitHub()
    publication.release(client, repository, tag, commit, published=True)
    endpoint = f"repos/{repository}/actions/runs/{run_id}"
    run = client.api("GET", endpoint)
    if (run.get("id") != int(run_id) or run.get("head_branch") != "main"
            or run.get("event") != "workflow_dispatch"
            or run.get("path") != ".github/workflows/release-host.yml"
            or run.get("repository", {}).get("full_name") != repository
            or run.get("head_repository", {}).get("full_name") != repository):
        raise ValueError("Linux packages require an original Host producer")
    jobs = client.api("GET", endpoint + "/jobs?filter=latest&per_page=100")["jobs"]
    for target in TARGETS:
        selected = [job for job in jobs if job.get("name") == f"Linux Host {target}"]
        if len(selected) != 1 or selected[0].get("conclusion") != "success":
            raise ValueError("The original Linux Host builds must have succeeded")
    with tempfile.TemporaryDirectory(prefix="sailry-host-recovery-") as temporary:
        staging = Path(temporary)
        for target in TARGETS:
            subprocess.run(["gh", "run", "download", run_id, "--repo", repository,
                            "--name", f"host-{target}", "--dir", str(staging)], check=True)
            archive = staging / f"sailry-host-{target}.tar.gz"
            checksums = staging / f"SHA256SUMS-{target}"
            if any(not path.is_file() or path.is_symlink() for path in (archive, checksums)):
                raise ValueError("The original Linux package files are missing or unsafe")
            fields = checksums.read_text().split()
            if (len(fields) != 2 or fields[1].removeprefix("./") != archive.name
                    or fields[0] != publication.digest(archive).removeprefix("sha256:")):
                raise ValueError("The original Linux package checksum does not match")
            with tarfile.open(archive, "r:gz") as bundle:
                metadata = [item for item in bundle.getmembers() if item.name in {"build.json", "./build.json"}]
                if len(metadata) != 1 or not metadata[0].isfile() or not 0 < metadata[0].size <= 16384:
                    raise ValueError("The Linux package build provenance is missing or unsafe")
                value = json.load(bundle.extractfile(metadata[0]))
            expected = {"version": 1, "source_revision": commit, "source_dirty": False,
                        "profile": "release", "target": target, "application_version": tag.removeprefix("v")}
            if any(value.get(key) != item for key, item in expected.items()):
                raise ValueError("The Linux package source does not match the release")
        names = {name for target in TARGETS for name in (f"sailry-host-{target}.tar.gz", f"SHA256SUMS-{target}")}
        if {path.name for path in staging.iterdir()} != names:
            raise ValueError("The original Linux artifacts contain unexpected files")
        directory.mkdir(parents=True, exist_ok=True)
        if any((directory / name).exists() or (directory / name).is_symlink() for name in names):
            raise ValueError("Recovered Linux packages would overwrite existing files")
        for name in names:
            shutil.copyfile(staging / name, directory / name)
    print(f"Recovered Linux Host packages from run {run_id}, source {commit}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--run", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    recover(args.source, args.tag, args.run, args.directory)


if __name__ == "__main__":
    main()
