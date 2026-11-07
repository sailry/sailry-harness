"""The Windows installer reuses the staged bundle, a pinned compiler and reproducible artwork."""

import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct
import tempfile
import unittest
from unittest.mock import patch
import zlib

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("windows_installer", ROOT / "scripts/package/windows-installer.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)
art = installer.art
TARGET = "x86_64-pc-windows-msvc"


def image(path):
    """Width, height, channel count and unfiltered rows of a generated PNG."""
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    width, height, depth, kind = struct.unpack(">IIBB", data[16:26])
    assert depth == 8
    channels = {2: 3, 6: 4}[kind]
    stream, offset = b"", 8
    while offset < len(data):
        length = struct.unpack(">I", data[offset:offset + 4])[0]
        if data[offset + 4:offset + 8] == b"IDAT":
            stream += data[offset + 8:offset + 8 + length]
        offset += 12 + length
    raw = zlib.decompress(stream)
    stride = width * channels + 1
    rows = [raw[row * stride + 1:(row + 1) * stride] for row in range(height)]
    assert all(raw[row * stride] == 0 for row in range(height))
    return width, height, channels, rows


def pixel(rows, channels, x, y):
    return tuple(rows[y][x * channels:(x + 1) * channels])


class Artwork(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.paths = art.write(art.MARK, cls.directory.name)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_draws_every_image_area(self):
        expected = {f"wizard-{width}x{height}.png": (width, height, 3) for width, height in art.WIZARD}
        expected |= {f"small-{size}.png": (size, size, 4) for size in art.SMALL}
        self.assertEqual({path.name for path in self.paths}, set(expected))
        for path in self.paths:
            with self.subTest(image=path.name):
                self.assertEqual(image(path)[:3], expected[path.name])

    def test_is_reproducible(self):
        with tempfile.TemporaryDirectory() as directory:
            again = art.write(art.MARK, directory)
            self.assertEqual([path.read_bytes() for path in again], [path.read_bytes() for path in self.paths])

    def test_paints_brand_colors(self):
        fills = {fill for fill, _ in art.shapes(art.MARK.read_text(encoding="utf-8"))}
        self.assertEqual(fills, {(0x79, 0xC9, 0xF1), (0x28, 0x74, 0xDA)})
        for path in self.paths:
            width, height, channels, rows = image(path)
            colors = {pixel(rows, channels, x, y) for y in range(height) for x in range(width)}
            with self.subTest(image=path.name):
                if channels == 3:
                    for corner in ((0, 0), (width - 1, 0), (0, height - 1), (width - 1, height - 1)):
                        self.assertEqual(pixel(rows, channels, *corner), art.BACKGROUND)
                    self.assertLessEqual(fills, colors)
                else:
                    self.assertEqual(pixel(rows, channels, 0, 0)[3], 0)
                    self.assertLessEqual({(*fill, 255) for fill in fills}, colors)

    def test_rejects_unsupported_mark_geometry(self):
        for path in ("M0 0 a1 1 0 0 0 1 1Z", "M0 0 C1 1 2 2Z", "1 2 L3 4Z"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                art.rings(path)
        svg = '<svg xmlns="http://www.w3.org/2000/svg"><g transform="scale(2)"><path fill="#79C9F1" d="M0 0L1 0L1 1Z"/></g></svg>'
        with self.assertRaisesRegex(ValueError, "transform"):
            art.shapes(svg)
        with self.assertRaisesRegex(ValueError, "fill"):
            art.shapes(svg.replace(' transform="scale(2)"', "").replace("#79C9F1", "url(#paint)"))


class Helper(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)

    def bundle(self, version="0.1.0-alpha.1", target=TARGET, executable=True):
        bundle = Path(tempfile.mkdtemp(dir=self.root)) / "Sailry"
        bundle.mkdir()
        (bundle / "build.json").write_text(json.dumps({"target": target, "application_version": version}))
        if executable:
            (bundle / "sailry-desktop.exe").write_bytes(b"MZ")
        return bundle

    def test_reads_staged_version(self):
        self.assertEqual(installer.release(self.bundle(), TARGET), ("0.1.0-alpha.1", "0.1.0"))
        self.assertEqual(installer.release(self.bundle("0.2.3"), TARGET), ("0.2.3", "0.2.3"))
        for bundle in (self.bundle(target="aarch64-pc-windows-msvc"), self.bundle("v0.1"), self.bundle(executable=False)):
            with self.subTest(bundle=bundle.parent.name), self.assertRaises(ValueError):
                installer.release(bundle, TARGET)
        with self.assertRaises(ValueError):
            installer.release(self.bundle(), "aarch64-pc-windows-msvc")

    def test_passes_staged_inputs_to_compiler(self):
        argv = installer.command(Path("C:/inno/ISCC.exe"), Path("C:/bundle"), Path("C:/out"), TARGET,
                                 "0.1.0-alpha.1", "0.1.0", Path("C:/icon.ico"), "C:/art")
        self.assertEqual(argv[0], str(Path("C:/inno/ISCC.exe")))
        self.assertEqual(argv[-1], str(installer.SCRIPT))
        for value in ("/Qp", f"/O{Path('C:/out')}", "/FSailry-x86_64-pc-windows-msvc-setup",
                      "/DAppVersion=0.1.0-alpha.1", "/DVersionCore=0.1.0", f"/DBundle={Path('C:/bundle')}",
                      f"/DIconFile={Path('C:/icon.ico')}", "/DArtDir=C:/art"):
            self.assertIn(value, argv)
        self.assertFalse([value for value in argv if value.startswith(("/S", "/DSignTool"))])
        signed = installer.command(Path("ISCC.exe"), Path("b"), Path("o"), TARGET, "1.0.0", "1.0.0",
                                   Path("i"), "a", "authenticode=signtool sign $f")
        self.assertIn("/Sauthenticode=signtool sign $f", signed)
        self.assertIn("/DSignTool=authenticode", signed)
        for tool in ("signtool", "=signtool", "bad name=signtool", "name="):
            with self.subTest(tool=tool), self.assertRaises(ValueError):
                installer.command(Path("ISCC.exe"), Path("b"), Path("o"), TARGET, "1.0.0", "1.0.0", Path("i"), "a", tool)

    def test_installs_only_the_pinned_compiler(self):
        payload = b"pinned installer"
        lock = {"version": "7.1.1", "url": "https://example.invalid/innosetup.exe", "archive": "innosetup.exe",
                "size": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}
        cache = self.root / "cache"

        class Response:
            def __init__(self, data):
                self.data = data

            def __enter__(self):
                return self

            def __exit__(self, *arguments):
                return False

            def read(self, limit):
                return self.data[:limit]

        with patch.object(installer, "urlopen", return_value=Response(b"tampered installer")), \
                patch.object(installer.subprocess, "run") as run, self.assertRaisesRegex(ValueError, "checksum"):
            installer.compiler(cache, lock)
        run.assert_not_called()
        self.assertFalse((cache / "innosetup.exe").exists())

        def install(argv, **options):
            directory = Path(next(value for value in argv if value.startswith("/DIR="))[5:])
            directory.mkdir(parents=True)
            (directory / "ISCC.exe").write_bytes(b"compiler")

        with patch.object(installer, "urlopen", return_value=Response(payload)), \
                patch.object(installer.subprocess, "run", side_effect=install) as run:
            executable = installer.compiler(cache, lock)
        self.assertEqual(executable, cache / "inno-setup-7.1.1/ISCC.exe")
        argv = run.call_args.args[0]
        self.assertEqual(argv[0], str(cache / "innosetup.exe"))
        for flag in ("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CURRENTUSER"):
            self.assertIn(flag, argv)
        with patch.object(installer, "urlopen", side_effect=AssertionError("unexpected download")):
            self.assertEqual(installer.compiler(cache, lock), executable)

    def test_reports_missing_compiler(self):
        payload = b"installer"
        lock = {"version": "7.1.1", "url": "https://example.invalid/a.exe", "archive": "a.exe",
                "size": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}
        (self.root / "a.exe").write_bytes(payload)
        with patch.object(installer.subprocess, "run"), self.assertRaisesRegex(ValueError, "compiler"):
            installer.compiler(self.root, lock)

    def test_requires_windows(self):
        with patch.object(installer.platform, "system", return_value="Darwin"), \
                patch.object(installer, "compiler", side_effect=AssertionError("unexpected compiler")), \
                self.assertRaisesRegex(ValueError, "Windows"):
            installer.build(ROOT, self.bundle(), TARGET, self.root / "out", self.root / "cache")

    def test_records_both_release_files(self):
        for name in installer.assets(TARGET):
            (self.root / name).write_bytes(name.encode())
        result = installer.checksums(self.root, TARGET)
        self.assertEqual(result.name, f"SHA256SUMS-desktop-{TARGET}")
        self.assertEqual(result.read_bytes().decode(), "".join(
            f"{hashlib.sha256(name.encode()).hexdigest()}  {name}\n" for name in installer.assets(TARGET)))
        (self.root / installer.assets(TARGET)[1]).unlink()
        with self.assertRaisesRegex(ValueError, "missing"):
            installer.checksums(self.root, TARGET)

    def test_pins_compiler_release(self):
        lock = json.loads(installer.LOCK.read_text())
        self.assertRegex(lock["sha256"], r"^[0-9a-f]{64}$")
        self.assertGreater(lock["size"], 0)
        self.assertTrue(lock["url"].startswith("https://github.com/jrsoftware/issrc/releases/download/"))
        self.assertTrue(lock["url"].endswith("/" + lock["archive"]))
        self.assertIn(lock["version"], lock["archive"])


class Script(unittest.TestCase):
    def setUp(self):
        self.script = installer.SCRIPT.read_text(encoding="utf-8")

    def section(self, name):
        return self.script.split(f"\n[{name}]\n", 1)[1].split("\n[", 1)[0]

    def test_installs_per_user_beside_updater_workspace(self):
        setup = self.section("Setup")
        for line in ("PrivilegesRequired=lowest", "DefaultDirName={autopf}\\Sailry", "CloseApplications=yes",
                     "RestartApplications=no", "WizardStyle=modern dynamic", "SetupIconFile={#IconFile}",
                     "UninstallDisplayIcon={app}\\app\\sailry-desktop.exe", "AppVersion={#AppVersion}",
                     "WizardImageFile={#ArtDir}\\wizard-*.png", "WizardSmallImageFile={#ArtDir}\\small-*.png"):
            self.assertIn(line + "\n", setup)
        self.assertNotIn("PrivilegesRequiredOverridesAllowed", setup)
        self.assertNotIn("UninstallFilesDir", setup)
        self.assertRegex(setup, r"AppId=\{\{[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}\}\n")
        self.assertIn('Source: "{#Bundle}\\*"; DestDir: "{app}\\app"; Flags: ignoreversion recursesubdirs createallsubdirs',
                      self.section("Files"))
        self.assertIn('Type: filesandordirs; Name: "{app}\\app"', self.section("InstallDelete"))
        uninstall = self.section("UninstallDelete")
        self.assertIn('Type: filesandordirs; Name: "{app}\\app"', uninstall)
        self.assertIn('Type: filesandordirs; Name: "{app}\\.sailry-update-*"', uninstall)
        self.assertNotIn(".sailry\"", uninstall)
        self.assertIn("Flags: nowait postinstall skipifsilent", self.section("Run"))
        self.assertIn("Tasks: desktopicon", self.section("Icons"))

    def test_matches_desktop_languages(self):
        official = {"en": "compiler:Default.isl", "de": "German", "es": "Spanish", "fr": "French", "ja": "Japanese",
                    "ko": "Korean", "pt-BR": "BrazilianPortuguese", "ru": "Russian",
                    "zh-CN": "ChineseSimplified", "zh-TW": "ChineseTraditional"}
        locales = {path.name.split(".")[-2] for path in (ROOT / "locales").glob("application.*.yml")}
        locales |= {path.name.split(".")[-2] for path in (ROOT / "locales").glob("artifact.*.yml")}
        self.assertEqual(locales, set(official))
        files = set(re.findall(r'MessagesFile: "([^"]+)"', self.section("Languages")))
        self.assertEqual(files, {name if name.startswith("compiler:") else f"compiler:Languages\\{name}.isl"
                                 for name in official.values()})

    def test_adds_no_custom_copy(self):
        self.assertNotIn("[Messages]", self.script)
        self.assertNotIn("[CustomMessages]", self.script)
        for description in re.findall(r'Description: "([^"]*)"', self.script):
            self.assertRegex(description, r"^\{cm:[A-Za-z]+(?:,Sailry)?\}$")

    def test_signs_only_when_requested(self):
        block = self.script.split("#ifdef SignTool\n", 1)[1].split("#endif", 1)[0]
        self.assertEqual(block, "SignTool={#SignTool}\nSignedUninstaller=yes\n")


class ReleaseWorkflow(unittest.TestCase):
    def test_checksums_follow_installer(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        job = workflow.split("\n  windows:\n", 1)[1].split("\n  windows_publish:\n", 1)[0]
        steps = ["name: Assemble portable Desktop", "name: Build Windows installer",
                 "name: Record Windows checksums", "name: windows-package-x86_64-pc-windows-msvc"]
        positions = [job.index(step) for step in steps]
        self.assertEqual(positions, sorted(positions))
        self.assertIn("windows-installer.py build --source \"$GITHUB_WORKSPACE\"", job)
        self.assertIn("PACKAGE_BUNDLE: ${{ steps.portable.outputs.bundle }}", job)
        self.assertNotIn("SHA256SUMS'", job)
        self.assertNotIn("secrets.", job.split("name: Build Windows installer", 1)[1])


if __name__ == "__main__":
    unittest.main()
