"""Shared verification for version-tagged GitHub release workflows."""

import datetime
import hashlib
import io
import json
from pathlib import Path
import re
import stat
import subprocess
import tempfile
from urllib.parse import quote
import zipfile

import versions

PREREQUISITES = {
    "desktop": ("Source", "Shared Node", "Desktop"),
    "host": ("Source", "Shared Node", "Host"),
    "mobile": ("Source", "Shared Node", "Mobile, shared bridge and iOS",
               "Mobile Dart FFI contracts", "Mobile Android"),
}


# Product Checks remain valid when only the reviewed signing tool and its
# provenance checks change. Workflow recovery additions must preserve its recipe.
SIGNING_FIX_FILES = frozenset({
    ".github/workflows/release.yml",
    "scripts/package/dmg.py",
    "scripts/package/sign-macos.py",
    "scripts/package/publication.py",
    "scripts/tests/test_macos_package.py",
    "scripts/tests/test_publish_packages.py",
})

# Legacy recovery requires a whole-target cache and an unchanged release recipe.
# Keep these checks for historical tag workflows; main always builds with compiler caching.
REUSE_INPUT = r"""      reuse_build_run:
        description: 'Successful native build run to restore after a signing-only fix'
        type: string
        required: false
        default: ''
"""
REUSE_STEP = r"""      - name: Verify reusable native build
        id: reusable-build
        if: github.event_name == 'workflow_dispatch' && inputs.reuse_build_run != ''
        env:
          GH_TOKEN: ${{ github.token }}
          REUSE_BUILD_RUN: ${{ inputs.reuse_build_run }}
          PACKAGE_TARGET: ${{ matrix.target }}
          CACHE_HIT: ${{ steps.native-cache.outputs.cache-hit }}
          CACHE_KEY: ${{ steps.native-cache.outputs.cache-primary-key }}
          CACHE_MATCHED_KEY: ${{ steps.native-cache.outputs.cache-matched-key }}
        run: |
          python3 - <<'PYTHON'
          import os, sys
          sys.path.insert(0, 'scripts/package')
          import publication
          publication.reuse_build(publication.GitHub(), os.environ['GITHUB_REPOSITORY'],
              os.environ['GITHUB_SHA'], os.environ['GITHUB_REF_NAME'],
              os.environ['REUSE_BUILD_RUN'], os.environ['PACKAGE_TARGET'],
              os.environ['CACHE_HIT'], os.environ['CACHE_KEY'], os.environ['CACHE_MATCHED_KEY'])
          with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
              output.write('verified=true\n')
          PYTHON
"""
REUSE_CONDITION = r"""        if: steps.reusable-build.outputs.verified != 'true'
"""


def checked_source(commit, root=None):
    root = root or versions.ROOT
    def git(*arguments, check=True):
        result = subprocess.run(["git", "-C", str(root), *arguments], capture_output=True)
        if check and result.returncode:
            raise ValueError("The release source could not be verified")
        return result
    if git("rev-parse", "HEAD").stdout.decode().strip() != commit:
        return None
    header = git("cat-file", "-p", commit).stdout.split(b"\n\n", 1)[0]
    parents = [line.split(b" ", 1)[1].decode() for line in header.splitlines() if line.startswith(b"parent ")]
    if len(parents) != 1:
        return None
    parent = parents[0]
    if git("cat-file", "-e", parent + "^{commit}", check=False).returncode:
        git("fetch", "--quiet", "--no-tags", "--depth=1", "origin", parent)
    changes = git("diff", "--raw", "-z", "--no-renames", "--no-ext-diff", "--no-textconv", "--ignore-submodules=none", parent, commit, "--").stdout.split(b"\0")
    if changes == [b""]:
        return None
    workflow_changed = False
    for index in range(0, len(changes) - 1, 2):
        fields = changes[index].split()
        path = changes[index + 1].decode()
        workflow_changed |= path == ".github/workflows/release.yml"
        if (path not in SIGNING_FIX_FILES or fields[-1] != b"M"
                or fields[0] != b":" + fields[1]
                or fields[1] not in {b"100644", b"100755"}):
            return None
    counts = git("diff", "--numstat", "-z", "--no-renames", "--no-ext-diff", "--no-textconv", "--ignore-submodules=none", parent, commit, "--").stdout
    if any(row.split(b"\t", 2)[:2] == [b"-", b"-"] for row in counts.split(b"\0") if row):
        return None
    if workflow_changed:
        try:
            build_recipe(parent, Path(root))
        except ValueError:
            return None
    return parent


