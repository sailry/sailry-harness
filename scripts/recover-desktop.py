#!/usr/bin/env python3
"""Reuse one signed ARM package while recovering Intel for the exact same tag."""

import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import zipfile

sys.path.insert(0, str(Path(__file__).parent / "package"))
import publication

TARGET = "aarch64-apple-darwin"


def timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def source(client, repository, tag, commit, run_id):
    if re.fullmatch(r"[1-9][0-9]*", str(run_id)) is None:
        raise ValueError("Recovery requires a positive workflow run ID")
    endpoint = f"repos/{repository}/actions"
    run = client.api("GET", f"{endpoint}/runs/{run_id}")
    if (run.get("id") != int(run_id) or run.get("head_sha") != commit or run.get("head_branch") != "main"
            or run.get("event") != "workflow_dispatch"
            or run.get("path") != ".github/workflows/release.yml"
            or run.get("run_attempt") != 1 or run.get("status") != "completed"
            or run.get("conclusion") not in {"success", "failure"}
            or run.get("repository", {}).get("full_name") != repository
            or run.get("head_repository", {}).get("full_name") != repository):
        raise ValueError("Recovery requires the original exact-source Desktop workflow")
    jobs = client.api("GET", f"{endpoint}/runs/{run_id}/jobs?filter=latest&per_page=100")["jobs"]
    selected = [job for job in jobs if job.get("name") == f"Desktop {TARGET}"]
    if len(selected) != 1:
        raise ValueError("The original ARM producer is missing or ambiguous")
    job = selected[0]
    steps = {step["name"]: step for step in job.get("steps", [])}
    required = ("Build native Desktop", "Sign, notarize and package", "Remove temporary signing credentials")
    if (job.get("run_id") != int(run_id) or job.get("head_sha") != commit
            or job.get("status") != "completed" or job.get("conclusion") != "success"
            or any(steps.get(name, {}).get("conclusion") != "success" for name in required)):
        raise ValueError("The original ARM producer did not complete signing and packaging")
    log = client.build_log(repository, run_id, job["name"])
    log = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", log)
    lines = re.sub(r"(?m)^\d{4}-\d\d-\d\dT[0-9:.]+Z ", "", log).splitlines()
    checkout = next((line for line in lines if "git checkout --progress --force " in line), "")
    revision = next((index for index, line in enumerate(lines) if "git log -1 --format=%H" in line), -1)
    if (not checkout.endswith(f" refs/tags/{tag}") or revision < 0
            or revision + 1 >= len(lines) or lines[revision + 1].strip() != commit):
        raise ValueError("The original ARM checkout does not match the exact product tag")
    artifacts = client.api("GET", f"{endpoint}/runs/{run_id}/artifacts?per_page=100")
    if artifacts.get("total_count", 0) > 100:
        raise ValueError("The original artifact inventory is ambiguous")
    selected = [item for item in artifacts["artifacts"] if item.get("name") == f"packages-{TARGET}"]
    if len(selected) != 1:
        raise ValueError("The signed ARM artifact is missing or ambiguous")
    artifact = selected[0]
    origin = artifact.get("workflow_run", {})
    if (not isinstance(artifact.get("id"), int) or artifact["id"] <= 0
            or artifact.get("expired") is not False or artifact.get("size_in_bytes", 0) <= 0
            or re.fullmatch(r"sha256:[0-9a-f]{64}", artifact.get("digest", "")) is None
            or origin.get("id") != int(run_id) or origin.get("head_sha") != commit
            or origin.get("head_branch") != "main"
            or origin.get("repository_id") != run["repository"]["id"]
            or origin.get("head_repository_id") != run["repository"]["id"]
            or not timestamp(steps["Sign, notarize and package"]["completed_at"])
            <= timestamp(artifact["created_at"]) <= timestamp(job["completed_at"])):
        raise ValueError("The signed ARM artifact provenance is invalid")
    print(f"Verified signed ARM artifact {artifact['id']} ({artifact['digest']}) from run {run_id}, product {commit}")
    return artifact


