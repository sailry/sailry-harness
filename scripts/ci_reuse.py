#!/usr/bin/env python3
"""Reuse successful Checks jobs only with verified, unchanged product inputs."""

import ast
import copy
from datetime import datetime, timezone
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import zipfile


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ".github/workflows/ci.yml"
RELEASE_WORKFLOW = ".github/workflows/release.yml"
JOB_NAMES = {
    "lint": "Rust strict lint",
    "build": "Test build",
    "desktop": "Desktop",
    "windows": "Desktop Windows",
    "host": "Host",
    "mobile": "Mobile",
    "android": "Mobile Android",
    "services": "Pairing service",
}
SHARDS = {1: "Shared Node (1/2)", 2: "Shared Node (2/2)"}
NAMES = frozenset((*JOB_NAMES.values(), *SHARDS.values()))
NEW_FILES = frozenset({"scripts/ci_reuse.py", "scripts/tests/test_ci_reuse.py"})
REPAIR_FILES = NEW_FILES | {
    "CHANGELOG.md",
    "scripts/tests/test_ui_tasks.py", "scripts/tests/test_ci_resources.py",
    "scripts/package/publication.py", "scripts/tests/test_publish_packages.py", "scripts/tests/test_release_notes.py",
    "scripts/ci.py", WORKFLOW, RELEASE_WORKFLOW,
}
PROOF_LIMIT = 64 * 1024
LOG_LIMIT = 4 * 1024 * 1024


def checks_module():
    spec = importlib.util.spec_from_file_location("_sailry_reuse_checks", ROOT / "scripts/ci.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class GitHub:
    def api(self, method, endpoint, payload=None, missing=False):
        command = ["gh", "api", "--method", method, endpoint]
        if payload is not None:
            command.extend(["--input", "-"])
        result = subprocess.run(command, input=json.dumps(payload) if payload is not None else None,
                                capture_output=True, text=True)
        if result.returncode:
            if missing and "HTTP 404" in result.stderr:
                return None
            raise ValueError("GitHub Checks API request failed")
        return json.loads(result.stdout) if result.stdout.strip() else None


def download(endpoint, limit):
    command = ["gh", "api"]
    usage = subprocess.run([*command, "--help"], capture_output=True, text=True)
    if usage.returncode:
        raise ValueError("GitHub CLI API options are unavailable")
    if "--allow-escape-sequences" in usage.stdout:
        command.append("--allow-escape-sequences")
    command.append(endpoint)
    with tempfile.TemporaryFile() as errors:
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=errors) as process:
            content = process.stdout.read(limit + 1)
            if len(content) > limit:
                process.kill()
                raise ValueError("Checks evidence exceeds its size limit")
            exit_code = process.wait()
            if exit_code:
                errors.seek(0)
                detail = errors.read(8192).decode("utf-8", errors="replace")
                for name in ("GH_TOKEN", "GITHUB_TOKEN"):
                    credential = os.getenv(name)
                    if credential:
                        detail = detail.replace(credential, "[redacted]")
                detail = re.sub(r"https?://\S+", "[URL]", detail)
                reason = " ".join(detail.split())[:512] or f"exit code {exit_code}"
                raise ValueError(f"Checks evidence could not be downloaded ({reason})")
    return content


def identifier(value):
    if isinstance(value, bool) or not re.fullmatch(r"[1-9][0-9]*", str(value)):
        raise ValueError("Invalid Checks identifier")
    return int(value)


