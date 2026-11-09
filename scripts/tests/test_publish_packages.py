"""Independent publishers verify release sources and replace changed assets without network writes."""

import copy
import subprocess
import importlib.util
import io
import json
from pathlib import Path
import re
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("publisher", ROOT / "scripts/publish-packages.py")
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)
publication = publisher.publication
TAG = "v0.1.0-alpha.1"
COMMIT = "a" * 40
REPOSITORY = "fixture/product"


def metadata(draft=False):
    return {"id": 12, "tag_name": TAG, "draft": draft, "prerelease": True,
            "published_at": None if draft else "2026-10-15T00:00:00Z",
            "immutable": False, "assets": []}


class FakeGitHub:
    def __init__(self, release=None):
        self.release = release
        self.calls = []
        self.uploads = []

    def api(self, method, endpoint, payload=None, missing=False):
        self.calls.append((method, endpoint, payload))
        if "/commits/" in endpoint:
            return {"sha": COMMIT}
        if "/releases?" in endpoint:
            return [copy.deepcopy(self.release)] if self.release else []
        if method == "GET":
            return copy.deepcopy(self.release)
        if method == "POST":
            self.release = metadata(draft=True)
            self.release.update(payload)
            return copy.deepcopy(self.release)
        if method == "PATCH":
            self.release.update(payload)
            self.release["published_at"] = "2026-10-15T00:00:00Z"
            return copy.deepcopy(self.release)
        raise AssertionError("Unexpected fixture API call")

    def upload(self, tag, assets):
        for path in assets:
            self.uploads.append(path.name)
            self.release["assets"] = [existing for existing in self.release["assets"]
                                      if existing["name"] != path.name]
            self.release["assets"].append(asset(path))

    def digest(self, repository, existing):
        return existing["fixture_digest"]


def asset(path):
    return {"id": 5, "name": path.name, "state": "uploaded", "size": path.stat().st_size,
            "digest": publication.digest(path)}