class GitHub:
    def build_log(self, repository, run_id, job_name):
        result = subprocess.run(["gh", "api", f"repos/{repository}/actions/runs/{run_id}/logs"],
                                capture_output=True)
        if result.returncode or not result.stdout.startswith(b"PK\x03\x04"):
            raise RuntimeError("The original native build archive could not be downloaded")
        try:
            with zipfile.ZipFile(io.BytesIO(result.stdout)) as archive:
                matches = [entry for entry in archive.infolist()
                           if re.fullmatch(r"[0-9]+_" + re.escape(job_name) + r"\.txt", entry.filename)
                           and not entry.is_dir()]
                if len(matches) != 1:
                    raise ValueError("The original native target log is missing or ambiguous")
                return archive.read(matches[0]).decode("utf-8")
        except (zipfile.BadZipFile, UnicodeDecodeError, RuntimeError):
            raise RuntimeError("The original native target log archive is invalid") from None

    def api(self, method, endpoint, payload=None, missing=False):
        command = ["gh", "api", "--method", method, endpoint]
        if payload is not None:
            command.extend(["--input", "-"])
        result = subprocess.run(command, input=json.dumps(payload) if payload is not None else None,
                                capture_output=True, text=True)
        if result.returncode:
            if missing and "HTTP 404" in result.stderr:
                return None
            raise RuntimeError("GitHub release API request failed")
        return json.loads(result.stdout) if result.stdout.strip() else None

    def upload(self, tag, assets):
        result = subprocess.run(["gh", "release", "upload", tag, *(str(asset) for asset in assets)],
                                capture_output=True, text=True)
        if result.returncode:
            raise RuntimeError("Release asset upload failed; retry with the same workflow artifacts")

    def digest(self, repository, asset):
        with tempfile.TemporaryFile() as output:
            result = subprocess.run(["gh", "api", "-H", "Accept: application/octet-stream",
                                     f"repos/{repository}/releases/assets/{asset['id']}"],
                                    stdout=output, stderr=subprocess.PIPE)
            if result.returncode:
                raise RuntimeError("Existing release asset could not be verified")
            output.seek(0)
            return "sha256:" + hashlib.file_digest(output, "sha256").hexdigest()


def tagged_commit(tag, root=None):
    root = root or versions.ROOT
    version, _ = versions.product(root)
    if tag != f"v{version}":
        raise ValueError("The release tag must match every application version")
    def revision(value):
        return subprocess.check_output(["git", "-C", str(root), "rev-parse", "--verify", value], text=True).strip()
    commit = revision(f"refs/tags/{tag}^{{commit}}")
    if revision("HEAD") != commit:
        raise ValueError("The checkout must be the release tag's exact commit")
    return commit


def remote_commit(client, repository, tag):
    return client.api("GET", f"repos/{repository}/commits/{quote(tag, safe='')}")["sha"]


def release(client, repository, tag, commit, published=False):
    metadata = client.api("GET", f"repos/{repository}/releases/tags/{quote(tag, safe='')}", missing=True)
    page = 1
    while metadata is None:
        releases = client.api("GET", f"repos/{repository}/releases?per_page=100&page={page}")
        metadata = next((entry for entry in releases if entry.get("tag_name") == tag), None)
        if metadata is not None or len(releases) < 100:
            break
        page += 1
    if remote_commit(client, repository, tag) != commit:
        raise ValueError("The remote release tag differs from the verified commit")
    if metadata is None:
        if published:
            raise ValueError("Desktop must publish this release before Host or publication follow-ups run")
        return None
    preview = versions.parse(tag[1:]).group("preview") is not None
    if metadata.get("tag_name") != tag or bool(metadata.get("prerelease")) != preview:
        raise ValueError("The release tag or channel does not match the application version")
    if published and (metadata.get("draft") is not False or not metadata.get("published_at")):
        raise ValueError("A published Desktop release is required")
    return metadata


def ready_run(client, repository, commit, component):
    gate = f"{component.title()} ready"
    required = {*PREREQUISITES[component], gate}
    page = 1
    while True:
        runs = client.api("GET", f"repos/{repository}/actions/workflows/ci.yml/runs?head_sha={commit}&per_page=100&page={page}")["workflow_runs"]
        for run in runs:
            if (run.get("head_sha") != commit or run.get("head_branch") != "main"
                    or run.get("event") not in {"push", "workflow_dispatch"}):
                continue
            jobs = client.api("GET", f"repos/{repository}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")["jobs"]
            successful = {job.get("name") for job in jobs if job.get("status") == "completed"
                          and job.get("conclusion") == "success"}
            if required.issubset(successful):
                return run["id"]
        if len(runs) < 100:
            break
        page += 1
    return None


def readiness(client, repository, commit, component):
    run = ready_run(client, repository, commit, component)
    if run is not None:
        return run
    source = checked_source(commit)
    if source is not None:
        run = ready_run(client, repository, source, component)
        if run is not None:
            print(f"Reused product Checks for unchanged source {source}; only signing tools differ")
            return run
    raise ValueError(f"The release product needs a successful {component.title()} ready gate from full main Checks")


