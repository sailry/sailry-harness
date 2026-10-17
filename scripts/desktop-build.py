#!/usr/bin/env python3
"""Preserve native Desktop inputs before signing and restore their exact producer."""

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
import tarfile
import tempfile
import zipfile

sys.path.insert(0, str(Path(__file__).parent / "package"))
import publication

COMMAND = "cargo build --locked --release --timings -p sailry-desktop -p sailry-release"
TARGETS = {"aarch64-apple-darwin", "x86_64-apple-darwin"}
EXECUTABLES = {"sailry-desktop", "sailry-release"}
FILES = EXECUTABLES


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def toolchain():
    return {"rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
            "sdk": subprocess.check_output(["xcrun", "--sdk", "macosx", "--show-sdk-version"], text=True).strip(),
            "deployment_target": os.environ["MACOSX_DEPLOYMENT_TARGET"]}


def context(root, tag, target):
    if target not in TARGETS:
        raise ValueError("Unsupported native Desktop target")
    commit = publication.tagged_commit(tag, root)
    controller = os.environ["WORKFLOW_REVISION"]
    if re.fullmatch(r"[0-9a-f]{40}", controller) is None:
        raise ValueError("Invalid packaging workflow revision")
    return {"version": 1, "source_revision": commit, "workflow_revision": controller,
            "tag": tag, "target": target, "profile": "release", "command": COMMAND,
            "run_id": int(os.environ["GITHUB_RUN_ID"]), **toolchain()}