class PackagePublication(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.desktop = self.assets("desktop")
        self.host = self.assets("host")

    def assets(self, component):
        output = self.root / component
        output.mkdir()
        for name in publisher.filenames(component):
            (output / name).write_bytes(("fixture " + name).encode())
        return sorted(output.iterdir())

    def test_public_adhoc_precedes_formal_append(self):
        initial = self.assets("desktop-adhoc")
        client = FakeGitHub(metadata())
        publisher.publish("desktop-adhoc", TAG, initial, REPOSITORY, COMMIT, client)
        self.assertFalse(client.release["draft"])
        self.assertEqual(len(client.uploads), 6)
        self.assertTrue(all("-adhoc" in path.name and TAG[1:] not in path.name for path in initial))
        original = copy.deepcopy(client.release["assets"])
        publisher.publish("desktop", TAG, self.desktop, REPOSITORY, COMMIT, client)
        self.assertEqual(client.release["assets"][:6], original)
        self.assertEqual(len(client.release["assets"]), 14)
        count = len(client.uploads)
        publisher.publish("desktop", TAG, self.desktop, REPOSITORY, COMMIT, client)
        self.assertEqual(len(client.uploads), count)

    def test_preserves_signed_arm_during_initial_publication(self):
        adhoc = self.assets("desktop-adhoc")
        initial = [path for path in adhoc if "x86_64" in path.name] + [
            path for path in self.desktop if "aarch64" in path.name]
        client = FakeGitHub(metadata())
        publisher.publish("desktop-adhoc", TAG, initial, REPOSITORY, COMMIT, client, signed_arm=True)
        self.assertEqual(len(client.release["assets"]), 7)
        original = copy.deepcopy(client.release["assets"])
        publisher.publish("desktop", TAG, self.desktop, REPOSITORY, COMMIT, client)
        self.assertEqual(client.release["assets"][:7], original)
        self.assertEqual(len(client.release["assets"]), 11)

    def test_host_does_not_wait_for_desktop(self):
        client = FakeGitHub(metadata())
        self.publish("host", client)
        self.assertEqual(len(client.release["assets"]), 10)
        self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_host_preserves_incomplete_desktop_assets(self):
        release = metadata()
        release["assets"] = [{**asset(self.desktop[0]), "state": "starter"}]
        client = FakeGitHub(release)
        self.publish("host", client)
        self.assertEqual(client.release["assets"][0], release["assets"][0])
        self.assertEqual(len(client.uploads), 10)

    def publish(self, component, client, assets=None):
        publisher.publish(component, TAG, assets or getattr(self, component),
                          REPOSITORY, COMMIT, client)

    def public_desktop(self):
        release = metadata()
        release["assets"] = [asset(path) for path in self.desktop]
        return release

    def test_rejects_versioned_asset_names(self):
        path = next(path for path in self.desktop if path.suffix == ".zip")
        renamed = path.with_name(path.name.replace("Sailry-", f"Sailry-{TAG[1:]}-", 1))
        path.rename(renamed)
        with self.assertRaisesRegex(ValueError, "complete Desktop asset set"):
            self.publish("desktop", FakeGitHub(), sorted(renamed.parent.iterdir()))

    def test_desktop_appends_first_packages_to_version(self):
        release = metadata()
        release.update(name="Published version", body="Published notes")
        client = FakeGitHub(release)
        self.publish("desktop", client)
        self.assertEqual(len(client.uploads), 8)
        self.assertTrue(all(call[0] == "GET" for call in client.calls))
        self.assertEqual(client.release["name"], "Published version")
        self.assertEqual(client.release["body"], "Published notes")

    def test_desktop_retry_preserves_published_assets(self):
        client = FakeGitHub(self.public_desktop())
        self.publish("desktop", client)
        self.assertFalse(client.uploads)
        self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_host_appends_only_missing_assets(self):
        original = self.public_desktop()
        original["assets"].append(asset(self.host[0]))
        client = FakeGitHub(original)
        self.publish("host", client)
        self.assertEqual(set(client.uploads), {path.name for path in self.host[1:]})
        self.assertTrue(all(call[0] == "GET" for call in client.calls))
        self.assertEqual(client.release["assets"][:8], original["assets"][:8])
        self.publish("host", client)
        self.assertEqual(len(client.uploads), 9)

    def test_partial_retry_skips_matching_assets(self):
        release = metadata()
        release["assets"] = [asset(self.desktop[0])]
        client = FakeGitHub(release)
        self.publish("desktop", client)
        self.assertEqual(len(client.uploads), 7)
        self.assertEqual(client.release["assets"][0], release["assets"][0])

    def test_products_require_a_published_version(self):
        adhoc = self.assets("desktop-adhoc")
        for component, assets in (("desktop", self.desktop), ("desktop-adhoc", adhoc), ("host", self.host)):
            for release in (None, metadata(draft=True)):
                with self.subTest(component=component, release=release):
                    client = FakeGitHub(release)
                    with self.assertRaisesRegex(ValueError, "[Pp]ublish"):
                        self.publish(component, client, assets)
                    self.assertFalse(client.uploads)
                    self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_replaces_changed_assets_and_uploads_missing_assets(self):
        for field, value in (("size", 0), ("digest", "sha256:" + "0" * 64), ("state", "open")):
            with self.subTest(field=field):
                release = self.public_desktop()
                original = copy.deepcopy(release["assets"])
                existing = asset(self.host[-1])
                existing[field] = value
                release["assets"].append(existing)
                client = FakeGitHub(release)
                self.publish("host", client)
                self.assertEqual(set(client.uploads), {path.name for path in self.host})
                self.assertEqual(client.release["assets"][:len(original)], original)
                self.assertEqual(len(client.release["assets"]), len(original) + len(self.host))
                published = {existing["name"]: existing for existing in client.release["assets"]}
                self.assertEqual(published[self.host[-1].name], asset(self.host[-1]))
                self.assertTrue(all(call[0] == "GET" for call in client.calls))
                self.publish("host", client)
                self.assertEqual(len(client.uploads), len(self.host))

    def test_missing_digest_verifies_existing_bytes(self):
        release = self.public_desktop()
        release["assets"][0]["fixture_digest"] = release["assets"][0].pop("digest")
        client = FakeGitHub(release)
        self.publish("desktop", client)
        self.assertFalse(client.uploads)

    def test_missing_digest_replaces_changed_bytes(self):
        release = self.public_desktop()
        release["assets"][0].pop("digest")
        release["assets"][0]["fixture_digest"] = "sha256:" + "0" * 64
        client = FakeGitHub(release)
        self.publish("desktop", client)
        self.assertEqual(client.uploads, [self.desktop[0].name])
        self.assertEqual(len(client.release["assets"]), len(self.desktop))

    def test_products_require_mutable_release(self):
        for component, assets in (("desktop", self.desktop), ("host", self.host)):
            with self.subTest(component=component):
                client = FakeGitHub({**metadata(), "immutable": True})
                with self.assertRaisesRegex(ValueError, "immutable"):
                    self.publish(component, client, assets)
                self.assertFalse(client.uploads)
                self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_component_allowlists_reject_mixed_or_partial_assets(self):
        for component, files in (("host", self.desktop), ("desktop", self.host),
                                 ("host", self.host[:-1]), ("desktop", self.desktop + self.host)):
            with self.subTest(component=component, count=len(files)):
                with self.assertRaises(ValueError):
                    self.publish(component, FakeGitHub(), files)

    def test_windows_appends_portable_archive(self):
        windows = self.assets("desktop-windows")
        self.assertEqual({path.name for path in windows},
                         {"Sailry-x86_64-pc-windows-msvc.zip", "Sailry-x86_64-pc-windows-msvc-setup.exe",
                          "SHA256SUMS-desktop-x86_64-pc-windows-msvc"})
        release = self.public_desktop()
        original = copy.deepcopy(release["assets"])
        client = FakeGitHub(release)
        self.publish("desktop-windows", client, windows)
        self.assertEqual(client.uploads, [path.name for path in windows])
        self.assertEqual(client.release["assets"][:len(original)], original)
        self.assertTrue(all(call[0] == "GET" for call in client.calls))
        for files in (windows[:1], windows + self.desktop, self.desktop):
            with self.subTest(count=len(files)), self.assertRaises(ValueError):
                self.publish("desktop-windows", FakeGitHub(metadata()), files)
        with self.assertRaisesRegex(ValueError, "[Pp]ublish"):
            self.publish("desktop-windows", FakeGitHub(metadata(draft=True)), windows)

    def test_published_preview_flags_preserve_existing_assets(self):
        for preview_flag in (False, True):
            with self.subTest(prerelease=preview_flag):
                existing = self.public_desktop()
                existing["prerelease"] = preview_flag
                original = copy.deepcopy(existing["assets"])
                client = FakeGitHub(existing)
                self.publish("host", client)
                self.assertEqual(client.release["assets"][:len(original)], original)
                self.assertEqual(client.release["prerelease"], preview_flag)
                self.assertFalse(client.release["draft"])
                self.assertEqual(set(client.uploads), {path.name for path in self.host})

    def test_stable_rejects_prerelease_flag(self):
        existing = self.public_desktop()
        existing["tag_name"] = "v0.1.0"
        client = FakeGitHub(existing)
        with self.assertRaisesRegex(ValueError, "channel"):
            publication.release(client, REPOSITORY, "v0.1.0", COMMIT, published=True)
        self.assertFalse(client.uploads)
        self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_rejects_moved_or_different_tag(self):
        client = FakeGitHub(self.public_desktop())
        with self.assertRaisesRegex(ValueError, "differs"):
            publisher.publish("host", TAG, self.host, REPOSITORY, "b" * 40, client)
        client.release["tag_name"] = "v0.1.0-alpha.2"
        with self.assertRaisesRegex(ValueError, "tag or channel"):
            self.publish("host", client)
        self.assertFalse(client.uploads)


class AssetUpload(unittest.TestCase):
    def test_replaces_existing_asset_names(self):
        result = subprocess.CompletedProcess([], 0, stdout="", stderr="")
        with patch.object(publication.subprocess, "run", return_value=result) as run:
            publication.GitHub().upload(TAG, [Path("package.zip")])
        run.assert_called_once_with(
            ["gh", "release", "upload", TAG, "package.zip", "--clobber"],
            capture_output=True, text=True)


class ReleaseLookup(unittest.TestCase):
    def test_finds_exact_draft_on_later_page(self):
        client = Mock()
        unrelated = {**metadata(draft=True), "tag_name": "v0.1.0-alpha.2"}
        expected = metadata(draft=True)
        client.api.side_effect = [None, [unrelated] * 100, [unrelated, expected], {"sha": COMMIT}]
        self.assertEqual(publication.release(client, REPOSITORY, TAG, COMMIT), expected)
        self.assertTrue(client.api.call_args_list[0].kwargs["missing"])
        self.assertIn("per_page=100&page=2", client.api.call_args_list[2].args[1])
        self.assertTrue(all(call.args[0] == "GET" for call in client.api.call_args_list))

    def test_missing_tag_preserves_publication_requirement(self):
        for published in (False, True):
            client = Mock()
            unrelated = {**metadata(draft=True), "tag_name": "v0.1.0-alpha.2"}
            client.api.side_effect = [None, [unrelated], {"sha": COMMIT}]
            if published:
                with self.assertRaisesRegex(ValueError, "Publish this version"):
                    publication.release(client, REPOSITORY, TAG, COMMIT, published=True)
            else:
                self.assertIsNone(publication.release(client, REPOSITORY, TAG, COMMIT))
            self.assertEqual(client.api.call_count, 3)
            self.assertTrue(all(call.args[0] == "GET" for call in client.api.call_args_list))


class SourceReuse(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.git("init", "--quiet")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "user.name", "Fixture")
        for name in {*publication.SIGNING_FIX_FILES, "Cargo.toml", "crates/node-runtime/src/lib.rs"}:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("original\n")
        self.parent = self.commit()

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args], stderr=subprocess.PIPE, text=True).strip()

    def commit(self):
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Fixture")
        return self.git("rev-parse", "HEAD")

    def test_admits_only_modified_signing_tools(self):
        for name in publication.SIGNING_FIX_FILES:
            content = "reviewed signing fix\n"
            (self.root / name).write_text(content)
        commit = self.commit()
        self.assertEqual(publication.checked_source(commit, self.root), self.parent)

    def test_rejects_product_inputs_and_unlisted_tools(self):
        for name in ("Cargo.toml", "Cargo.lock", "crates/node-runtime/src/lib.rs", ".github/workflows/other.yml"):
            with self.subTest(name=name):
                path = self.root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("unverified input\n")
                commit = self.commit()
                self.assertIsNone(publication.checked_source(commit, self.root))

    def test_rejects_workflow_changes(self):
        path = self.root / ".github/workflows/release.yml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("changed build recipe\n")
        self.assertIsNone(publication.checked_source(self.commit(), self.root))

    def test_rejects_binary_deletion_and_type_changes(self):
        path = self.root / "scripts/package/sign-macos.py"
        path.write_bytes(b"binary\0fixture")
        self.assertIsNone(publication.checked_source(self.commit(), self.root))
        path.unlink()
        self.assertIsNone(publication.checked_source(self.commit(), self.root))
        path.symlink_to("../../../Cargo.toml")
        self.assertIsNone(publication.checked_source(self.commit(), self.root))

    def test_rejects_renames_and_gitlinks(self):
        path = self.root / "scripts/package/sign-macos.py"
        path.rename(path.with_name("renamed.py"))
        self.assertIsNone(publication.checked_source(self.commit(), self.root))
        self.git("update-index", "--add", "--cacheinfo", "160000," + self.parent + ",plugins")
        self.git("commit", "--quiet", "-m", "Gitlink fixture")
        self.assertIsNone(publication.checked_source(self.git("rev-parse", "HEAD"), self.root))

    def test_rejects_merges_and_other_checkouts(self):
        tree = self.git("rev-parse", "HEAD^{tree}")
        other = self.git("commit-tree", tree, "-p", self.parent, "-m", "Other parent")
        merge = self.git("commit-tree", tree, "-p", self.parent, "-p", other, "-m", "Merge")
        self.git("checkout", "--quiet", "--detach", merge)
        self.assertIsNone(publication.checked_source(merge, self.root))
        self.assertIsNone(publication.checked_source(self.parent, self.root))

    def test_fetches_only_the_shallow_parent(self):
        (self.root / "scripts/package/sign-macos.py").write_text("reviewed fix\n")
        commit = self.commit()
        clone = self.root / "shallow"
        subprocess.run(["git", "clone", "--quiet", "--depth=1", "file://" + str(self.root), str(clone)],
                       capture_output=True, check=True)
        self.assertEqual(publication.checked_source(commit, clone), self.parent)
        self.assertEqual(self.git("rev-parse", "HEAD"), commit)