def build_recipe(source, root):
    workflow = ".github/workflows/release.yml"
    current = (root / workflow).read_text()
    for addition in (REUSE_INPUT, REUSE_STEP, REUSE_CONDITION):
        if current.count(addition) != 1:
            raise ValueError("The native build recovery recipe differs from its reviewed form")
        current = current.replace(addition, "", 1)
    original = subprocess.run(["git", "-C", str(root), "show", f"{source}:{workflow}"],
                              check=True, capture_output=True, text=True).stdout
    if current != original:
        raise ValueError("The original native build environment or command has changed")


def native_binary(path, target):
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111:
        raise ValueError("The restored native executable is missing or unsafe")
    with path.open("rb") as executable:
        header = executable.read(8)
    architecture = {"aarch64-apple-darwin": 0x100000C, "x86_64-apple-darwin": 0x1000007}
    if (header[:4] != b"\xcf\xfa\xed\xfe" or len(header) != 8
            or int.from_bytes(header[4:], "little") != architecture.get(target)):
        raise ValueError("The restored native executable has a different architecture")


def reuse_build(client, repository, commit, tag, run_id, target, hit, key, matched, root=None):
    root = Path(root or versions.ROOT)
    if (not str(run_id).isdigit() or hit != "true" or key != matched
            or not re.fullmatch(r"packages-" + re.escape(target) + r"-[0-9a-f]{64}", key)
            or target not in {"aarch64-apple-darwin", "x86_64-apple-darwin"}):
        raise ValueError("Native build recovery requires the exact restored cache")
    source = checked_source(commit, root)
    if source is None or ready_run(client, repository, source, "desktop") is None:
        raise ValueError("Native build recovery requires unchanged, checked product source")
    build_recipe(source, root)
    endpoint = f"repos/{repository}/actions"
    run = client.api("GET", f"{endpoint}/runs/{run_id}")
    if (run.get("head_sha") != source or run.get("head_branch") != tag
            or run.get("event") not in {"push", "workflow_dispatch"}
            or run.get("path") != ".github/workflows/release.yml"
            or run.get("run_attempt") != 1 or run.get("status") != "completed"):
        raise ValueError("The selected native build does not belong to the checked release source")
    jobs = client.api("GET", f"{endpoint}/runs/{run_id}/jobs?filter=latest&per_page=100")["jobs"]
    selected = [job for job in jobs if job.get("name") == f"Desktop {target}"]
    if len(selected) != 1:
        raise ValueError("The original native target is missing or ambiguous")
    job = selected[0]
    steps = {step["name"]: step for step in job["steps"]}
    build = steps.get("Build native Desktop", {})
    save = steps.get("Save compilation before signing", {})
    package = steps.get("Sign, notarize and package", {})
    if any(step.get("conclusion") != "success" for step in (build, save)):
        raise ValueError("The original native build and cache save must both have succeeded")
    caches = client.api("GET", f"{endpoint}/caches?key={quote(key, safe='')}&per_page=100")
    entries = [cache for cache in caches["actions_caches"] if cache.get("key") == key]
    if caches.get("total_count", 0) >= 100 or len(entries) != 1:
        raise ValueError("The original native cache identity is ambiguous")
    cache = entries[0]
    def timestamp(value):
        return datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if (cache.get("ref") not in {f"refs/tags/{tag}", f"refs/heads/refs/tags/{tag}"}
            or not isinstance(cache.get("id"), int) or cache["id"] <= 0
            or not re.fullmatch(r"[0-9a-f]{64}", cache.get("version", ""))
            or cache.get("size_in_bytes", 0) <= 0
            or not build.get("completed_at") or not save.get("started_at")
            or not package.get("started_at")
            or not timestamp(build["completed_at"]) <= timestamp(save["started_at"])
            <= timestamp(cache["created_at"]) <= timestamp(package["started_at"])):
        raise ValueError("The original native cache provenance does not match the successful build")
    log = re.sub(r"\x1b\[[0-9;]*m", "", client.build_log(repository, run_id, job["name"]))
    command = "cargo build --locked --release --timings -p sailry-desktop -p sailry-release"
    size = cache["size_in_bytes"]
    if (command not in log or "Finished" not in log or "`release` profile [optimized]" not in log
            or f"Cache saved with key: {key}" not in log
            or f"Sent {size} of {size} (100.0%)" not in log):
        raise ValueError("The original native build log does not confirm this cache")
    for name in ("sailry-desktop", "sailry-release"):
        native_binary(root / "target/release" / name, target)
    print(f"Reused native build run {run_id}, source {source}, cache {cache['id']}, target {target}; signing and notarization remain required")
    return cache["id"]


def event(metadata, repository):
    return {"action": "published", "repository": {"full_name": repository}, "release": metadata}
