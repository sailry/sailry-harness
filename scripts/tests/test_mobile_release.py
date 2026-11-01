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


class Admission(unittest.TestCase):
    def test_accepts_main_dispatch_and_matching_tags(self):
        base = {"GITHUB_SHA": REVISION, "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main"}
        release.context(VERSION, REVISION, base)
        for event in ("push", "workflow_dispatch"):
            release.context(VERSION, REVISION, {**base, "GITHUB_EVENT_NAME": event, "GITHUB_REF": f"refs/tags/v{VERSION}"})

    def test_rejects_other_branches_sources_and_versions(self):
        base = {"GITHUB_SHA": REVISION, "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main"}
        for change in ({"GITHUB_REF": "refs/heads/topic"}, {"GITHUB_REF": "refs/tags/v0.1.0-alpha.8"},
                       {"GITHUB_EVENT_NAME": "pull_request"}, {"GITHUB_EVENT_NAME": "push"},
                       {"GITHUB_SHA": "b" * 40}, {"GITHUB_SHA": ""}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                release.context(VERSION, REVISION, {**base, **change})

    def test_checks_before_emitting_distribution_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            environment = {"GITHUB_SHA": REVISION, "GITHUB_EVENT_NAME": "workflow_dispatch",
                           "GITHUB_REF": "refs/heads/main", "GITHUB_REPOSITORY": "fixture/product", "GITHUB_OUTPUT": str(output)}
            with patch.dict(os.environ, environment, clear=True), \
                    patch.object(release.versions, "product", return_value=(VERSION, 9)), \
                    patch.object(release.subprocess, "check_output", return_value=REVISION), \
                    patch.object(release.publication, "wait_ready", side_effect=ValueError("Checks failed")):
                with self.assertRaises(ValueError):
                    release.admit()
            self.assertFalse(output.exists())


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
    def test_preserves_bytes_and_exact_source_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "input.apk"
            source.write_bytes(b"signed package fixture")
            destination = release.package(source, directory / "output", "android", VERSION, 9, REVISION)
            self.assertEqual(destination.read_bytes(), source.read_bytes())
            metadata = json.loads((destination.parent / "release.json").read_text())
            self.assertEqual(metadata["source_revision"], REVISION)
            self.assertEqual(metadata["version"], VERSION)
            self.assertEqual(metadata["build_number"], 9)
            for row in (destination.parent / "SHA256SUMS").read_text().splitlines():
                digest, filename = row.split("  ")
                self.assertEqual(hashlib.sha256((destination.parent / filename).read_bytes()).hexdigest(), digest)
            with self.assertRaises(ValueError):
                release.package(source, destination.parent, "android", VERSION, 9, REVISION)


if __name__ == "__main__":
    unittest.main()