class BuildLogArchive(unittest.TestCase):
    def archive(self, entries):
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            for name, content in entries:
                archive.writestr(name, content)
        return output.getvalue()

    def download(self, content, returncode=0):
        result = subprocess.CompletedProcess([], returncode, stdout=content, stderr=b"")
        with patch.object(publication.subprocess, "run", return_value=result) as run:
            log = publication.GitHub().build_log(REPOSITORY, "123", "Desktop aarch64-apple-darwin")
        run.assert_called_once_with(["gh", "api", "repos/fixture/product/actions/runs/123/logs"], capture_output=True)
        return log

    def test_reads_only_the_original_target_job(self):
        archive = self.archive([
            ("1_Desktop x86_64-apple-darwin.txt", "Other target"),
            ("2_Desktop aarch64-apple-darwin.txt", "Complete ARM log"),
            ("Desktop aarch64-apple-darwin/system.txt", "Runner metadata")])
        self.assertEqual(self.download(archive), "Complete ARM log")

    def test_rejects_missing_ambiguous_or_nested_target(self):
        name = "2_Desktop aarch64-apple-darwin.txt"
        for entries in ([("1_Desktop x86_64-apple-darwin.txt", "Other")],
                        [(name, "First"), ("3_Desktop aarch64-apple-darwin.txt", "Second")],
                        [("nested/" + name, "Nested")]):
            with self.subTest(entries=entries), self.assertRaisesRegex(ValueError, "missing or ambiguous"):
                self.download(self.archive(entries))

    def test_rejects_failed_or_invalid_archive(self):
        for content, returncode in ((b"Forbidden", 1), (b"Not a ZIP", 0), (b"PK\x03\x04broken", 0)):
            with self.subTest(content=content), self.assertRaises(RuntimeError):
                self.download(content, returncode)
        with self.assertRaises(RuntimeError):
            self.download(self.archive([("2_Desktop aarch64-apple-darwin.txt", b"\xff")]))