def filenames(tag):
    archive = f"Sailry-{tag[1:]}-{TARGET}.zip"
    return {archive, archive + ".update.json", archive.removesuffix(".zip") + ".dmg",
            f"SHA256SUMS-desktop-{TARGET}"}


def unpack(archive, directory, tag):
    expected = filenames(tag)
    with zipfile.ZipFile(archive) as zipped:
        entries = zipped.infolist()
        if (len(entries) != 4 or {entry.filename for entry in entries} != expected
                or any(entry.is_dir() or entry.file_size <= 0 or
                       stat.S_IFMT(entry.external_attr >> 16) not in {0, stat.S_IFREG} for entry in entries)):
            raise ValueError("The signed ARM artifact must contain exactly four regular release files")
        for entry in entries:
            with zipped.open(entry) as incoming, (directory / entry.filename).open("xb") as output:
                shutil.copyfileobj(incoming, output)
    checksum = directory / f"SHA256SUMS-desktop-{TARGET}"
    rows = checksum.read_text().splitlines()
    expected.remove(checksum.name)
    actual = {}
    for row in rows:
        match = re.fullmatch(r"([0-9a-f]{64})  (\S+)", row)
        if match is None or match[2].removeprefix("./") in actual:
            raise ValueError("The signed ARM checksums are invalid")
        actual[match[2].removeprefix("./")] = match[1]
    if set(actual) != expected:
        raise ValueError("The signed ARM checksums do not describe all three assets")
    for name, digest in actual.items():
        with (directory / name).open("rb") as file:
            if hashlib.file_digest(file, "sha256").hexdigest() != digest:
                raise ValueError("A signed ARM asset differs from its checksum")


def restore(artifact, repository, tag, directory, binary, root):
    directory.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryFile() as downloaded:
        result = subprocess.run(["gh", "api", f"repos/{repository}/actions/artifacts/{artifact['id']}/zip"],
                                stdout=downloaded, stderr=subprocess.PIPE)
        if result.returncode:
            raise RuntimeError("The signed ARM artifact could not be downloaded")
        downloaded.seek(0)
        digest = "sha256:" + hashlib.file_digest(downloaded, "sha256").hexdigest()
        if digest != artifact["digest"]:
            raise ValueError("The downloaded ARM artifact digest does not match its immutable ID")
        downloaded.seek(0)
        unpack(downloaded, directory, tag)
    archive = directory / f"Sailry-{tag[1:]}-{TARGET}.zip"
    with tempfile.TemporaryDirectory(prefix="sailry-arm-metadata-") as temporary:
        described = Path(temporary) / "verified.json"
        subprocess.run([str(binary), "describe", "--archive", str(archive), "--version", tag[1:],
                        "--target", TARGET, "--minimum-system", "13.0", "--repository", repository,
                        "--tag", tag, "--keys", str(root / "release/update-public-keys.txt"),
                        "--output", str(described)], check=True)
        if described.read_bytes() != Path(str(archive) + ".update.json").read_bytes():
            raise ValueError("The signed ARM metadata differs from its authenticated archive")
    print("Verified signed ARM archive, version, target, metadata and checksums; original bytes preserved")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--run", required=True)
    parser.add_argument("--directory", type=Path)
    parser.add_argument("--binary", type=Path)
    arguments = parser.parse_args()
    if bool(arguments.directory) != bool(arguments.binary):
        parser.error("Recovery requires both an output directory and the compiled release tool")
    repository = os.environ["GITHUB_REPOSITORY"]
    client = publication.GitHub()
    commit = publication.tagged_commit(arguments.tag, root=arguments.source)
    if publication.remote_commit(client, repository, arguments.tag) != commit:
        raise ValueError("The remote product tag has changed")
    artifact = source(client, repository, arguments.tag, commit, arguments.run)
    if arguments.directory:
        restore(artifact, repository, arguments.tag, arguments.directory, arguments.binary.resolve(), arguments.source)


if __name__ == "__main__":
    main()
