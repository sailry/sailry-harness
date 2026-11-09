"""Mobile release fixtures isolate admission, signing metadata and private staging."""

import base64
import copy
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/package"))


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts/package" / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


release = load("mobile_release", "mobile-release.py")
signing = load("ios_signing", "ios-signing.py")
REVISION = "a" * 40
VERSION = "0.1.0-alpha.9"
TEAM = "TEAM123456"
IDENTIFIER = "70d562d5-f663-449f-bf22-567464d47498"
NOW = datetime.datetime(2026, 11, 1, tzinfo=datetime.timezone.utc)
CERTIFICATE = b"fixture distribution certificate"
FINGERPRINT = hashlib.sha1(CERTIFICATE).hexdigest().upper()


def profile():
    return {"UUID": IDENTIFIER, "TeamIdentifier": [TEAM], "Platform": ["iOS"],
            "ExpirationDate": datetime.datetime(2027, 11, 1), "DeveloperCertificates": [CERTIFICATE],
            "Entitlements": {"application-identifier": f"{TEAM}.{signing.BUNDLE}",
                             "com.apple.developer.team-identifier": TEAM, "get-task-allow": False}}


def project():
    return {"objects": {
        "app": {"isa": "PBXNativeTarget", "name": "Runner", "productType": "com.apple.product-type.application",
                "buildConfigurationList": "app-list"},
        "app-list": {"buildConfigurations": ["debug", "release"]},
        "debug": {"name": "Debug", "buildSettings": {"PRODUCT_BUNDLE_IDENTIFIER": signing.BUNDLE}},
        "release": {"name": "Release", "buildSettings": {"PRODUCT_BUNDLE_IDENTIFIER": signing.BUNDLE}},
        "tests": {"isa": "PBXNativeTarget", "name": "RunnerTests", "productType": "com.apple.product-type.bundle.unit-test",
                  "buildConfigurationList": "test-list"},
        "test-list": {"buildConfigurations": ["test-release"]},
        "test-release": {"name": "Release", "buildSettings": {"CODE_SIGN_STYLE": "Automatic"}},
    }}


def git(root, *arguments):
    return subprocess.run(["git", "-C", str(root), *arguments], check=True, capture_output=True, text=True).stdout.strip()


def write_versions(root, version, code):
    for app in ("desktop", "host", "mobile"):
        (root / "apps" / app).mkdir(parents=True, exist_ok=True)
    for app in ("desktop", "host"):
        (root / "apps" / app / "Cargo.toml").write_text(f'[package]\nversion = "{version}"\n')
    (root / "apps/mobile/pubspec.yaml").write_text(f"version: {version}+{code}\n")


class Repository:
    def __init__(self, directory):
        self.tooling = directory / "tooling"
        self.tooling.mkdir()
        git(self.tooling, "init", "--initial-branch=main", "--template=")
        git(self.tooling, "config", "user.name", "Fixture")
        git(self.tooling, "config", "user.email", "fixture@example.invalid")
        git(self.tooling, "config", "commit.gpgsign", "false")
        write_versions(self.tooling, VERSION, 9)
        project_path = signing.runner_project(self.tooling)
        project_path.parent.mkdir(parents=True)
        project_path.write_bytes(plistlib.dumps(project()))
        git(self.tooling, "add", ".")
        git(self.tooling, "commit", "-m", "Checked product")
        self.revision = git(self.tooling, "rev-parse", "HEAD")
        self.source = directory / "product"
        git(self.tooling, "worktree", "add", "--detach", str(self.source), self.revision)
        write_versions(self.tooling, "0.2.0-alpha.1", 10)
        git(self.tooling, "commit", "-a", "-m", "New workflow tooling")
        self.workflow = git(self.tooling, "rev-parse", "HEAD")
        self.environment = {"GITHUB_SHA": self.workflow, "WORKFLOW_REVISION": self.workflow,
                            "SOURCE_COMMIT": self.revision, "GITHUB_EVENT_NAME": "workflow_dispatch",
                            "GITHUB_REF": "refs/heads/main", "GITHUB_REPOSITORY": "fixture/product",
                            "GITHUB_OUTPUT": str(directory / "output")}