def stage(root, tag, target, output):
    provenance = context(root, tag, target)
    if subprocess.run(["git", "diff", "--quiet", "HEAD", "--"], cwd=root).returncode:
        raise ValueError("The native product checkout has changed")
    provenance["run_attempt"] = int(os.environ["GITHUB_RUN_ATTEMPT"])
    with tempfile.TemporaryDirectory(prefix="sailry-native-") as temporary:
        directory = Path(temporary)
        for name in EXECUTABLES:
            path = root / "target/release" / name
            publication.native_binary(path, target)
            shutil.copyfile(path, directory / name)
            (directory / name).chmod(0o755)
        provenance["files"] = {name: {"sha256": digest(directory / name),
                                      "size": (directory / name).stat().st_size}
                               for name in sorted(FILES)}
        (directory / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        output.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(output, "x:gz") as archive:
            for name in sorted(FILES | {"provenance.json"}):
                archive.add(directory / name, arcname=name, recursive=False)
    print(f"Preserved native Desktop source {provenance['source_revision']}, target {target}; no signing credentials included")


def producer(client, repository, expected, attempt):
    endpoint = f"repos/{repository}/actions/runs/{expected['run_id']}"
    run = client.api("GET", endpoint)
    if (run.get("id") != expected["run_id"] or run.get("head_branch") != "main"
            or run.get("head_sha") != expected["workflow_revision"]
            or run.get("event") != "workflow_dispatch" or run.get("path") != ".github/workflows/release.yml"
            or run.get("repository", {}).get("full_name") != repository
            or run.get("head_repository", {}).get("full_name") != repository
            or not isinstance(attempt, int) or attempt < 1 or attempt > int(os.environ["GITHUB_RUN_ATTEMPT"])):
        raise ValueError("The native artifact workflow does not match its controller")
    jobs = client.api("GET", f"{endpoint}/attempts/{attempt}/jobs?per_page=100")["jobs"]
    selected = [job for job in jobs if job.get("name") == f"Build Desktop {expected['target']}"]
    if len(selected) != 1:
        raise ValueError("The native Desktop producer is missing or ambiguous")
    job = selected[0]
    steps = {step["name"]: step for step in job.get("steps", [])}
    if (job.get("conclusion") != "success" or job.get("head_sha") != expected["workflow_revision"]
            or job.get("run_id") != expected["run_id"]
            or any(steps.get(name, {}).get("conclusion") != "success"
                   for name in ("Build native Desktop", "Preserve native Desktop"))):
        raise ValueError("The native Desktop producer did not finish successfully")
    return job


def unpack(path, directory, expected):
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        if (len(members) != len(FILES) + 1 or {item.name for item in members} != FILES | {"provenance.json"}
                or any(not item.isfile() or item.size <= 0 for item in members)):
            raise ValueError("Native Desktop inputs must be exactly the expected regular files")
        manifest = next(item for item in members if item.name == "provenance.json")
        if manifest.size > 16384:
            raise ValueError("Native Desktop provenance is too large")
        provenance = json.loads(archive.extractfile(manifest).read())
        if any(provenance.get(key) != value for key, value in expected.items()):
            raise ValueError("Native Desktop provenance does not match the product, target or toolchain")
        records = provenance.get("files")
        if not isinstance(records, dict) or set(records) != FILES:
            raise ValueError("Native Desktop checksums are incomplete")
        for item in members:
            if item.name == "provenance.json":
                continue
            record = records[item.name]
            if (not isinstance(record, dict) or record.get("size") != item.size
                    or re.fullmatch(r"[0-9a-f]{64}", record.get("sha256", "")) is None):
                raise ValueError("Native Desktop file identity is invalid")
            destination = directory / item.name
            with archive.extractfile(item) as incoming, destination.open("xb") as outgoing:
                shutil.copyfileobj(incoming, outgoing)
            destination.chmod(0o755 if item.name in EXECUTABLES else 0o644)
            if digest(destination) != record["sha256"]:
                raise ValueError("Native Desktop file checksum mismatch")
    return provenance


def restore(root, tag, target):
    expected = context(root, tag, target)
    repository = os.environ["GITHUB_REPOSITORY"]
    client = publication.GitHub()
    if publication.remote_commit(client, repository, tag) != expected["source_revision"]:
        raise ValueError("The remote product tag has changed")
    inventory = client.api("GET", f"repos/{repository}/actions/runs/{expected['run_id']}/artifacts?per_page=100")
    matches = [item for item in inventory["artifacts"] if item.get("name") == f"native-{target}"]
    if inventory.get("total_count", 0) > 100 or len(matches) != 1:
        raise ValueError("The native Desktop artifact is missing or ambiguous")
    artifact = matches[0]
    origin = artifact.get("workflow_run", {})
    if (artifact.get("expired") is not False or not isinstance(artifact.get("id"), int)
            or artifact["id"] <= 0 or artifact.get("size_in_bytes", 0) <= 0
            or re.fullmatch(r"sha256:[0-9a-f]{64}", artifact.get("digest", "")) is None
            or origin.get("id") != expected["run_id"] or origin.get("head_sha") != expected["workflow_revision"]
            or origin.get("head_branch") != "main"):
        raise ValueError("The native Desktop artifact identity is invalid")
    with tempfile.TemporaryFile() as downloaded, tempfile.TemporaryDirectory(prefix="sailry-restore-") as temporary:
        result = subprocess.run(["gh", "api", f"repos/{repository}/actions/artifacts/{artifact['id']}/zip"],
                                stdout=downloaded, stderr=subprocess.PIPE)
        if result.returncode:
            raise RuntimeError("The native Desktop artifact could not be downloaded")
        downloaded.seek(0)
        if "sha256:" + hashlib.file_digest(downloaded, "sha256").hexdigest() != artifact["digest"]:
            raise ValueError("The native Desktop artifact digest does not match its immutable ID")
        downloaded.seek(0)
        directory = Path(temporary)
        with zipfile.ZipFile(downloaded) as archive:
            members = archive.infolist()
            name = f"native-{target}.tar.gz"
            if (len(members) != 1 or members[0].filename != name or members[0].is_dir()
                    or stat.S_IFMT(members[0].external_attr >> 16) not in {0, stat.S_IFREG}):
                raise ValueError("The native Desktop artifact contains unexpected paths")
            path = directory / name
            with archive.open(members[0]) as incoming, path.open("xb") as outgoing:
                shutil.copyfileobj(incoming, outgoing)
        stage = directory / "inputs"
        stage.mkdir()
        provenance = unpack(path, stage, expected)
        job = producer(client, repository, expected, provenance.get("run_attempt"))
        moment = lambda value: datetime.fromisoformat(value.replace("Z", "+00:00"))
        build = next(step for step in job["steps"] if step["name"] == "Build native Desktop")
        if not moment(build["completed_at"]) <= moment(artifact["created_at"]) <= moment(job["completed_at"]):
            raise ValueError("The native Desktop artifact predates its successful build")
        for name in EXECUTABLES:
            publication.native_binary(stage / name, target)
        output = root / "target/release"
        if (root / "target").is_symlink() or output.is_symlink():
            raise ValueError("The native Desktop destination is unsafe")
        output.mkdir(parents=True, exist_ok=True)
        for name in FILES:
            if (output / name).exists() or (output / name).is_symlink():
                raise ValueError("The native Desktop destination already exists")
            shutil.copy2(stage / name, output / name)
    print(f"Restored native artifact {artifact['id']} for product {expected['source_revision']}, controller {expected['workflow_revision']}, target {target}; no Cargo build")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("stage", "restore"))
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--target", choices=sorted(TARGETS), required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.mode == "stage":
        if args.output is None:
            parser.error("Native staging requires an output archive")
        stage(args.source.resolve(), args.tag, args.target, args.output)
    else:
        restore(args.source.resolve(), args.tag, args.target)


if __name__ == "__main__":
    main()
