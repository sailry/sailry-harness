"""Published signed catalogs belong to exactly one immutable version and its canonical assets."""

import importlib.util
import hashlib
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "publish_update_feed", ROOT / "scripts/publish-update-feed.py"
)
publisher = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = publisher
spec.loader.exec_module(publisher)
import publication


def event(version="1.0.0", draft=False, prerelease=False, action="published"):
    return {
        "action": action,
        "repository": {"full_name": "sailry/sailry-harness"},
        "release": {
            "draft": draft,
            "prerelease": prerelease,
            "tag_name": f"v{version}",
        },
    }


class ReleaseOwnership(unittest.TestCase):
    def test_accepts_verified_dispatch_event(self):
        release = event("0.1.0-alpha.1", prerelease=True)["release"]
        dispatched = publication.event(release, "sailry/sailry-harness")
        result = publisher.publication(dispatched, "sailry/sailry-harness")
        self.assertEqual(result.tag, "v0.1.0-alpha.1")
        self.assertTrue(result.preview)

    def test_accepts_matching_stable_and_preview(self):
        stable = publisher.publication(event(), "sailry/sailry-harness")
        preview = publisher.publication(
            event("1.1.0-alpha.1", prerelease=True), "sailry/sailry-harness"
        )
        self.assertFalse(stable.preview)
        self.assertTrue(preview.preview)

    def test_ordinary_preview_keeps_experience_channel(self):
        result = publisher.publication(event("0.1.0-alpha.3", prerelease=False), "sailry/sailry-harness")
        self.assertEqual(result.tag, "v0.1.0-alpha.3")
        self.assertTrue(result.preview)

    def test_rejects_draft_wrong_event_channel_or_repository(self):
        fixtures = [
            event(draft=True),
            event(action="edited"),
            event("1.0.0", prerelease=True),
            {**event(), "repository": {"full_name": "other/project"}},
        ]
        for fixture in fixtures:
            with self.subTest(fixture=fixture), self.assertRaises(ValueError):
                publisher.publication(fixture, "sailry/sailry-harness")


class Client:
    pass


class ChildEnvironment(unittest.TestCase):
    def test_scopes_signing_key_to_producer(self):
        completed = publisher.subprocess.CompletedProcess([], 0, stdout="{}")
        with patch.dict(publisher.os.environ, {publisher.SIGNING_KEY:"inherited-fixture"}), \
                patch.object(publisher.subprocess,"run",return_value=completed) as run:
            publisher.GitHub().api("GET","repos/sailry/sailry-harness")
            self.assertNotIn(publisher.SIGNING_KEY,run.call_args.kwargs["env"])
            publisher.merge(Path("release-tool"),Path("keys"),Path("assets"),"sailry/sailry-harness",
                publisher.Publication("v1.0.0","1.0.0",False),"stable",Path("output"),"producer-fixture")
            command = run.call_args.args[0]
            self.assertNotIn("--feed",command)
            self.assertEqual(command[command.index("--tag")+1],"v1.0.0")
            self.assertEqual(run.call_args.kwargs["env"][publisher.SIGNING_KEY],"producer-fixture")


