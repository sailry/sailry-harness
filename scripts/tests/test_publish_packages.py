"""Layered publishers preserve released tags and existing assets without network writes."""

import copy
import subprocess
import importlib.util
import io
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
        self.uploads.extend(path.name for path in assets)
        self.release["assets"].extend(asset(path) for path in assets)

    def digest(self, repository, existing):
        return existing["fixture_digest"]


def asset(path):
    return {"id": 5, "name": path.name, "state": "uploaded", "size": path.stat().st_size,
            "digest": publisher.digest(path)}


class PackagePublication(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.notes = self.root / "notes.md"
        self.notes.write_text("Release notes")
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
        client = FakeGitHub()
        publisher.publish("desktop-adhoc", TAG, self.notes, initial, REPOSITORY, COMMIT, client)
        self.assertFalse(client.release["draft"])
        self.assertEqual(len(client.uploads), 6)
        self.assertTrue(all("-adhoc" in path.name and TAG[1:] not in path.name for path in initial))
        original = copy.deepcopy(client.release["assets"])
        publisher.publish("desktop", TAG, None, self.desktop, REPOSITORY, COMMIT, client, append_signed=True)
        self.assertEqual(client.release["assets"][:6], original)
        self.assertEqual(len(client.release["assets"]), 14)
        count = len(client.uploads)
        publisher.publish("desktop", TAG, None, self.desktop, REPOSITORY, COMMIT, client, append_signed=True)
        self.assertEqual(len(client.uploads), count)

    def test_preserves_signed_arm_during_initial_publication(self):
        adhoc = self.assets("desktop-adhoc")
        initial = [path for path in adhoc if "x86_64" in path.name] + [
            path for path in self.desktop if "aarch64" in path.name]
        client = FakeGitHub()
        publisher.publish("desktop-adhoc", TAG, self.notes, initial, REPOSITORY, COMMIT, client, signed_arm=True)
        self.assertEqual(len(client.release["assets"]), 7)
        original = copy.deepcopy(client.release["assets"])
        publisher.publish("desktop", TAG, None, self.desktop, REPOSITORY, COMMIT, client, append_signed=True)
        self.assertEqual(client.release["assets"][:7], original)
        self.assertEqual(len(client.release["assets"]), 11)

    def test_host_does_not_wait_for_formal_desktop(self):
        initial = self.assets("desktop-adhoc")
        client = FakeGitHub()
        publisher.publish("desktop-adhoc", TAG, self.notes, initial, REPOSITORY, COMMIT, client)
        self.publish("host", client)
        self.assertEqual(len(client.release["assets"]), 16)

    def test_latest_uses_server_side_version_selection(self):
        self.assertEqual(publisher.latest_flag("v1.0.0"), "legacy")
        self.assertEqual(publisher.latest_flag("v3.0.0"), "legacy")
        self.assertEqual(publisher.latest_flag(TAG), "false")

    def test_host_rejects_incomplete_uploaded_group(self):
        client = FakeGitHub()
        initial = self.assets("desktop-adhoc")
        publisher.publish("desktop-adhoc", TAG, self.notes, initial, REPOSITORY, COMMIT, client)
        client.release["assets"][0]["state"] = "starter"
        with self.assertRaisesRegex(ValueError, "complete published Desktop"):
            self.publish("host", client)

    def publish(self, component, client, assets=None):
        publisher.publish(component, TAG, self.notes, assets or getattr(self, component),
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

    def test_desktop_creates_draft_then_publishes(self):
        client = FakeGitHub()
        self.publish("desktop", client)
        creation = next(call for call in client.calls if call[0] == "POST")
        self.assertEqual(creation[2]["target_commitish"], COMMIT)
        self.assertTrue(creation[2]["draft"])
        self.assertFalse(creation[2]["prerelease"])
        self.assertEqual(len(client.uploads), 8)
        self.assertEqual(client.calls[-1][2], {
            "name": "Sailry Harness 0.1.0-alpha.1", "body": "Release notes",
            "target_commitish": COMMIT, "draft": False, "prerelease": False, "make_latest": "false",
        })

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

    def test_draft_retry_skips_matching_assets(self):
        release = metadata(draft=True)
        release["assets"] = [asset(self.desktop[0])]
        release.update(name="Old candidate", body="Old notes", target_commitish="b" * 40)
        client = FakeGitHub(release)
        self.publish("desktop", client)
        self.assertEqual(len(client.uploads), 7)
        self.assertEqual(client.release["name"], "Sailry Harness 0.1.0-alpha.1")
        self.assertEqual(client.release["body"], "Release notes")
        self.assertEqual(client.release["target_commitish"], COMMIT)

    def test_draft_requires_current_notes_and_supported_assets(self):
        release = metadata(draft=True)
        client = FakeGitHub(release)
        with self.assertRaisesRegex(ValueError, "release notes"):
            publisher.publish("desktop", TAG, None, self.desktop, REPOSITORY, COMMIT, client)
        release["assets"] = [{"name": f"Sailry-{TAG[1:]}-android-arm64.apk"}]
        client = FakeGitHub(release)
        with self.assertRaisesRegex(ValueError, "unsupported draft assets"):
            self.publish("desktop", client)
        self.assertFalse(client.uploads)
        self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_collision_fails_before_any_upload(self):
        for field, value in (("size", 0), ("digest", "sha256:" + "0" * 64), ("state", "open")):
            with self.subTest(field=field):
                release = self.public_desktop()
                existing = asset(self.host[-1])
                existing[field] = value
                release["assets"].append(existing)
                client = FakeGitHub(release)
                with self.assertRaisesRegex(ValueError, "collision"):
                    self.publish("host", client)
                self.assertFalse(client.uploads)
                self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_missing_digest_verifies_existing_bytes(self):
        release = self.public_desktop()
        release["assets"][0]["fixture_digest"] = release["assets"][0].pop("digest")
        self.publish("desktop", FakeGitHub(release))

    def test_host_requires_public_mutable_desktop(self):
        for release in (None, metadata(draft=True), metadata(),
                        {**self.public_desktop(), "immutable": True}):
            with self.subTest(release=release):
                client = FakeGitHub(release)
                with self.assertRaises(ValueError):
                    self.publish("host", client)
                self.assertFalse(client.uploads)
                self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_public_desktop_cannot_acquire_missing_assets(self):
        client = FakeGitHub(metadata())
        with self.assertRaisesRegex(ValueError, "missing Desktop"):
            self.publish("desktop", client)
        self.assertFalse(client.uploads)

    def test_component_allowlists_reject_mixed_or_partial_assets(self):
        for component, files in (("host", self.desktop), ("desktop", self.host),
                                 ("host", self.host[:-1]), ("desktop", self.desktop + self.host)):
            with self.subTest(component=component, count=len(files)):
                with self.assertRaises(ValueError):
                    self.publish(component, FakeGitHub(), files)

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
            publisher.publish("host", TAG, None, self.host, REPOSITORY, "b" * 40, client)
        client.release["tag_name"] = "v0.1.0-alpha.2"
        with self.assertRaisesRegex(ValueError, "tag or channel"):
            self.publish("host", client)
        self.assertFalse(client.uploads)


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
                with self.assertRaisesRegex(ValueError, "Desktop must publish"):
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
    def test_required_jobs_exist(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        names = set(re.findall(r"^    name: (.+)$", workflow, re.MULTILINE))
        for component, prerequisites in publication.PREREQUISITES.items():
            with self.subTest(component=component):
                self.assertLessEqual(set(prerequisites) | {f"{component.title()} ready"}, names)

    def run_fixture(self, gate="success", event="push", branch="main", sha=COMMIT, overall="failure"):
        client = Mock()
        client.api.side_effect = [
            {"workflow_runs": [{"id": 42, "head_sha": sha, "head_branch": branch,
                                "event": event, "conclusion": overall}]},
            {"jobs": [{"name": name, "status": "completed", "conclusion": "success"}
                      for name in ("Source", "Shared Node", "Desktop")] +
                     [{"name": "Desktop ready", "status": "completed", "conclusion": gate}]},
        ]
        return client

    def test_exact_checks_take_precedence(self):
        with patch.object(publication, "checked_source") as source:
            self.assertEqual(publication.readiness(self.run_fixture(), REPOSITORY, COMMIT, "desktop"), 42)
        source.assert_not_called()

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

    def test_requires_actual_prerequisites_with_success_gate(self):
        for name in ("Source", "Shared Node", "Desktop"):
            for conclusion in ("failure", "skipped"):
                client = self.run_fixture()
                runs, jobs = list(client.api.side_effect)
                for job in jobs["jobs"]:
                    if job["name"] == name:
                        job["conclusion"] = conclusion
                client.api.side_effect = [runs, jobs]
                with self.subTest(name=name, conclusion=conclusion), self.assertRaises(ValueError):
                    publication.readiness(client, REPOSITORY, COMMIT, "desktop")

    def test_tag_must_match_checkout_and_product(self):
        with patch.object(publication.versions, "product", return_value=(TAG[1:], 1)), \
                patch.object(publication.subprocess, "check_output", side_effect=[COMMIT, "b" * 40]):
            with self.assertRaisesRegex(ValueError, "exact commit"):
                publication.tagged_commit(TAG)
        with patch.object(publication.versions, "product", return_value=(TAG[1:], 1)):
            with self.assertRaisesRegex(ValueError, "application version"):
                publication.tagged_commit("v0.1.0-alpha.2")


if __name__ == "__main__":
    unittest.main()