class Checks:
    def __init__(self, revision, conclusion="success", gate="success"):
        self.run = {"id": 41, "head_sha": revision, "head_branch": "main", "event": "push",
                    "status": "completed", "conclusion": conclusion}
        self.gate = gate
        self.remote_revision = revision
        self.published = {"id": 7, "tag_name": f"v{VERSION}", "draft": False, "prerelease": False,
                          "published_at": "2026-11-01T00:00:00Z", "assets": []}
        self.uploads = []

    def api(self, method, endpoint, payload=None, missing=False):
        if method != "GET":
            raise AssertionError("Mobile packaging must not create or edit releases")
        if "/actions/workflows/ci.yml/runs?" in endpoint:
            if f"head_sha={self.run['head_sha']}&" not in endpoint:
                raise AssertionError("Checks must be queried for the selected product source")
            return {"workflow_runs": [self.run]}
        if "/actions/runs/41/jobs?" in endpoint:
            return {"jobs": [{"name": name, "status": "completed",
                              "conclusion": self.gate if name == "Mobile ready" else "success"}
                             for name in (*release.publication.PREREQUISITES["mobile"], "Mobile ready")]}
        if endpoint.endswith("/actions/runs/41"):
            return self.run
        if "/releases/tags/" in endpoint:
            return self.published
        if "/releases?" in endpoint:
            return [self.published] if self.published else []
        if "/commits/" in endpoint:
            return {"sha": self.remote_revision}
        raise AssertionError(f"Unexpected Checks API request: {method} {endpoint}")

    def upload(self, tag, assets):
        names = {path.name for path in assets}
        self.uploads.append((tag, names))
        self.published["assets"] = [existing for existing in self.published["assets"]
                                    if existing["name"] not in names]
        self.published["assets"].extend({"name": path.name, "state": "uploaded", "size": path.stat().st_size,
                                         "digest": "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()}
                                        for path in assets)


class Admission(unittest.TestCase):
    def test_accepts_main_dispatch_and_matching_tags(self):
        base = {"GITHUB_SHA": REVISION, "WORKFLOW_REVISION": REVISION,
                "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main"}
        release.context(VERSION, REVISION, base)
        release.context(VERSION, REVISION, {**base, "GITHUB_REF": f"refs/tags/v{VERSION}"})
        release.context(VERSION, "b" * 40, {**base, "RELEASE_TAG": f"v{VERSION}"})

    def test_rejects_other_branches_sources_and_versions(self):
        base = {"GITHUB_SHA": REVISION, "WORKFLOW_REVISION": REVISION,
                "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main"}
        for change in ({"GITHUB_REF": "refs/heads/topic"}, {"GITHUB_REF": "refs/tags/v0.1.0-alpha.8"},
                       {"GITHUB_EVENT_NAME": "pull_request"}, {"GITHUB_EVENT_NAME": "push"},
                       {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": f"refs/tags/v{VERSION}"},
                       {"GITHUB_SHA": "b" * 40}, {"GITHUB_SHA": ""}, {"SOURCE_COMMIT": "main"},
                       {"SOURCE_COMMIT": REVISION[:8]}, {"WORKFLOW_REVISION": "b" * 40}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                release.context(VERSION, REVISION, {**base, **change})

    def test_rejects_ambiguous_or_untrusted_tag_inputs(self):
        base = {"GITHUB_SHA": REVISION, "WORKFLOW_REVISION": REVISION, "RELEASE_TAG": f"v{VERSION}",
                "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main"}
        for change in ({"SOURCE_COMMIT": REVISION}, {"GITHUB_REF": "refs/heads/topic"},
                       {"GITHUB_REF": f"refs/tags/v{VERSION}"}, {"RELEASE_TAG": "main"},
                       {"RELEASE_TAG": "v0.1.0-alpha.8"}, {"GITHUB_EVENT_NAME": "push"}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                release.context(VERSION, REVISION, {**base, **change})

    def test_rejects_source_overrides_on_version_tags(self):
        for event in ("push", "workflow_dispatch"):
            with self.subTest(event=event), self.assertRaises(ValueError):
                release.context(VERSION, REVISION, {"GITHUB_SHA": REVISION, "WORKFLOW_REVISION": REVISION,
                                                   "GITHUB_EVENT_NAME": event, "GITHUB_REF": f"refs/tags/v{VERSION}",
                                                   "SOURCE_COMMIT": REVISION})

    def test_admits_checked_ancestor_using_newer_tooling(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Repository(Path(temporary))
            with patch.dict(os.environ, fixture.environment, clear=True), \
                    patch.object(release.publication, "GitHub", return_value=Checks(fixture.revision)):
                release.admit(fixture.source, fixture.tooling)
            values = dict(row.split("=", 1) for row in Path(fixture.environment["GITHUB_OUTPUT"]).read_text().splitlines())
            self.assertEqual(values["revision"], fixture.revision)
            self.assertEqual(values["workflow_revision"], fixture.workflow)
            self.assertNotEqual(fixture.revision, fixture.workflow)
            self.assertEqual((values["version"], values["apple_version"], values["build_number"]), (VERSION, "0.1.0", "9"))

    def test_requires_the_exact_product_tag_commit(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Repository(Path(temporary))
            git(fixture.tooling, "switch", "--detach", fixture.revision)
            git(fixture.tooling, "tag", f"v{VERSION}", fixture.revision)
            environment = {**fixture.environment, "GITHUB_SHA": fixture.revision, "WORKFLOW_REVISION": fixture.revision,
                           "SOURCE_COMMIT": "", "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": f"refs/tags/v{VERSION}"}
            with patch.dict(os.environ, environment, clear=True), \
                    patch.object(release.publication, "GitHub", return_value=Checks(fixture.revision)):
                release.admit(fixture.source, fixture.tooling)
                git(fixture.tooling, "tag", "-f", f"v{VERSION}", fixture.workflow)
                with self.assertRaisesRegex(ValueError, "exact commit"):
                    release.admit(fixture.source, fixture.tooling)

    def test_requires_published_tags_and_product_checks(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Repository(Path(temporary))
            git(fixture.tooling, "tag", f"v{VERSION}", fixture.revision)
            environment = {**fixture.environment, "SOURCE_COMMIT": "", "RELEASE_TAG": f"v{VERSION}"}
            for failure in ("checks", "missing", "draft", "moved"):
                client = Checks(fixture.revision, gate="failure" if failure == "checks" else "success")
                if failure == "missing":
                    client.published = None
                elif failure == "draft":
                    client.published.update(draft=True, published_at=None)
                elif failure == "moved":
                    client.remote_revision = fixture.workflow
                with self.subTest(failure=failure), patch.dict(os.environ, environment, clear=True), \
                        patch.object(release.publication, "GitHub", return_value=client), self.assertRaises(ValueError):
                    release.admit(fixture.source, fixture.tooling)
                self.assertFalse(Path(environment["GITHUB_OUTPUT"]).exists())
                self.assertFalse(client.uploads)

    def test_admits_published_tags_with_newer_workflow_tooling(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Repository(Path(temporary))
            git(fixture.tooling, "tag", f"v{VERSION}", fixture.revision)
            environment = {**fixture.environment, "SOURCE_COMMIT": "", "RELEASE_TAG": f"v{VERSION}"}
            with patch.dict(os.environ, environment, clear=True), \
                    patch.object(release.publication, "GitHub", return_value=Checks(fixture.revision)):
                release.admit(fixture.source, fixture.tooling)
            values = dict(row.split("=", 1) for row in Path(environment["GITHUB_OUTPUT"]).read_text().splitlines())
            self.assertEqual(values["tag"], f"v{VERSION}")
            self.assertEqual(values["revision"], fixture.revision)
            self.assertEqual(values["workflow_revision"], fixture.workflow)

    def test_admits_exact_tag_checks_from_version_entry(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Repository(Path(temporary))
            tag = f"v{VERSION}"
            git(fixture.tooling, "tag", tag, fixture.revision)
            environment = {**fixture.environment, "SOURCE_COMMIT": "", "RELEASE_TAG": tag}
            client = Checks(fixture.revision)
            client.run.update(head_branch=tag, event="workflow_dispatch")
            with patch.dict(os.environ, environment, clear=True), \
                    patch.object(release.publication, "GitHub", return_value=client):
                release.admit(fixture.source, fixture.tooling)
            values = dict(row.split("=", 1) for row in Path(environment["GITHUB_OUTPUT"]).read_text().splitlines())
            self.assertEqual(values["revision"], fixture.revision)
            self.assertEqual(values["tag"], tag)

    def test_admits_despite_unrelated_check_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Repository(Path(temporary))
            with patch.dict(os.environ, fixture.environment, clear=True), \
                    patch.object(release.publication, "GitHub", return_value=Checks(fixture.revision, "failure")):
                release.admit(fixture.source, fixture.tooling)
            values = dict(row.split("=", 1) for row in Path(fixture.environment["GITHUB_OUTPUT"]).read_text().splitlines())
            self.assertEqual(values["revision"], fixture.revision)

    def test_rejects_mismatched_checkouts_and_off_main_sources(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Repository(Path(temporary))
            for change in ({"SOURCE_COMMIT": fixture.workflow}, {"SOURCE_COMMIT": ""},
                           {"GITHUB_REF": "refs/heads/topic"},
                           {"GITHUB_SHA": fixture.revision, "WORKFLOW_REVISION": fixture.revision}):
                with self.subTest(change=change), self.assertRaises(ValueError):
                    release.candidate(fixture.source, fixture.tooling, {**fixture.environment, **change})
            git(fixture.source, "switch", "-c", "topic")
            (fixture.source / "topic").write_text("Unmerged source\n")
            git(fixture.source, "add", "topic")
            git(fixture.source, "commit", "-m", "Topic source")
            revision = git(fixture.source, "rev-parse", "HEAD")
            with self.assertRaisesRegex(ValueError, "ancestor"):
                release.candidate(fixture.source, fixture.tooling, {**fixture.environment, "SOURCE_COMMIT": revision})

    def test_checks_before_emitting_distribution_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Repository(Path(temporary))
            with patch.dict(os.environ, fixture.environment, clear=True), \
                    patch.object(release.publication, "GitHub", return_value=Checks(fixture.revision, gate="failure")):
                with self.assertRaisesRegex(ValueError, "successful Mobile readiness"):
                    release.admit(fixture.source, fixture.tooling)
            self.assertFalse(Path(fixture.environment["GITHUB_OUTPUT"]).exists())


class Profiles(unittest.TestCase):
    def test_accepts_only_matching_live_app_store_profiles(self):
        self.assertEqual(signing.profile_details(profile(), TEAM, NOW), (IDENTIFIER, {FINGERPRINT}))
        changes = ({"ExpirationDate": NOW.replace(tzinfo=None)}, {"TeamIdentifier": ["OTHERTEAM"]},
                   {"Platform": ["OSX"]}, {"ProvisionedDevices": ["fixture-device"]},
                   {"ProvisionsAllDevices": True}, {"DeveloperCertificates": []}, {"UUID": "../profile"})
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                signing.profile_details({**profile(), **change}, TEAM, NOW)

    def test_rejects_development_and_other_app_entitlements(self):
        for change in ({"get-task-allow": True}, {"application-identifier": f"{TEAM}.com.codux.mobile"},
                       {"application-identifier": f"{TEAM}.*"}, {"com.apple.developer.team-identifier": "OTHERTEAM"}):
            value = profile()
            value["Entitlements"].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                signing.profile_details(value, TEAM, NOW)


class ProjectSigning(unittest.TestCase):
    def test_changes_only_the_application_release_target(self):
        value = project()
        original = copy.deepcopy(value)
        signing.configure_project(value, TEAM, FINGERPRINT, IDENTIFIER)
        changed = value["objects"]["release"]["buildSettings"]
        self.assertEqual(changed["CODE_SIGN_STYLE"], "Manual")
        self.assertEqual(changed["PROVISIONING_PROFILE_SPECIFIER"], IDENTIFIER)
        self.assertEqual(changed["CODE_SIGN_IDENTITY"], FINGERPRINT)
        self.assertEqual(changed["DEVELOPMENT_TEAM"], TEAM)
        value["objects"]["release"] = original["objects"]["release"]
        self.assertEqual(value, original)

    def test_rejects_a_different_application_target(self):
        value = project()
        value["objects"]["release"]["buildSettings"]["PRODUCT_BUNDLE_IDENTIFIER"] = "com.codux.mobile"
        with self.assertRaises(ValueError):
            signing.configure_project(value, TEAM, FINGERPRINT, IDENTIFIER)

    def test_configures_and_restores_the_selected_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture = Repository(directory)
            selected = signing.runner_project(fixture.source)
            tooling = signing.runner_project(fixture.tooling)
            original, untouched = selected.read_bytes(), tooling.read_bytes()
            environment = {"RUNNER_TEMP": temporary, "GITHUB_ENV": str(directory / "environment"), "APPLE_TEAM_ID": TEAM,
                           "IOS_CERTIFICATE_P12": base64.b64encode(b"private fixture").decode(),
                           "IOS_CERTIFICATE_PASSWORD": "fixture-secret", "IOS_PROVISIONING_PROFILE": base64.b64encode(b"profile").decode()}
            def execute(program, *arguments):
                if program == "plutil":
                    return Path(arguments[-1]).read_bytes()
                if arguments[:3] == ("cms", "-D", "-i"):
                    return plistlib.dumps(profile())
                if arguments[:2] == ("list-keychains", "-d"):
                    return b'"/fixture/login.keychain-db"\n'
                if arguments[0] == "find-identity":
                    return f'1) {FINGERPRINT} "Apple Distribution: Fixture"\n'.encode()
                return b""
            with patch.dict(os.environ, environment, clear=True), patch.object(signing, "PROJECT", tooling), \
                    patch.object(signing, "run", side_effect=execute), \
                    patch.object(signing, "profiles", return_value=directory / "profiles"), \
                    patch.object(signing.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
                signing.install(fixture.source)
                configured = plistlib.loads(selected.read_bytes())
                self.assertEqual(configured["objects"]["release"]["buildSettings"]["CODE_SIGN_STYLE"], "Manual")
                self.assertEqual(tooling.read_bytes(), untouched)
                signing.cleanup(fixture.source)
            self.assertEqual(selected.read_bytes(), original)
            self.assertEqual(tooling.read_bytes(), untouched)
            self.assertFalse((directory / "sailry-ios").exists())


class SigningCleanup(unittest.TestCase):
    def test_failed_import_removes_private_staging(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            original = b"original project"
            source = directory / "project.pbxproj"
            source.write_bytes(original)
            environment = {"RUNNER_TEMP": temporary, "GITHUB_ENV": str(directory / "environment"), "APPLE_TEAM_ID": TEAM,
                           "IOS_CERTIFICATE_P12": base64.b64encode(b"private fixture").decode(),
                           "IOS_CERTIFICATE_PASSWORD": "fixture-secret", "IOS_PROVISIONING_PROFILE": base64.b64encode(b"profile").decode()}
            def execute(program, *arguments):
                if arguments[:3] == ("cms", "-D", "-i"):
                    return plistlib.dumps(profile())
                if arguments[:2] == ("list-keychains", "-d"):
                    return b'"/fixture/login.keychain-db"\n'
                if arguments[0] == "import":
                    raise RuntimeError("Import failed")
                return b""
            with patch.dict(os.environ, environment, clear=True), patch.object(signing, "PROJECT", source), \
                    patch.object(signing, "run", side_effect=execute), \
                    patch.object(signing.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
                with self.assertRaisesRegex(RuntimeError, "Import failed"):
                    signing.install()
            self.assertEqual(source.read_bytes(), original)
            self.assertFalse((directory / "sailry-ios").exists())
            self.assertFalse((directory / "environment").exists())

    def test_restores_project_and_preserves_preexisting_profile(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            state = directory / "sailry-ios"
            state.mkdir()
            (state / "project.pbxproj").write_bytes(b"original project")
            source = directory / "project.pbxproj"
            source.write_bytes(b"configured project")
            installed = directory / "profiles"
            installed.mkdir()
            preserved = installed / f"{IDENTIFIER}.mobileprovision"
            preserved.write_bytes(b"existing profile")
            with patch.dict(os.environ, {"RUNNER_TEMP": temporary}, clear=True), \
                    patch.object(signing, "PROJECT", source), patch.object(signing, "profiles", return_value=installed):
                signing.cleanup()
                signing.cleanup()
            self.assertEqual(source.read_bytes(), b"original project")
            self.assertEqual(preserved.read_bytes(), b"existing profile")
            self.assertFalse(state.exists())

    def test_hides_passwords_on_security_failure(self):
        failure = subprocess.CompletedProcess([], 1, b"", b"fixture-secret")
        with patch.object(signing.subprocess, "run", return_value=failure), self.assertRaises(RuntimeError) as outcome:
            signing.run("security", "import", "certificate", "-P", "fixture-secret")
        self.assertNotIn("fixture-secret", str(outcome.exception))


class Artifacts(unittest.TestCase):
    def test_verifies_selected_ios_versions_with_newer_tooling(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source, tooling = directory / "product", directory / "tooling"
            write_versions(source, VERSION, 9)
            write_versions(tooling, "0.2.0-alpha.1", 10)
            archive = directory / "input.ipa"
            archive.write_bytes(b"signed IPA fixture")
            (directory / "sailry-ios").mkdir()
            def execute(program, *arguments):
                if program == "ditto":
                    app = Path(arguments[-1]) / "Payload/Sailry.app"
                    app.mkdir(parents=True)
                    (app / "Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": signing.BUNDLE,
                                                                   "CFBundleShortVersionString": "0.1.0", "CFBundleVersion": "9"}))
                elif program == "security":
                    return plistlib.dumps(profile())
                elif arguments[0] == "--display":
                    prefix = arguments[1].removeprefix("--extract-certificates=")
                    Path(f"{prefix}0").write_bytes(CERTIFICATE)
                return b""
            environment = {"RUNNER_TEMP": temporary, "GITHUB_ENV": str(directory / "environment"), "APPLE_TEAM_ID": TEAM,
                           "IOS_SIGNING_IDENTITY": FINGERPRINT, "IOS_PROFILE_UUID": IDENTIFIER}
            with patch.dict(os.environ, environment, clear=True), patch.object(signing.versions, "ROOT", tooling), \
                    patch.object(signing, "run", side_effect=execute):
                signing.verify(archive, source)
            self.assertEqual((directory / "environment").read_text(), f"IOS_IPA_PATH={archive.resolve()}\n")

    def test_preserves_bytes_and_exact_source_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "input.apk"
            source.write_bytes(b"signed package fixture")
            fixture = Repository(directory)
            output = directory / "packages"
            with patch.dict(os.environ, fixture.environment, clear=True):
                destination = release.package(source, output, "android", fixture.source, fixture.tooling)
            self.assertEqual(destination.read_bytes(), source.read_bytes())
            metadata = json.loads(destination.with_suffix(".json").read_text())
            self.assertEqual(metadata["source_revision"], fixture.revision)
            self.assertEqual(metadata["workflow_revision"], fixture.workflow)
            self.assertEqual(metadata["version"], VERSION)
            self.assertEqual(metadata["build_number"], 9)
            for row in destination.with_suffix(".sha256").read_text().splitlines():
                digest, filename = row.split("  ")
                self.assertEqual(hashlib.sha256((destination.parent / filename).read_bytes()).hexdigest(), digest)
            with patch.dict(os.environ, fixture.environment, clear=True), self.assertRaises(ValueError):
                release.package(source, destination.parent, "android", fixture.source, fixture.tooling)


class Publishing(unittest.TestCase):
    def test_downloads_into_verification_directory(self):
        workflow = (ROOT / ".github/workflows/release-mobile.yml").read_text()
        for platform in ("android", "ios"):
            job = workflow.split(f"  publish-{platform}:\n", 1)[1]
            download = job.split("      - uses: actions/download-artifact@", 1)[1]
            arguments = download.split("      - name:", 1)[0]
            self.assertIn(f"artifact-ids: ${{{{ needs.{platform}.outputs.artifact_id }}}}", arguments)
            self.assertIn(f"path: packages/mobile-{platform}", arguments)
            # ID downloads otherwise nest files under the artifact's name.
            self.assertIn("merge-multiple: true", arguments)

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.fixture = Repository(self.directory)
        git(self.fixture.tooling, "tag", f"v{VERSION}", self.fixture.revision)
        environment = {**self.fixture.environment, "SOURCE_COMMIT": "", "RELEASE_TAG": f"v{VERSION}"}
        variables = patch.dict(os.environ, environment, clear=True)
        variables.start()
        self.addCleanup(variables.stop)
        self.client = Checks(self.fixture.revision)

    def stage(self, platform):
        path = self.directory / ("input.apk" if platform == "android" else "input.ipa")
        path.write_bytes(f"Signed {platform} fixture".encode())
        return release.package(path, self.directory / platform, platform, self.fixture.source, self.fixture.tooling)

    def append(self, platform):
        release.publish(self.directory / platform, platform, self.fixture.source, self.fixture.tooling, self.client)

    def test_appends_platforms_independently_without_filename_collisions(self):
        android = self.stage("android")
        ios = self.stage("ios")
        self.append("android")
        android_names = {path.name for path in android.parent.iterdir()}
        self.assertEqual(self.client.uploads, [(f"v{VERSION}", android_names)])
        self.append("ios")
        ios_names = {path.name for path in ios.parent.iterdir()}
        self.assertEqual(self.client.uploads[-1], (f"v{VERSION}", ios_names))
        self.assertFalse(android_names & ios_names)
        self.assertEqual(android_names, {"Sailry-android-arm64.apk", "Sailry-android-arm64.json",
                                         "Sailry-android-arm64.sha256"})
        self.assertEqual(ios_names, {"Sailry-ios.ipa", "Sailry-ios.json", "Sailry-ios.sha256"})
        self.assertNotIn("release.json", android_names | ios_names)
        self.assertNotIn("SHA256SUMS", android_names | ios_names)

    def test_replaces_changed_assets_and_skips_identical_retries(self):
        for platform in ("android", "ios"):
            with self.subTest(platform=platform):
                archive = self.stage(platform)
                self.client = Checks(self.fixture.revision)
                self.append(platform)
                self.append(platform)
                self.assertEqual(len(self.client.uploads), 1)
                existing = next(asset for asset in self.client.published["assets"] if asset["name"] == archive.name)
                expected_digest = existing["digest"]
                existing["digest"] = "sha256:" + "0" * 64
                self.append(platform)
                self.assertEqual(self.client.uploads[-1], (f"v{VERSION}", {archive.name}))
                self.assertEqual(len(self.client.uploads), 2)
                self.assertEqual(len(self.client.published["assets"]), 3)
                replaced = next(asset for asset in self.client.published["assets"] if asset["name"] == archive.name)
                self.assertEqual(replaced["digest"], expected_digest)
                self.append(platform)
                self.assertEqual(len(self.client.uploads), 2)

    def test_rejects_provenance_from_another_build(self):
        archive = self.stage("android")
        metadata = archive.with_suffix(".json")
        original = json.loads(metadata.read_text())
        for key, value in (("source_revision", "0" * 40), ("workflow_revision", "f" * 40),
                           ("version", "0.2.0-alpha.1"), ("build_number", 10), ("tag", "v0.2.0-alpha.1"),
                           ("platform", "ios"), ("asset", "unrelated.apk")):
            metadata.write_text(json.dumps({**original, key: value}))
            archive.with_suffix(".sha256").write_text(release.checksums((archive, metadata)))
            with self.subTest(field=key), self.assertRaisesRegex(ValueError, "provenance"):
                self.append("android")
        self.assertFalse(self.client.uploads)

    def test_rejects_changed_incomplete_or_extra_files(self):
        archive = self.stage("android")
        original = archive.read_bytes()
        archive.write_bytes(b"Changed package bytes")
        with self.assertRaisesRegex(ValueError, "checksums"):
            self.append("android")
        archive.write_bytes(original)
        checksum = archive.with_suffix(".sha256")
        checksum_bytes = checksum.read_bytes()
        checksum.unlink()
        with self.assertRaisesRegex(ValueError, "complete"):
            self.append("android")
        checksum.write_bytes(checksum_bytes)
        (archive.parent / "unexpected.json").write_text("{}")
        with self.assertRaisesRegex(ValueError, "complete"):
            self.append("android")
        self.assertFalse(self.client.uploads)

    def test_requires_a_published_mutable_matching_release(self):
        self.stage("android")
        for failure in ("missing", "draft", "immutable", "moved"):
            self.client = Checks(self.fixture.revision)
            if failure == "missing":
                self.client.published = None
            elif failure == "draft":
                self.client.published.update(draft=True, published_at=None)
            elif failure == "immutable":
                self.client.published["immutable"] = True
            else:
                self.client.remote_revision = self.fixture.workflow
            with self.subTest(failure=failure), self.assertRaises(ValueError):
                self.append("android")
            self.assertFalse(self.client.uploads)

    def test_preserves_source_builds_without_release_publication(self):
        with patch.dict(os.environ, self.fixture.environment, clear=True):
            archive = self.stage("android")
            self.assertEqual(json.loads(archive.with_suffix(".json").read_text())["tag"], "")
            with self.assertRaisesRegex(ValueError, "Only tagged"):
                self.append("android")
        self.assertTrue(archive.is_file())
        self.assertFalse(self.client.uploads)


if __name__ == "__main__":
    unittest.main()