class SignedDownloads(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.arguments = SimpleNamespace(
            source=self.root, assets=self.root / "assets", event=self.root / "event.json",
            repository="sailry/sailry-harness",
        )
        self.event = event("1.0.0-alpha.2", prerelease=True)
        self.contents = {name: (name + " signed fixture").encode() for name in publisher.FORMAL_ASSETS}
        self.event["release"]["assets"] = [{
            "name": name, "state": "uploaded", "size": len(content),
            "digest": "sha256:" + hashlib.sha256(content).hexdigest(),
            "browser_download_url": f"https://github.com/sailry/sailry-harness/releases/download/v1.0.0-alpha.2/{name}",
        } for name, content in self.contents.items()]

    def download(self, remote="source"):
        self.arguments.event.write_text(json.dumps(self.event))
        def run(command, **options):
            self.assertEqual(command[:7], ["gh", "release", "download", "v1.0.0-alpha.2",
                                          "--repo", self.arguments.repository, "--dir"])
            self.assertEqual(command[7], str(self.arguments.assets))
            self.assertEqual(set(command[9::2]), publisher.UPDATER_ASSETS)
            self.assertEqual(command[8::2], ["--pattern"] * 4)
            self.assertNotIn(publisher.SIGNING_KEY, options["env"])
            for name in publisher.UPDATER_ASSETS:
                (self.arguments.assets / name).write_bytes(self.contents[name])
            return publisher.subprocess.CompletedProcess(command, 0)
        with patch.object(publisher.release_api, "tagged_commit", return_value="source") as tagged, \
                patch.object(publisher.release_api, "remote_commit", return_value=remote), \
                patch.object(publisher.subprocess, "run", side_effect=run) as command:
            result = publisher.download(self.arguments, client=Client())
            tagged.assert_called_once_with("v1.0.0-alpha.2", root=self.root)
        return result, command

    def test_selects_only_exact_signed_tag_assets(self):
        self.event["release"]["assets"].append({"name": "Sailry-x86_64-apple-darwin-adhoc.zip"})
        ready, command = self.download()
        self.assertTrue(ready)
        self.assertEqual(command.call_count, 1)
        self.assertEqual({path.name for path in self.arguments.assets.iterdir()}, publisher.UPDATER_ASSETS)

    def test_skips_ad_hoc_or_incomplete_publication(self):
        for index, absent in enumerate((publisher.FORMAL_ASSETS, {next(iter(publisher.FORMAL_ASSETS))})):
            with self.subTest(absent=absent):
                original = self.event["release"]["assets"]
                self.event["release"]["assets"] = [asset for asset in original if asset["name"] not in absent]
                ready, command = self.download()
                self.assertFalse(ready)
                command.assert_not_called()
                self.assertFalse(self.arguments.assets.exists())
                self.event["release"]["assets"] = original

    def test_rejects_moved_tag_and_mutable_urls(self):
        with self.assertRaisesRegex(ValueError, "tag has changed"):
            self.download(remote="other source")
        asset = next(asset for asset in self.event["release"]["assets"]
                     if asset["name"] in publisher.UPDATER_ASSETS)
        for url in (f"https://github.com/sailry/sailry-harness/releases/latest/download/{asset['name']}",
                    asset["browser_download_url"].replace("v1.0.0-alpha.2", "v1.0.0-alpha.1")):
            with self.subTest(url=url), self.assertRaisesRegex(ValueError, "exact published tag"):
                asset["browser_download_url"] = url
                self.download()

    def test_rejects_changed_release_digest(self):
        asset = next(asset for asset in self.event["release"]["assets"]
                     if asset["name"] in publisher.UPDATER_ASSETS)
        asset["digest"] = "sha256:" + "f" * 64
        with self.assertRaisesRegex(ValueError, "digest differs"):
            self.download()

    def test_rejects_duplicate_signed_identity(self):
        self.event["release"]["assets"].append(self.event["release"]["assets"][0])
        with self.assertRaisesRegex(ValueError, "asset set is invalid"):
            self.download()

    def test_rejects_ad_hoc_payloads_and_links(self):
        self.arguments.assets.mkdir()
        for name in publisher.UPDATER_ASSETS:
            (self.arguments.assets / name).write_bytes(b"fixture")
        extra = self.arguments.assets / "Sailry-x86_64-apple-darwin-adhoc.zip"
        extra.write_bytes(b"ad hoc")
        with self.assertRaisesRegex(ValueError, "canonical signed ZIPs"):
            publisher.canonical_assets(self.arguments.assets)
        extra.unlink()
        archive = self.arguments.assets / "Sailry-x86_64-apple-darwin.zip"
        archive.unlink()
        archive.symlink_to("Sailry-aarch64-apple-darwin.zip")
        with self.assertRaisesRegex(ValueError, "canonical signed ZIPs"):
            publisher.canonical_assets(self.arguments.assets)


class CatalogPublication(unittest.TestCase):
    def publish(self, preview=False, existing=None, immutable=False, incomplete=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            assets = root / "assets"
            assets.mkdir()
            for name in publisher.UPDATER_ASSETS:
                (assets/name).write_bytes(b"signed fixture")
            version = "1.0.0-alpha.2" if preview else "1.0.0"
            tag = "v"+version
            payload = root/"event.json"
            payload.write_text(json.dumps(event(version,prerelease=preview)))
            arguments = SimpleNamespace(source=root,assets=assets,event=payload,repository="sailry/sailry-harness",
                binary=Path("release-tool"),keys=Path("keys"),stage=root/"stage")
            metadata = {"immutable":immutable,"assets":[{"name":name,"size":1,"state":"uploaded",
                "browser_download_url":f"https://github.com/sailry/sailry-harness/releases/download/{tag}/{name}"}
                for name in publisher.FORMAL_ASSETS]}
            if incomplete: metadata["assets"].pop()
            if existing: metadata["assets"].append({**existing,"name":"update-v1.json",
                "browser_download_url":f"https://github.com/sailry/sailry-harness/releases/download/{tag}/update-v1.json"})
            client = publisher.GitHub()
            calls = []
            def merge(binary,keys,assets,repository,release,channel,output,signing_key):
                calls.append((release.tag,channel))
                output.write_bytes(b"authenticated exact version catalog")
            with patch.object(publisher,"verify_source",return_value="exact product") as source, \
                    patch.object(publisher.release_api,"release",return_value=metadata) as release, \
                    patch.object(publisher,"merge",side_effect=merge), \
                    patch.object(client,"upload") as upload:
                publisher.publish(arguments,"signing fixture",client)
                release.assert_called_once_with(client,arguments.repository,tag,"exact product",published=True)
                self.assertEqual(source.call_count,2)
                if existing: upload.assert_not_called()
                else: upload.assert_called_once_with(arguments.repository,tag,root/"stage/update-v1.json")
            return calls

    def test_publishes_each_channel_to_its_exact_release(self):
        self.assertEqual(self.publish(),[("v1.0.0","stable")])
        self.assertEqual(self.publish(preview=True),[("v1.0.0-alpha.2","experience")])

    def test_preserves_already_published_identical_catalog(self):
        data = b"authenticated exact version catalog"
        self.publish(existing={"state":"uploaded","size":len(data),
            "digest":"sha256:"+hashlib.sha256(data).hexdigest()})

    def test_rejects_replacement_and_incomplete_formal_release(self):
        for options in ({"existing":{"state":"uploaded","size":1,"digest":"sha256:"+"a"*64}},
                        {"immutable":True},{"incomplete":True}):
            with self.subTest(options=options),self.assertRaises(ValueError):
                self.publish(**options)


class Workflow(unittest.TestCase):
    def test_admits_only_explicit_signed_publication(self):
        workflow = (ROOT / ".github/workflows/update-feed.yml").read_text()
        self.assertNotIn("\n  release:", workflow.split("permissions:", 1)[0])
        self.assertIn("workflow_dispatch:", workflow)
        self.assertIn("group: published-update-catalog-${{ inputs.tag }}", workflow)
        self.assertIn("ref: refs/tags/${{ inputs.tag }}", workflow)
        self.assertIn("ref: ${{ github.workflow_sha }}", workflow)
        self.assertIn("--source \"$GITHUB_WORKSPACE\"", workflow)
        self.assertNotIn("Sailry-*", workflow)
        for name in ("Build pinned release producer", "Publish signed catalog to the exact release"):
            step = workflow.split(f"      - name: {name}\n", 1)[1].split("      - name:", 1)[0]
            self.assertIn("if: steps.canonical.outputs.ready == 'true'", step)


if __name__ == "__main__":
    unittest.main()
