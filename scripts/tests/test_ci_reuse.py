"""Bounded Checks recovery preserves original evidence and fresh failures."""

import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("checks_reuse", ROOT / "scripts/ci_reuse.py")
reuse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reuse)
ci = reuse.checks_module()
REPOSITORY = "example/sailry"
SOURCE = "a" * 40
TARGET = "b" * 40
DONOR = 100
CURRENT = 200
PREFIX = f"repos/{REPOSITORY}/actions"


def workflow():
    source = "name: Checks\non:\n  workflow_dispatch:\nenv:\n  CARGO_INCREMENTAL: '0'\njobs:\n"
    for name in (*reuse.JOB_NAMES, "shared", "source", "desktop_ready", "host_ready", "mobile_ready",
                 "checks", "light_linux", "light_macos"):
        source += f"  {name}:\n    name: {name}\n    needs: source\n    if: true\n    runs-on: macos-15\n"
        if name == "shared":
            source += "    strategy:\n      fail-fast: false\n      matrix:\n        partition: [1, 2]\n"
        source += "    env:\n      CHECK_SCOPE: ${{ toJSON(needs.source.outputs) }}\n    steps:\n"
        if name in {"shared", "desktop"}:
            source += (
                "      - name: Test archive\n"
                "        uses: actions/download-artifact@37930b1c2abaa49bbe596cd826c3c89aef350131\n"
                "        with:\n          name: nextest-archive\n          path: .runtime/ci\n"
            )
        source += ("      - uses: actions/checkout@pinned\n"
                   "      - name: Execute\n        if: success()\n        run: |\n"
                   f"          python3 scripts/ci.py check {name}\n"
                   "          # This line is inside a shell block\n")
    return source


def restored_workflow():
    prefix, desktop = workflow().split("  desktop:\n", 1)
    return prefix + "  desktop:\n" + desktop.replace(
        "      - name: Test archive\n",
        "      - name: Rust dependency sources\n"
        "        uses: ./.github/actions/rust-cache\n"
        "      - name: Restore locked source checkouts\n"
        "        run: cargo fetch --locked --target aarch64-apple-darwin\n"
        "      - name: Test archive\n",
        1,
    )


def run(run_id, commit, completed=True):
    return {
        "id": run_id, "repository": {"full_name": REPOSITORY},
        "head_repository": {"full_name": REPOSITORY}, "path": reuse.WORKFLOW,
        "head_sha": commit, "head_branch": "main", "event": "workflow_dispatch",
        "status": "completed" if completed else "in_progress",
        "conclusion": "failure" if completed else None, "run_attempt": 1,
    }


def artifact(name, run_id, commit, artifact_id=300):
    return {
        "id": artifact_id, "name": name, "expired": False,
        "size_in_bytes": 1024, "digest": "sha256:" + "c" * 64,
        "created_at": "2026-11-05T01:00:00Z", "expires_at": "2099-01-01T00:00:00Z",
        "workflow_run": {"id": run_id, "head_sha": commit, "head_branch": "main"},
    }


def packed(proof, name="reuse.json"):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr(name, json.dumps(proof))
    return output.getvalue()


class FakeGitHub:
    def __init__(self):
        self.data = {}
        self.calls = []

    def api(self, method, endpoint, payload=None, missing=False):
        self.calls.append((method, endpoint))
        if method != "GET" or payload is not None:
            raise AssertionError("Recovery must only read GitHub evidence")
        if endpoint not in self.data:
            raise AssertionError(f"Unexpected API endpoint: {endpoint}")
        return copy.deepcopy(self.data[endpoint])