def sha(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", value):
        raise ValueError("Invalid Checks source revision")
    return value


def repository_name(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", value):
        raise ValueError("Invalid Checks repository")
    return value


def timestamp(value):
    try:
        result = datetime.fromisoformat(value.replace("Z", "+00:00"))
        if result.tzinfo is None:
            raise ValueError
        return result
    except (AttributeError, TypeError, ValueError):
        raise ValueError("Invalid Checks evidence timestamp") from None


def pages(client, endpoint, key):
    entries = []
    page = 1
    while True:
        separator = "&" if "?" in endpoint else "?"
        payload = client.api("GET", f"{endpoint}{separator}per_page=100&page={page}")
        batch = payload.get(key) if isinstance(payload, dict) else None
        if not isinstance(batch, list) or any(not isinstance(entry, dict) for entry in batch):
            raise ValueError("Invalid Checks evidence page")
        entries.extend(batch)
        if len(batch) < 100:
            return entries
        page += 1


def run_metadata(client, repository, run_id, commit=None, donor=False):
    run = client.api("GET", f"repos/{repository}/actions/runs/{run_id}")
    if (not isinstance(run, dict) or run.get("id") != run_id
            or not isinstance(run.get("repository"), dict) or not isinstance(run.get("head_repository"), dict)
            or run["repository"].get("full_name") != repository
            or run["head_repository"].get("full_name") != repository
            or run.get("path") != WORKFLOW or run.get("head_branch") != "main"
            or run.get("event") not in ("push", "workflow_dispatch")
            or run.get("conclusion") == "cancelled"
            or (commit is not None and run.get("head_sha") != commit)
            or (donor and (run.get("status") != "completed"
                           or run.get("conclusion") not in ("success", "failure")))):
        raise ValueError("Checks run does not match the required main workflow")
    sha(run.get("head_sha"))
    identifier(run.get("run_attempt"))
    return run


def jobs_for(client, repository, run):
    jobs = pages(client, f"repos/{repository}/actions/runs/{run['id']}/jobs?filter=latest", "jobs")
    named = {}
    for job in jobs:
        name = job.get("name")
        if not isinstance(name, str):
            raise ValueError("Invalid Checks job name")
        if name not in NAMES | {"Source"}:
            continue
        if (name in named or job.get("run_id") != run["id"]
                or job.get("head_sha") != run["head_sha"]
                or job.get("run_attempt") != run["run_attempt"]):
            raise ValueError("Checks jobs do not match their original run")
        identifier(job.get("id"))
        named[name] = job
    return named


def succeeded(job):
    return job.get("status") == "completed" and job.get("conclusion") == "success"


def artifact_metadata(artifact, run, name):
    binding = artifact.get("workflow_run", {})
    if (artifact.get("name") != name or artifact.get("expired") is not False or not isinstance(binding, dict)
            or binding.get("id") != run["id"] or binding.get("head_sha") != run["head_sha"]
            or binding.get("head_branch") != "main"
            or type(artifact.get("size_in_bytes")) is not int or artifact["size_in_bytes"] <= 0
            or not isinstance(artifact.get("digest"), str)
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", artifact["digest"])
            or timestamp(artifact.get("expires_at")) <= datetime.now(timezone.utc)):
        raise ValueError(f"Invalid or expired {name} artifact")
    identifier(artifact.get("id"))
    timestamp(artifact.get("created_at"))
    return {key: artifact[key] for key in (
        "id", "name", "digest", "size_in_bytes", "created_at", "expires_at",
    )}


def find_artifact(client, repository, run, name, optional=False):
    artifacts = pages(client, f"repos/{repository}/actions/runs/{run['id']}/artifacts", "artifacts")
    matches = [artifact for artifact in artifacts if artifact.get("name") == name]
    if not matches and optional:
        return None
    if len(matches) != 1:
        raise ValueError(f"Missing or ambiguous {name} artifact")
    return artifact_metadata(matches[0], run, name)


def source_selection(repository, job):
    if not succeeded(job):
        raise ValueError("The donor Source check did not pass")
    content = download(f"repos/{repository}/actions/jobs/{job['id']}/logs", LOG_LIMIT).decode("utf-8")
    prefix = r"^(?:\d{4}-\d\d-\d\dT[0-9:.]+Z )?"
    modes = re.findall(prefix + r"Check mode: ([a-z]+)\r?$", content, re.MULTILINE)
    selections = re.findall(prefix + r"Selected checks: ([a-z_, ]*)\r?$", content, re.MULTILINE)
    if (modes != ["full"] or len(selections) != 1
            or set(selections[0].split(", ")) != checks_module().LANES):
        raise ValueError("The donor must have selected every product input")


def git(root, *arguments):
    result = subprocess.run(["git", "--no-pager", "-C", str(root), *arguments], capture_output=True)
    if result.returncode:
        raise ValueError("Checks input history is unavailable or not ancestral; fetch the full checkout")
    return result.stdout


def script_inputs(source):
    tree = ast.parse(source)
    assignments = [node for node in tree.body if isinstance(node, ast.Assign)
                   and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name)
                   and node.targets[0].id == "FIXTURES"]
    if len(assignments) != 1:
        raise ValueError("CI fixture registry is missing or ambiguous")
    ast.literal_eval(assignments[0].value)
    tree.body.remove(assignments[0])
    return ast.dump(tree, include_attributes=False)


def yaml_lines(source):
    lines = source.splitlines()
    result = []
    index = 0
    while index < len(lines):
        line = lines[index]
        index += 1
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if "\t" in line[:len(line) - len(line.lstrip())]:
            raise ValueError("Unsupported workflow indentation")
        indent = len(line) - len(line.lstrip(" "))
        block = []
        if re.search(r":\s*[|>][-+]?\s*(?:#.*)?$", line):
            while index < len(lines):
                child = lines[index]
                if child.strip() and len(child) - len(child.lstrip(" ")) <= indent:
                    break
                block.append(child)
                index += 1
        result.append((indent, line[indent:], tuple(block)))
    return result


def fields(lines, indent):
    result = {}
    key = None
    for line in lines:
        if line[0] == indent:
            match = re.match(r"([a-zA-Z_][\w-]*):(?:\s|$)", line[1])
            if not match or match[1] in result:
                raise ValueError("Unsupported or duplicate workflow field")
            key = match[1]
            result[key] = []
        elif line[0] < indent or key is None:
            raise ValueError("Unsupported workflow structure")
        result[key].append(line)
    return result


def archive_step(lines):
    properties = fields([(8, lines[0][1].removeprefix("- "), lines[0][2]), *lines[1:]], 8)
    action = properties.get("uses", [])
    inputs = properties.get("with", [])
    if (not action or not action[0][1].startswith("uses: actions/download-artifact@")
            or not inputs):
        return lines
    settings = fields(inputs[1:], 10)
    if settings.get("name") != [(10, "name: nextest-archive", ())]:
        return lines
    for key, accepted in {
        "github-token": {"github-token: ${{ github.token }}"},
        "run-id": {
            "run-id: ${{ needs.source.outputs.archive_run || github.run_id }}",
            "run-id: ${{ needs.source.outputs.reuse_archive_run || github.run_id }}",
        },
    }.items():
        if key in settings:
            if len(settings[key]) != 1 or settings[key][0][1] not in accepted or settings[key][0][2]:
                raise ValueError("Unverified test archive location")
            del settings[key]
    properties["with"] = [inputs[0], *(line for group in settings.values() for line in group)]
    result = [line for group in properties.values() for line in group]
    result[0] = (6, "- " + result[0][1], result[0][2])
    return result


def dependency_source_steps(groups):
    restore = [
        [(6, "- name: Rust dependency sources", ()),
         (8, "uses: ./.github/actions/rust-cache", ())],
        [(6, "- name: Restore locked source checkouts", ()),
         (8, "run: cargo fetch --locked --target aarch64-apple-darwin", ())],
    ]
    for index in range(len(groups) - 1):
        if groups[index:index + 2] == restore:
            return groups[:index] + groups[index + 2:]
    return groups


def job_inputs(name, lines, normalize_sources=True):
    properties = fields(lines, 4)
    for key in ("if", "needs"):
        properties.pop(key, None)
    if name == "shared":
        strategy = properties.get("strategy", [])
        if not strategy:
            raise ValueError("Missing Shared partition strategy")
        settings = fields(strategy[1:], 6)
        matrix = settings.get("matrix")
        allowed = [
            [(6, "matrix:", ()), (8, "partition: [1, 2]", ())],
            [(6, "matrix: ${{ fromJSON(needs.source.outputs.shared_matrix) }}", ())],
            [(6, "matrix: ${{ fromJSON(needs.source.outputs.reuse_shared_matrix) }}", ())],
        ]
        if matrix not in allowed:
            raise ValueError("Unverified Shared partition matrix")
        settings["matrix"] = allowed[0]
        properties["strategy"] = [strategy[0], *(line for group in settings.values() for line in group)]
    if name in {"shared", "desktop"}:
        steps = properties.get("steps", [])
        groups = []
        for line in steps[1:]:
            if line[0] == 6 and line[1].startswith("- "):
                groups.append([])
            if not groups:
                raise ValueError("Unsupported workflow step")
            groups[-1].append(line)
        if name == "desktop" and normalize_sources:
            groups = dependency_source_steps(groups)
        properties["steps"] = [*steps[:1], *(line for group in groups for line in archive_step(group))]
    return properties


def workflow_inputs(source, normalize_sources=True):
    top = fields(yaml_lines(source), 0)
    top.pop("on", None)
    jobs = top.pop("jobs", None)
    if not jobs or jobs[0][1] != "jobs:":
        raise ValueError("Missing workflow jobs")
    declared = fields(jobs[1:], 2)
    expected = {*JOB_NAMES, "shared", "source", "desktop_ready", "host_ready", "mobile_ready",
                "checks", "light_linux", "light_macos"}
    if set(declared) != expected:
        raise ValueError("Unexpected Checks job set")
    runtime = {name: job_inputs(name, declared[name][1:], normalize_sources) for name in (*JOB_NAMES, "shared")}
    runtime.update({name: declared[name] for name in ("light_linux", "light_macos")})
    return top, runtime


def release_inputs(source):
    top = fields(yaml_lines(source), 0)
    jobs = top.pop("jobs", [])
    if jobs[:1] != [(0, "jobs:", ())]:
        raise ValueError("Missing release workflow jobs")
    declared = fields(jobs[1:], 2)
    if set(declared) != {"version", "build", "package", "publish", "windows_notices", "windows", "windows_publish"}:
        raise ValueError("Unexpected release job set")
    package = declared.pop("package")
    if package[0] != (2, "package:", ()):
        raise ValueError("Unsupported package job")
    properties = fields(package[1:], 4)
    steps = properties.get("steps", [])
    if steps[:1] != [(4, "steps:", ())]:
        raise ValueError("Missing package steps")
    groups = []
    for line in steps[1:]:
        if line[0] == 6 and line[1].startswith("- "):
            groups.append([])
        if not groups:
            raise ValueError("Unsupported package workflow step")
        groups[-1].append(line)
    python = [group for group in groups if any("actions/setup-python" in line[1] for line in group)]
    if len(python) != 1:
        raise ValueError("Missing or ambiguous packaging Python")
    step = python[0]
    versions = [(10, f"python-version: '{version}'", ()) for version in ("3.12", "3.14.6")]
    if (len(step) != 3 or step[0][2]
            or not re.fullmatch(r"- uses: actions/setup-python@[0-9a-f]{40}(?: +#.*)?", step[0][1])
            or step[1] != (8, "with:", ()) or step[2] not in versions):
        raise ValueError("Unverified packaging Python setup")
    step[2] = versions[0]
    properties["steps"] = [steps[0], *(line for group in groups for line in group)]
    return top, declared, properties


def equivalent_inputs(root, source, target):
    source, target = sha(source), sha(target)
    invalidated = set()
    git(root, "merge-base", "--is-ancestor", source, target)
    raw = git(root, "diff", "--raw", "-z", "--no-renames", "--no-ext-diff", "--no-textconv",
              "--ignore-submodules=none", source, target, "--").split(b"\0")
    if raw[-1] != b"" or len(raw) % 2 != 1:
        raise ValueError("Invalid Checks source diff")
    normalizers = {"scripts/ci.py": script_inputs, WORKFLOW: workflow_inputs, RELEASE_WORKFLOW: release_inputs}
    for index in range(0, len(raw) - 1, 2):
        metadata = raw[index].split()
        path = raw[index + 1].decode("utf-8")
        if len(metadata) != 5 or path not in REPAIR_FILES:
            raise ValueError(f"Product inputs changed: {path}")
        old_mode, new_mode = metadata[0].removeprefix(b":"), metadata[1]
        if path not in NEW_FILES:
            if metadata[-1] != b"M" or old_mode != new_mode or new_mode not in {b"100644", b"100755"}:
                raise ValueError(f"Recovery requires a regular modification: {path}")
        elif old_mode not in {b"000000", b"100644", b"100755"} or new_mode not in {b"000000", b"100644", b"100755"}:
            raise ValueError(f"Invalid recovery file type: {path}")
        if path in normalizers:
            previous = git(root, "show", f"{source}:{path}").decode("utf-8")
            current = git(root, "show", f"{target}:{path}").decode("utf-8")
            normalize = normalizers[path]
            if normalize(previous) != normalize(current):
                raise ValueError(f"Executable check inputs changed: {path}")
            if path == WORKFLOW and workflow_inputs(previous, False) != workflow_inputs(current, False):
                invalidated.add(JOB_NAMES["desktop"])
    return invalidated


def make_proof(client, repository, commit, run_id, donor_id, root=None):
    repository, commit = repository_name(repository), sha(commit)
    run_id, donor_id = identifier(run_id), identifier(donor_id)
    if run_id == donor_id:
        raise ValueError("A Checks run cannot reuse itself")
    current = run_metadata(client, repository, run_id, commit)
    donor = run_metadata(client, repository, donor_id, donor=True)
    invalidated = equivalent_inputs(root or ROOT, donor["head_sha"], commit)
    jobs = jobs_for(client, repository, donor)
    build = jobs.get(JOB_NAMES["build"], {})
    if not succeeded(build):
        raise ValueError("The donor Test build must have actually succeeded")
    source_selection(repository, jobs.get("Source", {}))
    archive = find_artifact(client, repository, donor, "nextest-archive")
    if not timestamp(build.get("started_at")) <= timestamp(archive["created_at"]) <= timestamp(build.get("completed_at")):
        raise ValueError("The test archive does not belong to the successful build")
    return {
        "version": 1, "repository": repository, "target_sha": commit,
        "target_run": current["id"], "target_attempt": current["run_attempt"],
        "source_run": donor["id"], "source_sha": donor["head_sha"],
        "source_attempt": donor["run_attempt"],
        "jobs": [{"id": jobs[name]["id"], "name": name} for name in sorted(NAMES)
                 if name not in invalidated and name in jobs and succeeded(jobs[name])],
        "archive": archive,
    }


def validate_proof(client, repository, commit, run_id, proof, root=None):
    if (not isinstance(proof, dict) or proof.get("version") != 1
            or proof.get("repository") != repository or proof.get("target_sha") != commit
            or proof.get("target_run") != identifier(run_id)):
        raise ValueError("Reuse proof does not match the current Checks run")
    expected = make_proof(client, repository, commit, run_id, proof.get("source_run"), root)
    if json.dumps(proof, sort_keys=True) != json.dumps(expected, sort_keys=True):
        raise ValueError("Reuse proof differs from the original successful Checks evidence")
    return {job["name"] for job in expected["jobs"]}


def read_proof(repository, artifact):
    if artifact["size_in_bytes"] > PROOF_LIMIT:
        raise ValueError("Reuse proof artifact exceeds its size limit")
    content = download(f"repos/{repository}/actions/artifacts/{artifact['id']}/zip", PROOF_LIMIT)
    if "sha256:" + hashlib.sha256(content).hexdigest() != artifact["digest"]:
        raise ValueError("Reuse proof artifact digest changed")
    try:
        with zipfile.ZipFile(io.BytesIO(content)) as archive:
            entries = archive.infolist()
            if (len(entries) != 1 or entries[0].filename != "reuse.json"
                    or entries[0].is_dir() or entries[0].file_size > PROOF_LIMIT
                    or stat.S_ISLNK(entries[0].external_attr >> 16)):
                raise ValueError("Reuse proof archive must contain only reuse.json")
            return json.loads(archive.read(entries[0]).decode("utf-8"))
    except (zipfile.BadZipFile, UnicodeError, RuntimeError, OSError) as error:
        raise ValueError("Invalid reuse proof archive") from error


def verified_jobs(client, repository, commit, run_id, root=None):
    repository, commit, run_id = repository_name(repository), sha(commit), identifier(run_id)
    metadata = {"id": run_id, "head_sha": commit}
    artifact = find_artifact(client, repository, metadata, "checks-reuse", optional=True)
    if artifact is None:
        return set()
    proof = read_proof(repository, artifact)
    return validate_proof(client, repository, commit, run_id, proof, root)


def reuse_outputs(proof):
    names = {job["name"] for job in proof["jobs"]} if proof else set()
    missing = [partition for partition, name in SHARDS.items() if name not in names]
    return {
        "proof": json.dumps(proof, separators=(",", ":")) if proof else "",
        "archive_run": str(proof["source_run"]) if proof else "",
        **{key: str(name in names).lower() for key, name in JOB_NAMES.items()},
        "shared": str(not missing).lower(),
        "shared_matrix": json.dumps({"partition": missing or [1, 2]}, separators=(",", ":")),
    }


def prepare(environment=None, root=None, client=None):
    environment = os.environ if environment is None else environment
    root = root or ROOT
    proof = None
    donor = environment.get("CHECK_REUSE_RUN", "")
    if donor:
        proof = make_proof(client or GitHub(), environment["GITHUB_REPOSITORY"], environment["GITHUB_SHA"],
                           environment["GITHUB_RUN_ID"], donor, root)
    path = Path(root) / ".runtime/ci/reuse.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(proof, separators=(",", ":")) + "\n", encoding="utf-8")
    output = reuse_outputs(proof)
    with Path(environment["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as target:
        target.writelines(f"{key}={value}\n" for key, value in output.items())
    return output


def report(results, proof=None, product=None, client=None, environment=None, root=None):
    checks = checks_module()
    if proof is None:
        return checks.require_results(results, product)
    environment = os.environ if environment is None else environment
    names = validate_proof(client or GitHub(), environment["GITHUB_REPOSITORY"], environment["GITHUB_SHA"],
                           environment["GITHUB_RUN_ID"], proof, root)
    promoted = copy.deepcopy(results)
    mapping = {key: {name} for key, name in JOB_NAMES.items()}
    mapping["shared"] = set(SHARDS.values())
    if isinstance(promoted, dict):
        for key, required in mapping.items():
            job = promoted.get(key)
            if required <= names and isinstance(job, dict) and job.get("result") == "skipped":
                job["result"] = "success"
    return checks.require_results(promoted, product)


def main():
    try:
        if sys.argv[1:] == ["prepare"]:
            prepare()
        elif sys.argv[1:] == ["report"] or (len(sys.argv) == 3 and sys.argv[1] == "report"
                                            and sys.argv[2] in {"desktop", "host", "mobile"}):
            product = sys.argv[2] if len(sys.argv) == 3 else None
            raw = os.getenv("CHECK_REUSE_PROOF", "")
            proof = json.loads(raw) if raw else None
            if raw and not isinstance(proof, dict):
                raise ValueError("Invalid reuse proof")
            mode = report(json.loads(os.environ["CHECK_RESULTS"]), proof, product)
            print(f"{product.title()} ready" if product else "Checks deferred" if mode == "skip" else "Checks passed")
        else:
            raise ValueError("Usage: ci_reuse.py prepare|report [desktop|host|mobile]")
    except (ValueError, KeyError, TypeError, OSError, SyntaxError) as error:
        raise SystemExit(str(error)) from error


if __name__ == "__main__":
    main()