class NativeBinary(unittest.TestCase):
    def test_rejects_unsafe_or_wrong_architecture(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "binary"
            path.write_bytes(b"\xcf\xfa\xed\xfe" + (0x100000C).to_bytes(4, "little"))
            path.chmod(0o755)
            publication.native_binary(path, "aarch64-apple-darwin")
            with self.assertRaises(ValueError):
                publication.native_binary(path, "x86_64-apple-darwin")
            path.chmod(0o644)
            with self.assertRaises(ValueError):
                publication.native_binary(path, "aarch64-apple-darwin")
            link = path.with_name("link")
            link.symlink_to(path)
            with self.assertRaises(ValueError):
                publication.native_binary(link, "aarch64-apple-darwin")


class Readiness(unittest.TestCase):
    def setUp(self):
        reuse = patch.object(publication.ci_reuse, "verified_jobs", return_value=set())
        self.reuse = reuse.start()
        self.addCleanup(reuse.stop)

    def test_required_jobs_exist(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        names = set()
        jobs = workflow.split("\njobs:\n", 1)[1]
        for job in re.split(r"^  [a-z_]+:\n", jobs, flags=re.MULTILINE)[1:]:
            name = re.search(r"^    name: (.+)$", job, re.MULTILINE)[1]
            if "${{ matrix.partition }}" in name:
                self.assertIn("matrix: ${{ fromJSON(needs.source.outputs.reuse_shared_matrix) }}", job)
                partitions = json.loads(publication.ci_reuse.reuse_outputs(None)["shared_matrix"])["partition"]
                self.assertEqual(partitions, [1, 2])
                names.update(name.replace("${{ matrix.partition }}", str(partition))
                             for partition in partitions)
            else:
                names.add(name)
        for component, prerequisites in publication.PREREQUISITES.items():
            with self.subTest(component=component):
                self.assertLessEqual({"Source", "Rust strict lint", "Test build", "Shared Node (1/2)", "Shared Node (2/2)"},
                                     set(prerequisites))
                self.assertLessEqual(set(prerequisites) | {f"{component.title()} ready"}, names)

    def run_fixture(self, gate="success", event="push", branch="main", sha=COMMIT, overall="failure", component="desktop"):
        client = Mock()
        client.api.side_effect = [
            {"workflow_runs": [{"id": 42, "head_sha": sha, "head_branch": branch,
                                "event": event, "conclusion": overall}]},
            {"jobs": [{"name": name, "status": "completed", "conclusion": "success"}
                      for name in publication.PREREQUISITES[component]] +
                     [{"name": f"{component.title()} ready", "status": "completed", "conclusion": gate}]},
        ]
        return client

    def test_exact_checks_take_precedence(self):
        with patch.object(publication, "checked_source") as source:
            self.assertEqual(publication.readiness(self.run_fixture(), REPOSITORY, COMMIT, "desktop"), 42)
        source.assert_not_called()
        self.reuse.assert_not_called()

    def test_preserves_run_pagination(self):
        client = self.run_fixture()
        runs, jobs = list(client.api.side_effect)
        unrelated = {**runs["workflow_runs"][0], "head_sha": "b" * 40}
        client.api.side_effect = [{"workflow_runs": [unrelated] * 100}, runs, jobs]
        self.assertEqual(publication.ready_run(client, REPOSITORY, COMMIT, "desktop"), 42)
        self.assertIn("per_page=100&page=2", client.api.call_args_list[1].args[1])
        self.reuse.assert_not_called()

    def test_reuses_only_successful_direct_parent_checks(self):
        parent = "b" * 40
        for gate in ("success", "failure"):
            with self.subTest(gate=gate):
                fixture = self.run_fixture(gate=gate, sha=parent)
                responses = list(fixture.api.side_effect)
                fixture.api.side_effect = [{"workflow_runs": []}, *responses]
                with patch.object(publication, "checked_source", return_value=parent) as source:
                    if gate == "success":
                        self.assertEqual(publication.readiness(fixture, REPOSITORY, COMMIT, "desktop"), 42)
                    else:
                        with self.assertRaises(ValueError):
                            publication.readiness(fixture, REPOSITORY, COMMIT, "desktop")
                source.assert_called_once_with(COMMIT)
                self.assertIn("head_sha=" + parent, fixture.api.call_args_list[1].args[1])

    def test_product_success_admits_despite_other_product_failure(self):
        client = self.run_fixture()
        runs, jobs = list(client.api.side_effect)
        jobs["jobs"].append({"name": "Mobile", "status": "completed", "conclusion": "failure"})
        client.api.side_effect = [runs, jobs]
        self.assertEqual(publication.readiness(client, REPOSITORY, COMMIT, "desktop"), 42)
        self.assertIn("head_sha=" + COMMIT, client.api.call_args_list[0].args[1])
        self.assertIn("filter=latest", client.api.call_args_list[1].args[1])

    def test_rejects_skipped_failed_and_missing_gates(self):
        for conclusion in ("skipped", "failure", "cancelled", None):
            with self.subTest(conclusion=conclusion), self.assertRaises(ValueError):
                publication.readiness(self.run_fixture(gate=conclusion), REPOSITORY, COMMIT, "desktop")
        with self.assertRaises(ValueError):
            publication.readiness(self.run_fixture(), REPOSITORY, COMMIT, "host")

    def test_rejects_partial_pr_and_wrong_sha(self):
        for options in ({"event": "pull_request"}, {"branch": "feature"}, {"sha": "b" * 40}):
            client = self.run_fixture(**options)
            with self.subTest(options=options), self.assertRaises(ValueError):
                publication.readiness(client, REPOSITORY, COMMIT, "desktop")
            self.assertEqual(client.api.call_count, 1)

    def test_full_manual_main_can_admit(self):
        self.assertEqual(publication.readiness(self.run_fixture(event="workflow_dispatch"),
                                              REPOSITORY, COMMIT, "desktop"), 42)

    def test_accepts_only_the_selected_tag_dispatch(self):
        for component in publication.PREREQUISITES:
            client = self.run_fixture(event="workflow_dispatch", branch=TAG, component=component)
            with self.subTest(component=component):
                self.assertEqual(publication.readiness(client, REPOSITORY, COMMIT, component, tag=TAG), 42)
        for tag, event, branch in ((None, "workflow_dispatch", TAG), (TAG, "workflow_dispatch", "v0.2.0"),
                                   (TAG, "push", TAG), (TAG, "pull_request", TAG)):
            client = self.run_fixture(event=event, branch=branch)
            with self.subTest(tag=tag, event=event, branch=branch), self.assertRaises(ValueError):
                publication.readiness(client, REPOSITORY, COMMIT, "desktop", tag=tag)
            self.assertEqual(client.api.call_count, 1)

    def test_requires_actual_prerequisites_with_success_gate(self):
        for name in publication.PREREQUISITES["desktop"]:
            for conclusion in ("failure", "skipped"):
                client = self.run_fixture()
                runs, jobs = list(client.api.side_effect)
                for job in jobs["jobs"]:
                    if job["name"] == name:
                        job["conclusion"] = conclusion
                client.api.side_effect = [runs, jobs]
                with self.subTest(name=name, conclusion=conclusion), self.assertRaises(ValueError):
                    publication.readiness(client, REPOSITORY, COMMIT, "desktop")

    def test_requires_build_and_both_shards(self):
        for component in publication.PREREQUISITES:
            for name in ("Test build", "Shared Node (1/2)", "Shared Node (2/2)"):
                for state in ("missing", "failure", "skipped", "cancelled", "in_progress"):
                    with self.subTest(component=component, name=name, state=state):
                        client = self.run_fixture(component=component)
                        runs, jobs = list(client.api.side_effect)
                        job = next(job for job in jobs["jobs"] if job["name"] == name)
                        if state == "missing":
                            jobs["jobs"].remove(job)
                        elif state == "in_progress":
                            job.update(status=state, conclusion=None)
                        else:
                            job["conclusion"] = state
                        client.api.side_effect = [runs, jobs]
                        with patch.object(publication, "checked_source", return_value=None), self.assertRaises(ValueError):
                            publication.readiness(client, REPOSITORY, COMMIT, component)

    def test_does_not_combine_different_runs(self):
        client = self.run_fixture()
        runs, jobs = list(client.api.side_effect)
        runs["workflow_runs"].append({**runs["workflow_runs"][0], "id": 43})
        client.api.side_effect = [
            runs,
            {"jobs": [job for job in jobs["jobs"] if job["name"] != "Desktop ready"]},
            {"jobs": [job for job in jobs["jobs"] if job["name"] == "Desktop ready"]},
        ]
        with patch.object(publication, "checked_source", return_value=None), self.assertRaises(ValueError):
            publication.readiness(client, REPOSITORY, COMMIT, "desktop")
        self.assertIn("/runs/42/jobs?", client.api.call_args_list[1].args[1])
        self.assertIn("/runs/43/jobs?", client.api.call_args_list[2].args[1])

    def test_tag_must_match_checkout_and_product(self):
        with patch.object(publication.versions, "product", return_value=(TAG[1:], 1)), \
                patch.object(publication.subprocess, "check_output", side_effect=[COMMIT, "b" * 40]):
            with self.assertRaisesRegex(ValueError, "exact commit"):
                publication.tagged_commit(TAG)
        with patch.object(publication.versions, "product", return_value=(TAG[1:], 1)):
            with self.assertRaisesRegex(ValueError, "application version"):
                publication.tagged_commit("v0.1.0-alpha.2")


class RecoveredReadiness(unittest.TestCase):
    def setUp(self):
        self.revision = "b" * 40
        self.root = ROOT / ".workflow-tools"
        self.candidate = {
            "id": 42, "head_sha": self.revision, "head_branch": "main", "event": "workflow_dispatch",
            "path": publication.ci_reuse.WORKFLOW, "status": "completed", "conclusion": "success",
        }
        self.pages = {1: [self.candidate]}
        self.jobs = {42: [{"name": name, "status": "completed", "conclusion": "success"}
                          for name in (*publication.PREREQUISITES["desktop"], "Desktop ready")]}
        self.client = Mock()
        self.client.api.side_effect = self.api
        self.git = self.enterContext(patch.object(
            publication.ci_reuse, "git", side_effect=[self.revision.encode(), b"false"]))
        self.inputs = self.enterContext(patch.object(publication.ci_reuse, "equivalent_inputs", return_value=set()))
        self.parent = self.enterContext(patch.object(publication, "checked_source", return_value=None))
        self.verified = self.enterContext(patch.object(publication.ci_reuse, "verified_jobs", return_value=set()))
        self.artifact = self.enterContext(patch.object(
            publication.ci_reuse, "find_artifact", return_value={"id": 13}))
        self.proof = {"source_sha": COMMIT, "target_sha": self.revision}
        self.read = self.enterContext(patch.object(publication.ci_reuse, "read_proof", return_value=self.proof))
        self.validate = self.enterContext(patch.object(publication.ci_reuse, "validate_proof"))

    def api(self, method, endpoint):
        self.assertEqual(method, "GET")
        prefix = f"repos/{REPOSITORY}/actions"
        query = prefix + "/workflows/ci.yml/runs?branch=main&status=completed&per_page=100&page="
        if endpoint.startswith(query):
            return {"workflow_runs": self.pages.get(int(endpoint.removeprefix(query)), [])}
        for run_id, jobs in self.jobs.items():
            query = f"{prefix}/runs/{run_id}/jobs?filter=latest&per_page=100&page="
            if endpoint.startswith(query):
                page = int(endpoint.removeprefix(query))
                return {"jobs": jobs[(page - 1) * 100:page * 100]}
        raise AssertionError(f"Unexpected recovery request: {method} {endpoint}")

    def recover(self):
        return publication.recovered_run(self.client, REPOSITORY, COMMIT, "desktop", self.root)

    def test_requires_unchanged_source_and_verified_proof(self):
        self.assertEqual(self.recover(), 42)
        self.inputs.assert_called_once_with(self.root, COMMIT, self.revision)
        self.validate.assert_called_once_with(
            self.client, REPOSITORY, self.revision, 42, self.proof, self.root)
        self.parent.assert_not_called()

    def test_discovers_older_tooling_checks(self):
        candidate = "c" * 40
        self.candidate["head_sha"] = candidate
        self.proof["target_sha"] = candidate
        self.assertEqual(self.recover(), 42)
        self.assertEqual([call.args for call in self.inputs.call_args_list], [
            (self.root, COMMIT, self.revision), (self.root, COMMIT, candidate),
            (self.root, candidate, self.revision),
        ])
        self.parent.assert_not_called()
        self.artifact.assert_called_once_with(
            self.client, REPOSITORY, {"id": 42, "head_sha": candidate}, "checks-reuse", optional=True)
        self.validate.assert_called_once_with(
            self.client, REPOSITORY, candidate, 42, self.proof, self.root)

    def test_skips_nonmatching_proof(self):
        self.pages[1].insert(0, {**self.candidate, "id": 43})
        self.jobs[43] = copy.deepcopy(self.jobs[42])
        self.candidate["head_sha"] = "c" * 40
        self.proof["target_sha"] = self.candidate["head_sha"]
        self.read.side_effect = [{"source_sha": "d" * 40}, self.proof]
        self.assertEqual(self.recover(), 42)
        self.validate.assert_called_once_with(
            self.client, REPOSITORY, self.candidate["head_sha"], 42, self.proof, self.root)

    def test_paginates_completed_checks(self):
        self.pages[1] = [{**self.candidate, "head_branch": "other"}] * 100
        self.pages[2] = [self.candidate]
        self.assertEqual(self.recover(), 42)
        self.assertIn("page=2", self.client.api.call_args_list[1].args[1])

    def test_paginates_candidate_jobs(self):
        self.jobs[42] = [{"name": f"Other {index}"} for index in range(100)] + self.jobs[42]
        self.assertEqual(self.recover(), 42)
        self.assertIn("jobs?filter=latest&per_page=100&page=2", self.client.api.call_args_list[2].args[1])

    def test_bounds_history_queries(self):
        self.pages = {page: [{**self.candidate, "head_branch": "other"}] * 100
                      for page in range(1, publication.RECOVERY_PAGES + 1)}
        self.assertIsNone(self.recover())
        self.assertEqual(self.client.api.call_count, publication.RECOVERY_PAGES)
        self.artifact.assert_not_called()

    def test_rejects_invalid_run_pages(self):
        for payload in (None, {}, {"workflow_runs": {}}, {"workflow_runs": [None]},
                        {"workflow_runs": [self.candidate] * 101}):
            with self.subTest(payload=payload):
                self.git.side_effect = [self.revision.encode(), b"false"]
                self.client.api.side_effect = [payload]
                with self.assertRaisesRegex(ValueError, "Invalid recovery Checks run page"):
                    self.recover()
        self.artifact.assert_not_called()

    def test_rejects_untrusted_or_cancelled_runs(self):
        for key, value in (("head_branch", "other"), ("event", "pull_request"),
                           ("path", ".github/workflows/other.yml"), ("status", "in_progress"),
                           ("conclusion", "cancelled"), ("head_sha", "invalid"), ("id", True)):
            with self.subTest(key=key, value=value):
                self.git.side_effect = [self.revision.encode(), b"false"]
                previous = self.candidate[key]
                self.candidate[key] = value
                self.assertIsNone(self.recover())
                self.candidate[key] = previous
        self.artifact.assert_not_called()

    def test_preserves_component_readiness(self):
        self.candidate["conclusion"] = "failure"
        self.jobs[42].append({"name": "Mobile", "status": "completed", "conclusion": "failure"})
        self.assertEqual(self.recover(), 42)

    def test_rejects_unverified_candidate_history(self):
        self.candidate["head_sha"] = "c" * 40
        for index in (1, 2):
            for message in ("Checks input history is unavailable or not ancestral",
                            "Product inputs changed: Cargo.toml", "Executable check inputs changed: ci.yml"):
                with self.subTest(index=index, message=message):
                    self.git.side_effect = [self.revision.encode(), b"false"]
                    results = [set(), set(), set()]
                    results[index] = ValueError(message)
                    self.inputs.side_effect = results
                    self.assertIsNone(self.recover())
        self.artifact.assert_not_called()

    def test_rejects_invalidated_product_checks(self):
        self.candidate["head_sha"] = "c" * 40
        self.inputs.side_effect = [set(), set(), {"Desktop"}]
        self.assertIsNone(self.recover())
        self.artifact.assert_not_called()

    def test_history_failures_remain_visible(self):
        self.git.side_effect = ValueError("Checks input history is unavailable")
        with self.assertRaisesRegex(ValueError, "history is unavailable"):
            self.recover()
        self.client.api.assert_not_called()

    def test_rejects_changed_product_inputs(self):
        self.inputs.side_effect = ValueError("Product inputs changed")
        self.assertIsNone(self.recover())
        self.client.api.assert_not_called()

    def test_requires_successful_recovery_gate(self):
        self.jobs[42][-1]["conclusion"] = "failure"
        self.assertIsNone(self.recover())
        self.artifact.assert_not_called()

    def test_rejects_partial_checks(self):
        baseline = copy.deepcopy(self.jobs[42])
        for name in ("Source", "Test build", "Shared Node (1/2)", "Shared Node (2/2)", "Desktop ready"):
            with self.subTest(name=name):
                self.git.side_effect = [self.revision.encode(), b"false"]
                self.jobs[42] = [job for job in baseline if job["name"] != name]
                self.assertIsNone(self.recover())
        self.artifact.assert_not_called()

    def test_requires_verified_skipped_prerequisites(self):
        self.jobs[42][1]["conclusion"] = "skipped"
        self.assertIsNone(self.recover())
        self.verified.assert_called_once_with(self.client, REPOSITORY, self.revision, 42)
        self.artifact.assert_not_called()

    def test_does_not_combine_different_runs(self):
        gate = self.jobs[42].pop()
        self.pages[1].append({**self.candidate, "id": 43})
        self.jobs[43] = [gate]
        self.assertIsNone(self.recover())
        self.artifact.assert_not_called()

    def test_requires_original_tag_proof(self):
        for proof in (None, {}, {"source_sha": "c" * 40}):
            with self.subTest(proof=proof):
                self.git.side_effect = [self.revision.encode(), b"false"]
                self.read.return_value = proof
                self.assertIsNone(self.recover())
        self.validate.assert_not_called()

    def test_requires_proof_artifact(self):
        self.artifact.return_value = None
        self.assertIsNone(self.recover())
        self.read.assert_not_called()

    def test_rejects_invalid_evidence(self):
        self.validate.side_effect = ValueError("Stale Checks proof")
        with self.assertRaisesRegex(ValueError, "Stale Checks proof"):
            self.recover()

    def test_rejects_changed_proof_digest(self):
        self.read.side_effect = ValueError("Reuse proof artifact digest changed")
        with self.assertRaisesRegex(ValueError, "digest changed"):
            self.recover()
        self.validate.assert_not_called()

    def test_fetches_shallow_tooling_history(self):
        self.git.side_effect = [self.revision.encode(), b"true", b""]
        self.assertEqual(self.recover(), 42)
        self.assertEqual(self.git.call_args.args,
                         (self.root, "fetch", "--quiet", "--no-tags", "--no-recurse-submodules",
                          "--unshallow", "origin", self.revision))

    def test_avoids_duplicate_exact_source_lookup(self):
        self.git.side_effect = [COMMIT.encode()]
        self.assertIsNone(self.recover())
        self.inputs.assert_not_called()
        self.client.api.assert_not_called()

    def test_release_admission_accepts_verified_recovery(self):
        with patch.object(publication, "ready_run", return_value=None), \
                patch.object(publication, "recovered_run", return_value=42) as recovered, \
                patch.object(publication.time, "sleep") as sleep:
            self.assertEqual(publication.readiness(self.client, REPOSITORY, COMMIT, "desktop"), 42)
            self.assertEqual(publication.wait_ready(self.client, REPOSITORY, COMMIT, "desktop"), 42)
        self.assertEqual(recovered.call_count, 2)
        sleep.assert_not_called()


class ChecksRecovery(unittest.TestCase):
    def setUp(self):
        reuse = patch.object(publication.ci_reuse, "verified_jobs")
        self.reuse = reuse.start()
        self.addCleanup(reuse.stop)

    def fixture(self, component="desktop", missing=False, event="workflow_dispatch"):
        client = Mock()
        run = {"id": 42, "head_sha": COMMIT, "head_branch": "main", "event": event,
               "status": "in_progress", "conclusion": None}
        self.reuse.return_value = set(publication.PREREQUISITES[component]) - {"Source"}
        jobs = [{"name": name, "status": "completed", "conclusion": "success"}
                for name in ("Source", f"{component.title()} ready")]
        if not missing:
            jobs.extend({"name": name, "status": "completed", "conclusion": "skipped"}
                        for name in sorted(self.reuse.return_value))

        def api(method, endpoint):
            self.assertEqual(method, "GET")
            if "/actions/workflows/ci.yml/runs?" in endpoint:
                self.assertIn("head_sha=" + COMMIT, endpoint)
                return {"workflow_runs": [run]}
            if "/actions/runs/42/jobs?" in endpoint:
                return {"jobs": jobs}
            if endpoint.endswith("/actions/runs/42"):
                return run
            raise AssertionError(f"Unexpected recovery request: {method} {endpoint}")

        client.api.side_effect = api
        return client, run, jobs

    def test_accepts_verified_prerequisites_at_exact_source(self):
        for component in publication.PREREQUISITES:
            for missing in (False, True):
                for event in ("push", "workflow_dispatch"):
                    with self.subTest(component=component, missing=missing, event=event):
                        client, _, _ = self.fixture(component, missing, event)
                        self.reuse.reset_mock()
                        with patch.object(publication, "checked_source") as source:
                            self.assertEqual(publication.readiness(client, REPOSITORY, COMMIT, component), 42)
                        source.assert_not_called()
                        self.reuse.assert_called_once_with(client, REPOSITORY, COMMIT, 42)

    def test_rejects_current_failures_despite_donor_success(self):
        for name in publication.PREREQUISITES["desktop"]:
            for conclusion in ("failure", "cancelled"):
                with self.subTest(name=name, conclusion=conclusion):
                    client, _, jobs = self.fixture()
                    self.reuse.reset_mock()
                    next(job for job in jobs if job["name"] == name)["conclusion"] = conclusion
                    jobs.append({"name": name, "status": "completed", "conclusion": "success"})
                    self.assertIsNone(publication.ready_run(client, REPOSITORY, COMMIT, "desktop"))
                    self.reuse.assert_not_called()

    def test_does_not_replace_active_prerequisites(self):
        for status in ("queued", "in_progress"):
            with self.subTest(status=status):
                client, _, jobs = self.fixture()
                next(job for job in jobs if job["name"] == "Desktop").update(status=status, conclusion=None)
                self.assertIsNone(publication.ready_run(client, REPOSITORY, COMMIT, "desktop"))

    def test_requires_fresh_source_and_ready(self):
        for name in ("Source", "Desktop ready"):
            for state in ("missing", "skipped", "failure", "cancelled", "in_progress"):
                with self.subTest(name=name, state=state):
                    client, _, jobs = self.fixture()
                    self.reuse.return_value |= {"Source", "Desktop ready"}
                    job = next(job for job in jobs if job["name"] == name)
                    if state == "missing":
                        jobs.remove(job)
                    elif state == "in_progress":
                        job.update(status=state, conclusion=None)
                    else:
                        job["conclusion"] = state
                    self.assertIsNone(publication.ready_run(client, REPOSITORY, COMMIT, "desktop"))

    def test_rejects_absent_or_incomplete_proof(self):
        for verified in (set(), {"Rust strict lint"}):
            with self.subTest(verified=verified):
                client, _, _ = self.fixture()
                self.reuse.return_value = verified
                self.assertIsNone(publication.ready_run(client, REPOSITORY, COMMIT, "desktop"))
                with patch.object(publication.time, "sleep") as sleep, \
                        self.assertRaisesRegex(ValueError, "ended without"):
                    publication.wait_ready(client, REPOSITORY, COMMIT, "desktop")
                sleep.assert_not_called()

    def test_rejects_invalid_or_stale_proof(self):
        for message in ("Invalid Checks proof", "Stale Checks proof"):
            for check in (publication.readiness, publication.wait_ready):
                with self.subTest(message=message, check=check.__name__):
                    client, _, _ = self.fixture()
                    self.reuse.side_effect = ValueError(message)
                    with patch.object(publication, "checked_source") as source, \
                            patch.object(publication.time, "sleep") as sleep, \
                            self.assertRaisesRegex(ValueError, message):
                        check(client, REPOSITORY, COMMIT, "desktop")
                    source.assert_not_called()
                    sleep.assert_not_called()

    def test_waits_for_fresh_gates_with_reuse(self):
        for missing in (False, True):
            with self.subTest(missing=missing):
                client, run, jobs = self.fixture(missing=missing)
                fresh = [job for job in jobs if job["name"] in {"Source", "Desktop ready"}]
                for job in fresh:
                    job.update(status="in_progress", conclusion=None)

                def complete(_):
                    for job in fresh:
                        job.update(status="completed", conclusion="success")
                    run.update(status="completed", conclusion="success")

                with patch.object(publication.time, "sleep", side_effect=complete) as sleep, \
                        patch.object(publication.time, "monotonic", side_effect=[0, 1]), \
                        patch.object(publication, "checked_source") as source:
                    self.assertEqual(publication.wait_ready(client, REPOSITORY, COMMIT, "desktop"), 42)
                sleep.assert_called_once_with(60)
                source.assert_not_called()
                self.reuse.assert_called_with(client, REPOSITORY, COMMIT, 42)

    def test_waiting_rejects_actual_failures_without_delay(self):
        for name in (*publication.PREREQUISITES["desktop"], "Desktop ready"):
            for conclusion in ("failure", "cancelled"):
                with self.subTest(name=name, conclusion=conclusion):
                    client, _, jobs = self.fixture()
                    self.reuse.reset_mock()
                    self.reuse.return_value |= {"Source", "Desktop ready"}
                    next(job for job in jobs if job["name"] == name)["conclusion"] = conclusion
                    with patch.object(publication.time, "sleep") as sleep, \
                            self.assertRaisesRegex(ValueError, "ended without"):
                        publication.wait_ready(client, REPOSITORY, COMMIT, "desktop")
                    sleep.assert_not_called()
                    self.reuse.assert_not_called()

    def test_waiting_rejects_missing_fresh_gates(self):
        for name in ("Source", "Desktop ready"):
            with self.subTest(name=name):
                client, run, jobs = self.fixture()
                run.update(status="completed", conclusion="success")
                jobs.remove(next(job for job in jobs if job["name"] == name))
                self.reuse.return_value |= {"Source", "Desktop ready"}
                with patch.object(publication.time, "sleep") as sleep, \
                        self.assertRaisesRegex(ValueError, "ended without"):
                    publication.wait_ready(client, REPOSITORY, COMMIT, "desktop")
                sleep.assert_not_called()


class PublicationWait(unittest.TestCase):
    """Products built alongside Checks upload only after the version entry publishes."""

    def setUp(self):
        self.runs = [{"id": 42, "head_sha": COMMIT, "head_branch": TAG, "event": "workflow_dispatch",
                      "status": "in_progress", "conclusion": None}]
        self.ready = None
        self.enterContext(patch.object(publication, "ready_run", side_effect=lambda *_, **__: self.ready))
        self.enterContext(patch.object(publication.ci_reuse, "pages", side_effect=lambda *_: copy.deepcopy(self.runs)))
        self.enterContext(patch("builtins.print"))

    def test_waits_for_checks_then_publication(self):
        client = FakeGitHub(metadata(draft=True))
        steps = iter((lambda: setattr(self, "ready", 42),
                      lambda: client.release.update(draft=False, published_at="2026-10-15T00:00:00Z")))
        with patch.object(publication.time, "sleep", side_effect=lambda _: next(steps)()) as sleep, \
                patch.object(publication.time, "monotonic", return_value=0):
            published = publication.wait_published(client, REPOSITORY, TAG, COMMIT)
        self.assertFalse(published["draft"])
        self.assertEqual(sleep.call_count, 2)

    def test_returns_existing_publication_immediately(self):
        with patch.object(publication.time, "sleep") as sleep:
            self.assertEqual(publication.wait_published(FakeGitHub(metadata()), REPOSITORY, TAG, COMMIT)["id"], 12)
        sleep.assert_not_called()

    def test_stops_when_checks_end_without_success(self):
        self.runs[0].update(status="completed", conclusion="failure")
        with patch.object(publication.time, "sleep") as sleep, \
                self.assertRaisesRegex(ValueError, "ended without success"):
            publication.wait_published(FakeGitHub(None), REPOSITORY, TAG, COMMIT)
        sleep.assert_not_called()

    def test_bounds_publication_after_checks_pass(self):
        self.ready = 42
        with patch.object(publication.time, "sleep") as sleep, \
                patch.object(publication.time, "monotonic", side_effect=[0, 0, 60, 1800]), \
                self.assertRaisesRegex(TimeoutError, "not published"):
            publication.wait_published(FakeGitHub(None), REPOSITORY, TAG, COMMIT)
        sleep.assert_called_once_with(60)

    def test_rejects_moved_tag(self):
        with self.assertRaisesRegex(ValueError, "differs"):
            publication.wait_published(FakeGitHub(metadata()), REPOSITORY, TAG, "b" * 40)


if __name__ == "__main__":
    unittest.main()