class Evidence(unittest.TestCase):
    def setUp(self):
        self.client = FakeGitHub()
        self.donor = run(DONOR, SOURCE)
        self.current = run(CURRENT, TARGET, completed=False)
        self.jobs = [{
            "id": 1000 + index, "name": name, "run_id": DONOR, "head_sha": SOURCE,
            "run_attempt": 1, "status": "completed",
            "conclusion": "failure" if name in {"Desktop", reuse.SHARDS[2]} else "success",
            "started_at": "2026-11-05T00:00:00Z", "completed_at": "2026-11-05T02:00:00Z",
        } for index, name in enumerate(["Source", *sorted(reuse.NAMES)])]
        self.archive = artifact("nextest-archive", DONOR, SOURCE)
        self.artifacts = [self.archive]
        self.client.data.update({
            f"{PREFIX}/runs/{DONOR}": self.donor,
            f"{PREFIX}/runs/{CURRENT}": self.current,
            f"{PREFIX}/runs/{DONOR}/jobs?filter=latest&per_page=100&page=1": {"jobs": self.jobs},
            f"{PREFIX}/runs/{DONOR}/artifacts?per_page=100&page=1": {"artifacts": self.artifacts},
            f"{PREFIX}/runs/{CURRENT}/artifacts?per_page=100&page=1": {"artifacts": []},
        })
        self.log = ("2026-11-05T00:00:00.123Z Check mode: full\n"
                    "2026-11-05T00:00:00.124Z Selected checks: " + ", ".join(sorted(ci.LANES)) + "\n").encode()
        self.download = self.enterContext(patch.object(reuse, "download", return_value=self.log))
        self.inputs = self.enterContext(patch.object(reuse, "equivalent_inputs", return_value=set()))
        self.environment = {"GITHUB_REPOSITORY": REPOSITORY, "GITHUB_SHA": TARGET,
                            "GITHUB_RUN_ID": str(CURRENT)}

    def proof(self):
        return reuse.make_proof(self.client, REPOSITORY, TARGET, CURRENT, DONOR)

    def job(self, name):
        return next(job for job in self.jobs if job["name"] == name)

    def verified(self):
        return reuse.verified_jobs(self.client, REPOSITORY, TARGET, CURRENT)

    def publish_proof(self, proof):
        content = packed(proof)
        metadata = artifact("checks-reuse", CURRENT, TARGET, 400)
        metadata.update(size_in_bytes=len(content), digest="sha256:" + hashlib.sha256(content).hexdigest())
        self.client.data[f"{PREFIX}/runs/{CURRENT}/artifacts?per_page=100&page=1"] = {"artifacts": [metadata]}
        self.download.side_effect = lambda endpoint, limit: content if endpoint.endswith("/zip") else self.log
        return metadata

    def results(self):
        return {
            "source": {"result": "success", "outputs": {**ci.outputs(ci.LANES), "mode": "full"}},
            **{key: {"result": "success"} for key in {*reuse.JOB_NAMES, "shared"}},
            **{key: {"result": "skipped"} for key in ci.LIGHT_JOBS},
        }

    def report(self, results, proof, product=None):
        return reuse.report(results, proof, product, client=self.client, environment=self.environment)

    def test_reuses_only_actual_successes(self):
        proof = self.proof()
        names = {entry["name"] for entry in proof["jobs"]}
        self.assertEqual(names, reuse.NAMES - {"Desktop", reuse.SHARDS[2]})
        self.assertNotIn("Source", names)
        self.assertEqual(proof["archive"]["id"], self.archive["id"])
        self.assertEqual(proof["source_sha"], SOURCE)
        self.inputs.assert_called_once_with(reuse.ROOT, SOURCE, TARGET)
        output = reuse.reuse_outputs(proof)
        self.assertEqual(output["build"], "true")
        self.assertEqual(output["desktop"], "false")
        self.assertEqual(output["shared"], "false")
        self.assertEqual(json.loads(output["shared_matrix"]), {"partition": [2]})
        self.assertEqual(output["archive_run"], str(DONOR))
        self.assertEqual(json.loads(output["proof"]), proof)

    def test_leaves_missing_checks_for_recovery(self):
        self.jobs.remove(self.job("Desktop"))
        self.jobs.remove(self.job(reuse.SHARDS[1]))
        output = reuse.reuse_outputs(self.proof())
        self.assertEqual(output["desktop"], "false")
        self.assertEqual(output["build"], "true")
        self.assertEqual(json.loads(output["shared_matrix"]), {"partition": [1, 2]})

    def test_invalidates_changed_desktop_environment(self):
        self.job("Desktop")["conclusion"] = "success"
        for invalidated in ({"Desktop"}, set()):
            with self.subTest(invalidated=invalidated):
                self.inputs.return_value = invalidated
                proof = self.proof()
                names = {entry["name"] for entry in proof["jobs"]}
                self.assertEqual(names, reuse.NAMES - {reuse.SHARDS[2]} - invalidated)
                self.assertEqual(reuse.reuse_outputs(proof)["desktop"], str(not invalidated).lower())

    def test_accepts_completed_push_donor(self):
        self.donor["event"] = "push"
        self.assertEqual(self.proof()["source_run"], DONOR)

    def test_rejects_untrusted_or_incomplete_donor(self):
        for key, value in (
            ("event", "pull_request"), ("head_branch", "feature"), ("status", "in_progress"),
            ("conclusion", "cancelled"), ("path", ".github/workflows/other.yml"),
            ("repository", {"full_name": "other/project"}),
            ("head_repository", {"full_name": "other/project"}),
        ):
            with self.subTest(key=key, value=value):
                previous = self.donor[key]
                self.donor[key] = value
                with self.assertRaises(ValueError):
                    self.proof()
                self.donor[key] = previous

    def test_rejects_current_mismatch(self):
        for key, value in (("head_sha", SOURCE), ("id", DONOR), ("event", "pull_request"),
                           ("head_branch", "other"), ("conclusion", "cancelled")):
            with self.subTest(key=key):
                previous = self.current[key]
                self.current[key] = value
                with self.assertRaises(ValueError):
                    self.proof()
                self.current[key] = previous
        with self.assertRaises(ValueError):
            reuse.make_proof(self.client, REPOSITORY, TARGET, CURRENT, CURRENT)

    def test_rejects_missing_or_unsuccessful_build(self):
        build = self.job("Test build")
        for status in ("failure", "skipped", "cancelled", None):
            with self.subTest(status=status):
                build["conclusion"] = status
                with self.assertRaisesRegex(ValueError, "actually succeeded"):
                    self.proof()
        self.jobs.remove(build)
        with self.assertRaisesRegex(ValueError, "actually succeeded"):
            self.proof()

    def test_rejects_foreign_or_duplicate_jobs(self):
        build = self.job("Test build")
        for key, value in (("run_id", CURRENT), ("head_sha", TARGET), ("run_attempt", 2)):
            with self.subTest(key=key):
                previous = build[key]
                build[key] = value
                with self.assertRaises(ValueError):
                    self.proof()
                build[key] = previous
        self.jobs.append(copy.deepcopy(build))
        with self.assertRaises(ValueError):
            self.proof()

    def test_requires_full_original_selection(self):
        for content in (b"", b"Check mode: light\nSelected checks: desktop\n",
                        b"Check mode: full\nSelected checks: shared, host\n", self.log + self.log):
            with self.subTest(content=content):
                self.download.return_value = content
                with self.assertRaisesRegex(ValueError, "every product input"):
                    self.proof()
        self.job("Source")["conclusion"] = "failure"
        with self.assertRaisesRegex(ValueError, "Source check"):
            self.proof()

    def test_rejects_stale_or_expired_archive(self):
        for key, value in (
            ("expired", True), ("expires_at", "2020-01-01T00:00:00Z"),
            ("created_at", "2026-11-04T01:00:00Z"), ("created_at", "2026-11-05T03:00:00Z"),
            ("digest", None), ("size_in_bytes", 0),
            ("workflow_run", {"id": CURRENT, "head_sha": SOURCE, "head_branch": "main"}),
            ("workflow_run", {"id": DONOR, "head_sha": TARGET, "head_branch": "main"}),
            ("name", "replacement-archive"),
        ):
            with self.subTest(key=key, value=value):
                previous = self.archive[key]
                self.archive[key] = value
                with self.assertRaises(ValueError):
                    self.proof()
                self.archive[key] = previous
        self.artifacts.append(copy.deepcopy(self.archive))
        with self.assertRaises(ValueError):
            self.proof()
        self.artifacts.clear()
        with self.assertRaises(ValueError):
            self.proof()

    def test_paginates_jobs_and_artifacts(self):
        for kind in ("jobs", "artifacts"):
            query = "filter=latest&" if kind == "jobs" else ""
            endpoint = f"{PREFIX}/runs/{DONOR}/{kind}?{query}per_page=100&page="
            entries = self.jobs if kind == "jobs" else self.artifacts
            self.client.data[endpoint + "1"] = {kind: [{"name": f"Other {index}"} for index in range(100)]}
            self.client.data[endpoint + "2"] = {kind: entries}
        self.assertIn({"name": "Test build", "id": self.job("Test build")["id"]}, self.proof()["jobs"])
        self.assertTrue(any(endpoint.endswith("page=2") for _, endpoint in self.client.calls))

    def test_verifies_small_proof_artifact(self):
        proof = self.proof()
        self.publish_proof(proof)
        self.assertEqual(self.verified(), {job["name"] for job in proof["jobs"]})
        self.assertTrue(all("/artifacts/300/zip" not in call.args[0] for call in self.download.call_args_list))

    def test_absent_proof_keeps_ordinary_path(self):
        self.assertEqual(self.verified(), set())
        self.download.assert_not_called()
        self.inputs.assert_not_called()
        self.assertEqual(len(self.client.calls), 1)
        results = self.results()
        self.assertEqual(reuse.report(results), "full")
        results["desktop"]["result"] = "failure"
        with self.assertRaises(ValueError):
            reuse.report(results)

    def test_proof_rejects_binding_and_evidence_changes(self):
        proof = self.proof()
        for key, value in (("version", 2), ("version", True), ("repository", "other/project"),
                           ("target_sha", SOURCE), ("target_run", DONOR), ("target_attempt", 2),
                           ("source_sha", TARGET), ("source_attempt", 2), ("jobs", []),
                           ("archive", {})):
            with self.subTest(key=key, value=value):
                changed = {**proof, key: value}
                self.publish_proof(changed)
                with self.assertRaises(ValueError):
                    self.verified()
        for invalid in (None, [], "proof", {}):
            with self.subTest(invalid=invalid):
                self.publish_proof(invalid)
                with self.assertRaises(ValueError):
                    self.verified()

    def test_never_recursively_reuses_a_skipped_job(self):
        proof = self.proof()
        proof["jobs"].append({"id": self.job("Desktop")["id"], "name": "Desktop"})
        self.job("Desktop")["conclusion"] = "skipped"
        self.publish_proof(proof)
        with self.assertRaises(ValueError):
            self.verified()
        self.job("Test build")["conclusion"] = "skipped"
        with self.assertRaisesRegex(ValueError, "actually succeeded"):
            self.verified()

    def test_requires_every_current_result(self):
        for job in self.jobs:
            job["conclusion"] = "success"
        proof = self.proof()
        baseline = self.results()
        for name in {*reuse.JOB_NAMES, "shared"}:
            baseline[name]["result"] = "skipped"
        original = copy.deepcopy(baseline)
        self.assertEqual(self.report(baseline, proof), "full")
        self.assertEqual(baseline, original)
        for name in ("source", *reuse.JOB_NAMES, "shared", *ci.LIGHT_JOBS):
            for conclusion in ("failure", "cancelled", "unknown", None):
                with self.subTest(name=name, conclusion=conclusion):
                    results = copy.deepcopy(baseline)
                    results[name]["result"] = conclusion
                    with self.assertRaises(ValueError):
                        self.report(results, proof)
            results = copy.deepcopy(baseline)
            del results[name]
            with self.subTest(name=name, missing=True), self.assertRaises(ValueError):
                self.report(results, proof)
        baseline["source"]["result"] = "skipped"
        with self.assertRaises(ValueError):
            self.report(baseline, proof)

    def test_partial_shared_requires_current_success(self):
        proof = self.proof()
        results = self.results()
        results["build"]["result"] = "skipped"
        self.assertEqual(self.report(results, proof), "full")
        for conclusion in ("skipped", "failure", "cancelled"):
            with self.subTest(conclusion=conclusion):
                results["shared"]["result"] = conclusion
                with self.assertRaises(ValueError):
                    self.report(results, proof)
        self.job(reuse.SHARDS[2])["conclusion"] = "success"
        results["shared"]["result"] = "skipped"
        proof = self.proof()
        self.assertEqual(self.report(results, proof), "full")
        output = reuse.reuse_outputs(proof)
        self.assertEqual(output["shared"], "true")
        self.assertEqual(json.loads(output["shared_matrix"]), {"partition": [1, 2]})

    def test_preserves_product_prerequisites(self):
        proof = self.proof()
        for product in ci.PRODUCTS:
            with self.subTest(product=product):
                results = self.results()
                results["lint"]["result"] = "skipped"
                self.assertEqual(self.report(results, proof, product), "full")
                results[product]["result"] = "failure"
                with self.assertRaises(ValueError):
                    self.report(results, proof, product)
                results = self.results()
                for lane in ci.PRODUCTS[product]:
                    results["source"]["outputs"][lane] = "false"
                with self.assertRaises(ValueError):
                    self.report(results, proof, product)

    def test_prepares_bound_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "outputs"
            environment = {**self.environment, "GITHUB_OUTPUT": str(output), "CHECK_REUSE_RUN": str(DONOR)}
            values = reuse.prepare(environment, root, self.client)
            saved = json.loads((root / ".runtime/ci/reuse.json").read_text())
            self.assertEqual(json.loads(values["proof"]), saved)
            self.assertEqual(dict(line.split("=", 1) for line in output.read_text().splitlines()), values)

    def test_revalidates_product_inputs(self):
        proof = self.proof()
        self.publish_proof(proof)
        self.inputs.side_effect = ValueError("Product inputs changed")
        with self.assertRaisesRegex(ValueError, "Product inputs changed"):
            self.verified()
        with self.assertRaisesRegex(ValueError, "Product inputs changed"):
            self.report(self.results(), proof)


class InputEquivalence(unittest.TestCase):
    def test_accepts_fixture_registry_only(self):
        before = "FIXTURES = {'desktop': {'test_ui_tasks'}}\nVALUE = 1\n"
        after = "FIXTURES = {'shared': {'test_ui_tasks', 'test_ci_reuse'}}\nVALUE = 1\n"
        self.assertEqual(reuse.script_inputs(before), reuse.script_inputs(after))
        self.assertNotEqual(reuse.script_inputs(before), reuse.script_inputs(after.replace("VALUE = 1", "VALUE = 2")))
        for invalid in ("VALUE = 1\n", "FIXTURES = function()\n", "FIXTURES = {}\nFIXTURES = {}\n"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                reuse.script_inputs(invalid)

    def test_accepts_only_recovery_scheduling(self):
        before = workflow()
        after = before.replace("    needs: source\n    if: true\n", "    needs: [source, build]\n    if: >-\n      !cancelled()\n", 8)
        after = after.replace("      matrix:\n        partition: [1, 2]", "      matrix: ${{ fromJSON(needs.source.outputs.reuse_shared_matrix) }}")
        after = after.replace("          path: .runtime/ci\n", "          path: .runtime/ci\n"
                              "          github-token: ${{ github.token }}\n"
                              "          run-id: ${{ needs.source.outputs.reuse_archive_run || github.run_id }}\n")
        after = after.replace("python3 scripts/ci.py check source", "python3 scripts/ci_reuse.py prepare")
        after = after.replace("python3 scripts/ci.py check desktop_ready", "python3 scripts/ci_reuse.py report desktop")
        self.assertEqual(reuse.workflow_inputs(before), reuse.workflow_inputs(after))

        restored = restored_workflow()
        self.assertEqual(reuse.workflow_inputs(before), reuse.workflow_inputs(restored))
        self.assertNotEqual(
            reuse.workflow_inputs(before),
            reuse.workflow_inputs(restored.replace("./.github/actions/rust-cache", "./different", 1)),
        )
        self.assertNotEqual(
            reuse.workflow_inputs(before),
            reuse.workflow_inputs(restored.replace("cargo fetch --locked", "cargo fetch", 1)),
        )

    def test_rejects_changed_executable_inputs(self):
        before = workflow()
        for old, new in (
            ("python3 scripts/ci.py check lint", "true"),
            ("actions/checkout@pinned", "actions/checkout@different"),
            ("runs-on: macos-15", "runs-on: ubuntu-24.04"),
            ("CARGO_INCREMENTAL: '0'", "CARGO_INCREMENTAL: '1'"),
            ("CHECK_SCOPE: ${{ toJSON(needs.source.outputs) }}", "CHECK_SCOPE: '{}'"),
            ("        if: success()", "        if: false"),
            ("# This line is inside a shell block", "# Changed literal content"),
            ("path: .runtime/ci", "path: .runtime/other"),
        ):
            with self.subTest(new=new):
                self.assertNotEqual(reuse.workflow_inputs(before), reuse.workflow_inputs(before.replace(old, new, 1)))
        for matrix in ("partition: [1]", "partition: [1, 2, 3]"):
            with self.subTest(matrix=matrix), self.assertRaises(ValueError):
                reuse.workflow_inputs(before.replace("partition: [1, 2]", matrix))
        with self.assertRaises(ValueError):
            reuse.workflow_inputs(before.replace("          path: .runtime/ci", "          path: .runtime/ci\n          run-id: 123"))

    def test_parses_committed_workflow_layout(self):
        self.assertIn("shared", reuse.workflow_inputs((ROOT / reuse.WORKFLOW).read_text())[1])

    def compare(self, path, old_mode="100644", new_mode="100644", status="M", before="", after=""):
        raw = f":{old_mode} {new_mode} a b {status}\0{path}\0".encode()
        with patch.object(reuse, "git", side_effect=[b"", raw, before.encode(), after.encode()]) as read:
            invalidated = reuse.equivalent_inputs(ROOT, SOURCE, TARGET)
        self.assertEqual(read.call_args_list[0].args, (ROOT, "merge-base", "--is-ancestor", SOURCE, TARGET))
        return invalidated

    def test_invalidates_dependency_source_changes(self):
        for before, after, expected in (
            (workflow(), restored_workflow(), {"Desktop"}),
            (restored_workflow(), workflow(), {"Desktop"}),
            (restored_workflow(), restored_workflow(), set()),
            (workflow(), workflow(), set()),
        ):
            with self.subTest(expected=expected):
                self.assertEqual(self.compare(reuse.WORKFLOW, before=before, after=after), expected)

    def test_rejects_changed_product_paths(self):
        for path in ("apps/desktop/src/main.rs", "Cargo.toml", "Cargo.lock", "plugins", "themes",
                     "scripts/ci-run.py", ".github/actions/native-cache/action.yml"):
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, "Product inputs changed"):
                self.compare(path)

    def test_rejects_added_deleted_or_retyped_repairs(self):
        for mode in (("000000", "100644", "A"), ("100644", "000000", "D"),
                     ("100644", "120000", "T"), ("100644", "100755", "M")):
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                self.compare("scripts/tests/test_ui_tasks.py", *mode)
        for path in reuse.NEW_FILES:
            with self.subTest(path=path):
                self.compare(path, "000000", "100644", "A")
        self.compare("scripts/tests/test_ui_tasks.py")

    def test_checks_script_and_workflow_diffs(self):
        self.compare("CHANGELOG.md")
        self.compare("scripts/ci.py", before="FIXTURES = {'desktop': set()}\n", after="FIXTURES = {'shared': set()}\n")
        self.compare(reuse.WORKFLOW, before=workflow(), after=workflow().replace("    if: true", "    if: false", 1))
        with self.assertRaisesRegex(ValueError, "Executable check inputs changed"):
            self.compare(reuse.WORKFLOW, before=workflow(), after=workflow().replace("check lint", "check desktop", 1))

    def test_requires_ancestor_history(self):
        with patch.object(reuse, "git", side_effect=ValueError("not ancestral")), self.assertRaises(ValueError):
            reuse.equivalent_inputs(ROOT, SOURCE, TARGET)
        with patch.object(reuse, "subprocess") as process, self.assertRaises(ValueError):
            process.run.return_value.returncode = 1
            reuse.git(ROOT, "merge-base", "--is-ancestor", SOURCE, TARGET)


class ProofArchive(unittest.TestCase):
    def setUp(self):
        self.usage = self.enterContext(patch.object(
            reuse.subprocess, "run", return_value=Mock(returncode=0, stdout="--allow-escape-sequences")))

    def read(self, content, size=None):
        metadata = {"id": 400, "size_in_bytes": len(content) if size is None else size,
                    "digest": "sha256:" + hashlib.sha256(content).hexdigest()}
        with patch.object(reuse, "download", return_value=content):
            return reuse.read_proof(REPOSITORY, metadata)

    def test_download_stops_at_size_limit(self):
        process = Mock()
        process.stdout.read.return_value = b"x" * 11
        process.__enter__ = Mock(return_value=process)
        process.__exit__ = Mock(return_value=False)
        with patch.object(reuse.subprocess, "Popen", return_value=process), self.assertRaises(ValueError):
            reuse.download("repos/example/sailry/actions/artifacts/1/zip", 10)
        process.stdout.read.assert_called_once_with(11)
        process.kill.assert_called_once_with()

    def test_download_supports_both_cli_versions(self):
        process = Mock()
        process.stdout.read.return_value = b"\x1b[36mCheck mode: full\x1b[0m"
        process.wait.return_value = 0
        process.__enter__ = Mock(return_value=process)
        process.__exit__ = Mock(return_value=False)
        endpoint = "repos/example/sailry/actions/jobs/1/logs"
        for options in ("--allow-escape-sequences", ""):
            with self.subTest(options=options), patch.object(reuse.subprocess, "Popen", return_value=process) as launch:
                self.usage.return_value.stdout = options
                self.assertEqual(reuse.download(endpoint, 100), process.stdout.read.return_value)
                self.assertEqual(launch.call_args.args[0], ["gh", "api", *([options] if options else []), endpoint])

    def test_download_requires_available_api_options(self):
        self.usage.return_value.returncode = 1
        with self.assertRaisesRegex(ValueError, "API options are unavailable"):
            reuse.download("repos/example/sailry/actions/jobs/1/logs", 10)

    def test_download_redacts_failure_details(self):
        process = Mock()
        process.stdout.read.return_value = b""
        process.wait.return_value = 1
        process.__enter__ = Mock(return_value=process)
        process.__exit__ = Mock(return_value=False)

        def failure(command, stdout, stderr):
            stderr.write(b"gh: request https://storage.example/logs?secret=value secret-token failed (HTTP 403)\n")
            return process

        with patch.object(reuse.subprocess, "Popen", side_effect=failure), \
                patch.dict(os.environ, {"GH_TOKEN": "secret-token"}), \
                self.assertRaisesRegex(ValueError, "HTTP 403") as caught:
            reuse.download("repos/example/sailry/actions/jobs/1/logs", 10)
        self.assertNotIn("secret", str(caught.exception))
        self.assertNotIn("https://", str(caught.exception))

    def test_rejects_unsafe_or_extra_entries(self):
        for name in ("../reuse.json", "/reuse.json", "folder/reuse.json", "other.json"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.read(packed({}, name))
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            archive.writestr("reuse.json", "{}")
            archive.writestr("other", "extra")
        with self.assertRaises(ValueError):
            self.read(output.getvalue())
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            entry = zipfile.ZipInfo("reuse.json")
            entry.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(entry, "{}")
        with self.assertRaises(ValueError):
            self.read(output.getvalue())

    def test_rejects_oversized_or_corrupt_proof(self):
        for content, size in ((packed({}), reuse.PROOF_LIMIT + 1), (b"invalid", None),
                              (packed("x" * reuse.PROOF_LIMIT), None)):
            with self.subTest(size=size), self.assertRaises(ValueError):
                self.read(content, size)
        metadata = {"id": 400, "size_in_bytes": 10, "digest": "sha256:" + "0" * 64}
        with patch.object(reuse, "download", return_value=packed({})), self.assertRaisesRegex(ValueError, "digest"):
            reuse.read_proof(REPOSITORY, metadata)


class Commands(unittest.TestCase):
    def test_empty_prepare_avoids_network(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "outputs"
            client = Mock()
            values = reuse.prepare({"GITHUB_OUTPUT": str(output)}, root, client)
            client.api.assert_not_called()
            self.assertEqual(values["proof"], "")
            self.assertEqual(values["archive_run"], "")
            self.assertEqual(json.loads(values["shared_matrix"]), {"partition": [1, 2]})
            self.assertTrue(all(values[key] == "false" for key in {*reuse.JOB_NAMES, "shared"}))
            self.assertEqual(json.loads((root / ".runtime/ci/reuse.json").read_text()), None)

    def test_plain_report_preserves_existing_gate(self):
        results = {
            "source": {"result": "success", "outputs": {**ci.outputs(ci.LANES), "mode": "full"}},
            **{name: {"result": "success"} for name in {*ci.CHECK_JOBS, "lint"}},
            **{name: {"result": "skipped"} for name in ci.LIGHT_JOBS},
        }
        for product in (None, *ci.PRODUCTS):
            with self.subTest(product=product):
                outcome = subprocess.run(
                    [sys.executable, str(ROOT / "scripts/ci_reuse.py"), "report", *([product] if product else [])],
                    env={**os.environ, "CHECK_RESULTS": json.dumps(results), "CHECK_REUSE_PROOF": ""},
                    text=True, capture_output=True,
                )
                self.assertEqual(outcome.returncode, 0, outcome.stderr)
                self.assertIn(f"{product.title()} ready" if product else "Checks passed", outcome.stdout)
        for proof in ("null", "[]", '""'):
            with self.subTest(proof=proof), patch.dict(os.environ, {"CHECK_REUSE_PROOF": proof}), \
                    patch.object(sys, "argv", ["ci_reuse.py", "report"]), self.assertRaises(SystemExit) as result:
                reuse.main()
            self.assertIn("Invalid reuse proof", str(result.exception))


if __name__ == "__main__":
    unittest.main()
