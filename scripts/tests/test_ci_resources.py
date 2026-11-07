"""CI requires selected checks and preserves lint and test coverage."""

import copy
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[2]
JOBS = ("source", "lint", "build", "shared", "desktop", "windows", "host", "mobile", "android", "services")
spec = importlib.util.spec_from_file_location("ci", ROOT / "scripts/ci.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


def workflow_source():
    workflow = (ROOT / ".github/workflows/ci.yml").read_text()

    def expand(job):
        # Jobs that declare the archive run its prebuilt binaries, as in CI.
        env = {"CHECK_ARCHIVE": ci.TEST_ARCHIVE} if "CHECK_ARCHIVE:" in job else {}
        if "CHECK_PARTITION:" in job:
            env["CHECK_PARTITION"] = "count:1/2"

        def command(match):
            commands = ci.check_commands(match.group(1), "1", selection=ci.outputs(ci.LANES), env=env)
            return "\n".join(shlex.join(["python3" if value == sys.executable else value
                                           for value in command["argv"]]) for command in commands)

        return re.sub(r"python3 (?:(?:\.\./){2})?scripts/ci\.py check ([a-z-]+)", command, job)

    return "".join(expand(job) for job in re.split(r"(?=\n  [a-z_]+:\n)", workflow))


def job_section(workflow, name):
    return re.split(r"\n  [a-z_]+:\n", workflow.split(f"\n  {name}:\n", 1)[1], maxsplit=1)[0]


class CheckResults(unittest.TestCase):
    def results(self, selected):
        selection = {**ci.outputs(selected), "mode": "full", "light_linux": "false", "light_macos": "false"}
        return {
            "source": {"result": "success", "outputs": selection},
            **{name: {"result": "success" if lanes & selected else "skipped"}
               for name, lanes in ci.CHECK_JOBS.items()},
            "lint": {"result": "success" if ci.lint_packages(selected) else "skipped"},
            **{name: {"result": "skipped"} for name in ci.LIGHT_JOBS},
        }

    def light(self, selected, mode="light"):
        jobs = ci.light_jobs(selected, mode)
        results = self.results(set())
        results["source"]["outputs"] = {**ci.outputs(selected), "mode": mode,
                                        **{name: str(name in jobs).lower() for name in ci.LIGHT_JOBS}}
        for name in ci.LIGHT_JOBS:
            results[name]["result"] = "success" if name in jobs else "skipped"
        results["services"]["result"] = "success" if "services" in selected else "skipped"
        return results

    def summarize(self, results, product=None):
        try:
            ci.require_results(results, product)
        except ValueError as error:
            return SimpleNamespace(returncode=1, stderr=str(error))
        return SimpleNamespace(returncode=0, stderr="")

    def test_report_cli(self):
        for product in (None, *ci.PRODUCTS):
            for results in (self.results(ci.LANES), {}):
                with self.subTest(product=product, results=bool(results)):
                    result = subprocess.run(
                        [sys.executable, str(ROOT / "scripts/ci.py"), "report",
                         *([product] if product else [])],
                        env={**os.environ, "CHECK_RESULTS": json.dumps(results)},
                        text=True, capture_output=True,
                    )
                    self.assertEqual(result.returncode == 0, bool(results), result.stderr)
                    if results:
                        self.assertIn(f"{product.title()} ready" if product else "Checks passed",
                                      result.stdout)
                    else:
                        self.assertIn("Missing check results", result.stderr)

    def test_requires_all_jobs(self):
        results = self.results(ci.LANES)
        result = self.summarize(results)
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in JOBS:
            for status in ("failure", "cancelled", "skipped"):
                with self.subTest(job=name, status=status):
                    changed = copy.deepcopy(results)
                    changed[name]["result"] = status
                    self.assertNotEqual(self.summarize(changed).returncode, 0)

    def test_accepts_only_intentional_skips(self):
        for selected in (
            set(), {"desktop"}, {"shared"}, {"host"}, {"mobile"}, {"mobile_ffi"}, {"android"}, {"services"},
        ):
            with self.subTest(selected=selected):
                results = self.results(selected)
                result = self.summarize(results)
                self.assertEqual(result.returncode, 0, result.stderr)
                for name in JOBS:
                    changed = copy.deepcopy(results)
                    changed[name]["result"] = "skipped"
                    if results[name]["result"] == "success":
                        self.assertNotEqual(self.summarize(changed).returncode, 0)
                    for status in ("failure", "cancelled", "unknown", None):
                        changed[name]["result"] = status
                        self.assertNotEqual(self.summarize(changed).returncode, 0)

    def test_light_requires_lint_and_skipped_tests(self):
        for selected, jobs in ((set(), set()), ({"services"}, set()), ({"host"}, {"light_linux"}),
                               ({"desktop"}, {"light_linux", "light_macos"}),
                               ({"shared"}, {"light_linux", "light_macos"}),
                               ({"mobile", "android"}, {"light_linux", "light_macos"}),
                               (ci.LANES, {"light_linux", "light_macos"})):
            with self.subTest(selected=selected):
                self.assertEqual(ci.light_jobs(selected, "light"), jobs)
                results = self.light(selected)
                self.assertEqual(ci.require_results(results), "light")
                for name in JOBS[1:] + tuple(ci.LIGHT_JOBS):
                    for status in ("success", "failure", "cancelled", "skipped", None):
                        if status == results[name]["result"]:
                            continue
                        changed = copy.deepcopy(results)
                        changed[name]["result"] = status
                        accepted = name == "services" and not selected & {"services"} and status == "success"
                        self.assertEqual(self.summarize(changed).returncode == 0, accepted, (name, status))

    def test_light_ignores_untrusted_job_outputs(self):
        results = self.light({"desktop"})
        results["source"]["outputs"]["light_macos"] = "false"
        results["light_macos"]["result"] = "skipped"
        self.assertNotEqual(self.summarize(results).returncode, 0)

    def test_deferred_runs_only_source(self):
        results = self.light(set(), "skip")
        self.assertEqual(ci.require_results(results), "skip")
        for name in JOBS[1:] + tuple(ci.LIGHT_JOBS):
            changed = copy.deepcopy(results)
            changed[name]["result"] = "success"
            if name != "services":
                self.assertNotEqual(self.summarize(changed).returncode, 0)
        selected = self.light({"host"}, "skip")
        self.assertNotEqual(self.summarize(selected).returncode, 0)
        for product in ci.PRODUCTS:
            self.assertNotEqual(self.summarize(self.light(ci.LANES, "light"), product).returncode, 0)

    def test_rejects_missing_mode(self):
        for mode in (None, "", "partial"):
            with self.subTest(mode=mode):
                results = self.results(ci.LANES)
                results["source"]["outputs"]["mode"] = mode
                self.assertNotEqual(self.summarize(results).returncode, 0)
        results = self.results(ci.LANES)
        del results["light_linux"]
        self.assertNotEqual(self.summarize(results).returncode, 0)

    def test_rejects_missing_results(self):
        self.assertNotEqual(self.summarize({}).returncode, 0)
        for name in JOBS:
            with self.subTest(job=name):
                results = self.results(ci.LANES)
                del results[name]
                self.assertNotEqual(self.summarize(results).returncode, 0)

    def test_rejects_missing_selection(self):
        for key in ci.LANES:
            with self.subTest(key=key):
                results = self.results({"desktop"})
                del results["source"]["outputs"][key]
                self.assertNotEqual(self.summarize(results).returncode, 0)
        for invalid in (None, [], "false", {"desktop": True}):
            results = self.results({"desktop"})
            results["source"]["outputs"] = invalid
            self.assertNotEqual(self.summarize(results).returncode, 0)

    def test_requires_service_result(self):
        for selected in (set(), {"services"}):
            for status in ("success", "skipped", "failure", "cancelled", "unknown", None):
                with self.subTest(selected=selected, status=status):
                    results = self.results(selected)
                    results["services"]["result"] = status
                    passed = self.summarize(results).returncode == 0
                    self.assertEqual(passed, status == "success" or (status == "skipped" and not selected))

    def test_rejects_invalid_selection(self):
        for value in (True, False, "invalid", None):
            with self.subTest(value=value):
                results = self.results(ci.LANES)
                results["source"]["outputs"]["shared"] = value
                self.assertNotEqual(self.summarize(results).returncode, 0)

    def test_products_require_actual_success(self):
        for product in ci.PRODUCTS:
            with self.subTest(product=product):
                results = self.results(ci.LANES)
                needed = {"source", "lint"} | ci.required_jobs(product)
                for name in set(JOBS) - needed:
                    results[name]["result"] = "failure"
                result = self.summarize(results, product)
                self.assertEqual(result.returncode, 0, result.stderr)
                for name in needed:
                    for status in ("failure", "cancelled", "skipped", None):
                        with self.subTest(job=name, status=status):
                            changed = copy.deepcopy(results)
                            changed[name]["result"] = status
                            self.assertNotEqual(self.summarize(changed, product).returncode, 0)

    def test_windows_gates_only_desktop(self):
        self.assertIn("windows", ci.required_jobs())
        self.assertIn("windows", ci.required_jobs("desktop"))
        results = self.results(ci.LANES)
        results["windows"]["result"] = "failure"
        self.assertNotEqual(self.summarize(results).returncode, 0)
        self.assertNotEqual(self.summarize(results, "desktop").returncode, 0)
        for product in ("host", "mobile"):
            with self.subTest(product=product):
                self.assertNotIn("windows", ci.required_jobs(product))
                result = self.summarize(results, product)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_products_reject_partial_selection(self):
        for product, lanes in ci.PRODUCTS.items():
            for missing in lanes:
                with self.subTest(product=product, missing=missing):
                    results = self.results(ci.LANES - {missing})
                    for name, selected in ci.CHECK_JOBS.items():
                        if missing in selected:
                            results[name]["result"] = "success"
                    self.assertNotEqual(self.summarize(results, product).returncode, 0)


class SharedCommands(unittest.TestCase):
    def packages(self, command):
        return [command[index + 1] for index, value in enumerate(command) if value == "-p"]

    def test_limits_the_selected_package_union(self):
        for selected in ({"shared"}, {"host"}, {"shared", "host"}):
            with self.subTest(selected=selected):
                scope = ci.outputs(selected | {"desktop", "mobile", "mobile_ffi", "android", "services"})
                commands = ci.check_commands("shared", "2", selection=scope, threads="7")
                run = commands[0]["argv"]
                expected = [*(ci.LINT_PACKAGES["shared"] if "shared" in selected else ()),
                            *(ci.LINT_PACKAGES["host"] if "host" in selected else ())]
                self.assertEqual(self.packages(run), expected)
                self.assertEqual(len(commands), 3 if "shared" in selected else 1)
                self.assertIn("--test-threads=7", run)
                self.assertIn("--timings", run)
                self.assertEqual(run[run.index("--build-jobs") + 1], "2")
                if selected == {"host"}:
                    self.assertEqual(run[run.index("--test") + 1], "lifecycle")
                    self.assertEqual(commands, ci.check_commands("host-mac", "2", threads="7"))
                    self.assertNotIn("--filterset", run)
                elif "host" in selected:
                    self.assertEqual(run[run.index("--filterset") + 1],
                                     "not (package(sailry-host) and not binary(lifecycle))")
                    self.assertNotIn("--test", run)
                else:
                    self.assertNotIn("--filterset", run)

    def test_keeps_plugin_and_database_graphs_identical(self):
        for selected in ({"shared"}, {"shared", "host"}):
            with self.subTest(selected=selected):
                selection = ci.outputs(selected)
                ordinary = ci.check_commands("shared", "2", selection=selection)
                databases = ci.check_commands("shared-databases", "2", selection=selection)
                self.assertEqual(len(databases), 1)
                run, build, contracts = [item["argv"] for item in ordinary]
                database = databases[0]["argv"]
                self.assertEqual(self.packages(run), self.packages(build))
                self.assertEqual(self.packages(run), self.packages(database))
                self.assertEqual(run[run.index("--profile") + 1], "ci")
                self.assertEqual(build[build.index("--profile") + 1], "test")
                self.assertEqual(database[database.index("--profile") + 1], "databases")
                self.assertEqual(database[database.index("--filterset") + 1], "binary(databases)")
                self.assertEqual(database[database.index("--run-ignored") + 1], "only")
                self.assertIn("--test-threads=1", database)
                self.assertNotIn("--run-ignored", run)
                self.assertEqual(build[build.index("--features") + 1], "sailry-node-runtime/test-support")
                for argv in (run, database):
                    self.assertEqual(argv[argv.index("--features") + 1], "sailry-node-runtime/test-support")
                self.assertEqual(build[build.index("--example") + 1], "plugin-test-host")
                self.assertEqual(contracts[1], "scripts/test-plugins.py")

    def test_rejects_invalid_or_empty_selection(self):
        for name in ("shared", "shared-databases"):
            for selection in (None, {}, ci.outputs(set()), ci.outputs({"desktop"}),
                              {**ci.outputs({"shared"}), "host": True},
                              {**ci.outputs({"shared"}), "host": "invalid"}):
                with self.subTest(name=name, selection=selection), self.assertRaises(ValueError):
                    ci.check_commands(name, selection=selection)
        with self.assertRaises(ValueError):
            ci.check_commands("shared-databases", selection=ci.outputs({"host"}))

    def test_reads_scope_from_the_cli(self):
        selection = ci.outputs({"shared", "host"})
        for name in ("archive", "shared", "shared-databases", "windows"):
            with self.subTest(name=name), patch.dict(os.environ, {"CHECK_SCOPE": json.dumps(selection)}), \
                    patch.object(sys, "argv", ["ci.py", "check", name]), \
                    patch.object(ci, "run_check", return_value=0) as run:
                with self.assertRaises(SystemExit) as outcome:
                    ci.main()
                self.assertEqual(outcome.exception.code, 0)
                run.assert_called_once_with(name, selection=selection)

    def test_prepares_the_native_test_environment(self):
        for name in ("shared", "shared-databases", "windows"):
            with self.subTest(name=name), \
                    patch.object(ci, "check_commands", return_value=[{"argv": ["fixture"], "cwd": "."}]), \
                    patch.object(ci, "parser_environment", return_value={}) as prepare, \
                    patch.object(ci.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run:
                self.assertEqual(ci.run_check(name), 0)
                prepare.assert_called_once_with(ROOT, None, libraries=True)
                self.assertEqual(run.call_args.kwargs["env"]["GOOSE_PROVIDER_SKIP_BACKOFF"], "true")

    def test_hides_ci_selection_from_tests(self):
        environment = {"CHECK_SCOPE": "{}", "CHECK_PARTITION": "count:1/2", "PATH": "fixture path"}
        for name in ("source", "mobile-check", "host"):
            with self.subTest(name=name), \
                    patch.object(ci, "check_commands", return_value=[{"argv": ["fixture"], "cwd": "."}]), \
                    patch.object(ci, "parser_environment", side_effect=lambda root, env, libraries: dict(env)), \
                    patch.object(ci.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run:
                self.assertEqual(ci.run_check(name, env=environment), 0)
                passed = run.call_args.kwargs["env"]
                self.assertEqual(passed["PATH"], "fixture path")
                self.assertFalse([key for key in passed if key.startswith("CHECK_")])
        self.assertIn("CHECK_SCOPE", environment)


class WindowsCommands(unittest.TestCase):
    def test_runs_unit_tests_of_selected_packages(self):
        for selected in ({"shared"}, {"desktop"}, {"desktop", "host", "mobile"}, ci.LANES):
            with self.subTest(selected=selected):
                commands = ci.check_commands("windows", "4", selection=ci.outputs(selected), threads="3")
                self.assertEqual(len(commands), 1)
                run = commands[0]["argv"]
                self.assertEqual(run[:4], ["cargo", "nextest", "run", "--locked"])
                packages = [run[index + 1] for index, value in enumerate(run) if value == "-p"]
                self.assertEqual(packages, ci.lint_packages(selected, "windows"))
                self.assertNotIn("sailry-host", packages)
                self.assertNotIn("sailry-mobile-bridge", packages)
                self.assertEqual("--features" in run, "shared" in selected)
                for option in ("--lib", "--bins", "--timings", "--test-threads=3"):
                    self.assertIn(option, run)
                self.assertEqual(run[run.index("--profile") + 1], "ci")
                self.assertEqual(run[run.index("--build-jobs") + 1], "4")
                self.assertNotIn("--archive-file", run)

    def test_requires_windows_lanes(self):
        for selection in (None, ci.outputs(set()), ci.outputs({"host", "mobile", "android"}),
                          {**ci.outputs({"desktop"}), "shared": "yes"}):
            with self.subTest(selection=selection), self.assertRaises(ValueError):
                ci.check_commands("windows", selection=selection)


class Archives(unittest.TestCase):
    def test_builds_the_selected_union(self):
        for selected in ({"shared"}, {"desktop"}, {"host"}, {"shared", "desktop", "host"}):
            with self.subTest(selected=selected):
                commands = ci.check_commands("archive", "2", selection=ci.outputs(selected), env={})
                archive = commands[-1]["argv"]
                self.assertEqual(archive[:4], ["cargo", "nextest", "archive", "--locked"])
                packages = [archive[index + 1] for index, value in enumerate(archive) if value == "-p"]
                self.assertEqual(set(packages), {package for lane in selected for package in ci.LINT_PACKAGES[lane]})
                self.assertEqual(archive[archive.index("--archive-file") + 1], str(ROOT / ci.TEST_ARCHIVE))
                self.assertEqual(archive[archive.index("--build-jobs") + 1], "2")
                self.assertEqual(len(commands), 2 if "shared" in selected else 1)
                if "shared" in selected:
                    # Test crates and the plugin host share one schedule; the archive then compiles nothing new.
                    build = commands[0]["argv"]
                    self.assertEqual(build[:3], ["cargo", "build", "--locked"])
                    self.assertEqual(build[3:build.index("--tests")], archive[4:archive.index("--timings")])
                    self.assertEqual(build[build.index("--tests") + 1:build.index("--profile")],
                                     ["--example", "plugin-test-host"])
                    self.assertEqual(build[build.index("--profile") + 1], "test")
                    self.assertIn("--timings", build)
                    self.assertEqual(build[build.index("--jobs") + 1], "2")

    def test_partitions_only_ordinary_shared_tests(self):
        for selected in ({"shared"}, {"host"}, {"shared", "host"}):
            for partition in ("count:1/2", "count:2/2"):
                with self.subTest(selected=selected, partition=partition):
                    commands = ci.check_commands("shared", selection=ci.outputs(selected), env={
                        "CHECK_ARCHIVE": ci.TEST_ARCHIVE, "CHECK_PARTITION": partition,
                    })
                    self.assertEqual(len(commands), 1)
                    run = commands[0]["argv"]
                    self.assertEqual(run[:3], ["cargo", "nextest", "run"])
                    self.assertEqual(run[run.index("--extract-to") + 1], str(ROOT))
                    self.assertIn("--extract-overwrite", run)
                    self.assertEqual(run[run.index("--partition") + 1], partition)
                    self.assertNotIn("--locked", run)
                    self.assertNotIn("-p", run)
                    filters = run[run.index("--filterset") + 1]
                    self.assertEqual(set(re.findall(r"package\(([\w-]+)\)", filters)),
                                     {package for lane in selected for package in ci.LINT_PACKAGES[lane]})
                    if "host" in selected:
                        self.assertIn("not (package(sailry-host) and not binary(lifecycle))", filters)
        env = {"CHECK_ARCHIVE": ci.TEST_ARCHIVE, "CHECK_PARTITION": "count:2/2"}
        for name, profile in (("shared-databases", "databases"), ("desktop", "ci"),
                              ("desktop-lifecycle", "lifecycle")):
            with self.subTest(name=name):
                run = ci.check_commands(name, selection=ci.outputs(ci.LANES), env=env)[0]["argv"]
                self.assertNotIn("--partition", run)
                self.assertNotIn("--no-tests=pass", run)
                self.assertEqual(run[run.index("--profile") + 1], profile)
                if name == "shared-databases":
                    self.assertEqual(run[run.index("--run-ignored") + 1], "only")
                    self.assertIn("binary(databases)", run[run.index("--filterset") + 1])
                    self.assertIn("--test-threads=1", run)

    def test_uses_shipped_parsers_without_preparation(self):
        for name in ("shared", "shared-databases", "desktop", "desktop-lifecycle", "plugin-contracts"):
            with self.subTest(name=name), \
                    patch.object(ci, "parser_environment", side_effect=AssertionError("unexpected parser download")), \
                    patch.object(ci.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run:
                environment = {"CHECK_ARCHIVE": ci.TEST_ARCHIVE, "PATH": "fixture path"}
                self.assertEqual(ci.run_check(name, selection=ci.outputs(ci.LANES), env=environment), 0)
                self.assertEqual(run.call_count, 1)
                # The archive location selects these commands but never reaches the tests.
                self.assertEqual(run.call_args.kwargs["env"],
                                 {"PATH": "fixture path", "GOOSE_PROVIDER_SKIP_BACKOFF": "true"})
                self.assertNotIn("GOOSE_PROVIDER_SKIP_BACKOFF", environment)
                if name == "plugin-contracts":
                    self.assertEqual(run.call_args.args[0], [sys.executable, "scripts/test-plugins.py",
                                                            "--runtime-host", str(ROOT / "target/debug/examples/plugin-test-host")])

    def test_prepares_parsers_before_building(self):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(ci, "parser_environment", return_value={}) as prepare, \
                patch.object(ci.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run:
            root = Path(directory)
            self.assertEqual(ci.run_check("archive", root=root, selection=ci.outputs({"shared"}), env={}), 0)
            prepare.assert_called_once_with(root, {}, libraries=True)
            self.assertTrue((root / ci.TEST_ARCHIVE).parent.is_dir())
            self.assertEqual(run.call_count, 2)

    def test_includes_runtime_parsers(self):
        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text())
        self.assertIn({"path": "debug/code-parsers", "relative-to": "target", "depth": 1, "on-missing": "error"},
                      config["profile"]["default"]["archive"]["include"])


class Workflow(unittest.TestCase):
    def test_gates_runtime_and_service_checks(self):
        workflow = workflow_source()
        source = workflow.split("\n  source:\n", 1)[1].split("\n  services:\n", 1)[0]
        self.assertNotIn("    if:", source.split("    steps:\n", 1)[0])
        self.assertIn("fetch-depth: 0", source)
        self.assertIn("run: python3 scripts/ci.py select", source)
        for name, lanes in ci.CHECK_JOBS.items():
            section = job_section(workflow, name).split("    steps:\n", 1)[0]
            self.assertIn("needs: [source, build]" if name in {"shared", "desktop"} else "needs: source", section)
            for lane in lanes:
                self.assertIn(f"needs.source.outputs.{lane} == 'true'", section)
        self.assertIn("needs: [source, lint, build, shared, desktop, windows, host, mobile, android, services, light_linux, light_macos]", workflow)
        self.assertIn("run: python3 scripts/ci_reuse.py report", workflow)

    def test_lints_every_workspace_member(self):
        workflow = workflow_source()
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["members"]
        packages = {tomllib.loads((ROOT / member / "Cargo.toml").read_text())["package"]["name"]
                    for member in workspace}
        self.assertEqual(set(ci.lint_packages(ci.LANES)), packages)
        lint = job_section(workflow, "lint")
        self.assertIn("run: python3 scripts/ci-run.py -- python3 scripts/ci.py lint", lint)
        self.assertIn("CHECK_SCOPE: ${{ toJSON(needs.source.outputs) }}", lint)
        self.assertIn("needs: source", lint)
        self.assertNotIn("cargo clippy", workflow)
        for line in ("run: python3 scripts/ci-run.py -- python3 scripts/ci.py lint\n",
                     "run: python3 scripts/ci-run.py -- python3 scripts/ci.py lint linux\n",
                     "run: python3 scripts/ci-run.py -- python3 scripts/ci.py lint macos\n",
                     "run: python3 scripts/ci-run.py -- python3 scripts/ci.py lint windows\n"):
            self.assertEqual(workflow.count(line), 1)
        for name in ci.CHECK_JOBS:
            self.assertNotIn("needs: [source, lint]", job_section(workflow, name))

    def test_runs_shared_package_tests(self):
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["members"]
        packages = {tomllib.loads((ROOT / member / "Cargo.toml").read_text())["package"]["name"]
                    for member in workspace}
        products = {package for owner, names in ci.LINT_PACKAGES.items()
                    if owner != "shared" for package in names}
        command = ci.check_commands("backend")[0]["argv"]
        selected = {command[index + 1] for index, value in enumerate(command) if value == "-p"}
        self.assertEqual(selected, packages - products)
        self.assertNotIn("--run-ignored", command)

    def test_compiles_tests_once_and_keeps_timings(self):
        workflow = workflow_source()
        self.assertNotIn("--no-run", workflow)
        for command in re.findall(r"run: (?:python3 scripts/ci-run.py -- )?(cargo nextest run[^\n]+--profile ci[^\n]*)", workflow):
            # Archived runs compile nothing; Build records the timings instead.
            if "--archive-file" not in command:
                self.assertIn("--timings", command)
        self.assertIn("cargo nextest archive --locked", job_section(workflow, "build"))
        self.assertIn("--timings --build-jobs 1 --archive-file", job_section(workflow, "build"))
        self.assertIn("--test dart --timings --", workflow)
        self.assertIn("run: cargo fmt --all -- --check", job_section(workflow, "lint"))

    def test_runs_plugin_contracts_in_shared(self):
        workflow = workflow_source()
        build = job_section(workflow, "build")
        shared = job_section(workflow, "shared")
        self.assertIn("node-version: '22.22.1'", shared)
        # Only the executable is needed, not hashed duplicates or unrelated examples.
        self.assertIn("--example plugin-test-host --profile test", build)
        self.assertLess(build.index("--example plugin-test-host"), build.index("cargo nextest archive"))
        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text())
        self.assertIn({"path": "debug/examples/plugin-test-host", "relative-to": "target", "on-missing": "warn"},
                      config["profile"]["default"]["archive"]["include"])
        step = shared.split("      - name: Plugin contracts\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("if: matrix.partition == 2 && needs.source.outputs.shared == 'true'", step)
        self.assertIn("scripts/test-plugins.py --runtime-host", step)
        self.assertNotIn("cargo build", shared)
        self.assertNotIn("scripts/test-plugins.py", job_section(workflow, "source"))

    def test_formats_once_for_selected_native_lanes(self):
        workflow = workflow_source()
        lint = job_section(workflow, "lint")
        header = lint.split("    steps:\n", 1)[0]
        self.assertEqual(workflow.count("run: cargo fmt --all -- --check"), 2)
        self.assertIn("run: cargo fmt --all -- --check", lint)
        self.assertIn("run: cargo fmt --all -- --check", job_section(workflow, "light_linux"))
        for lane in ("shared", "desktop", "host", "mobile"):
            self.assertIn(f"needs.source.outputs.{lane} == 'true'", header)
        for lane in ("shared", "desktop", "host", "mobile", "android"):
            self.assertNotIn("name: Formatting", job_section(workflow, lane))

    def test_product_gates_require_real_jobs(self):
        workflow = workflow_source()
        for product, lanes in ci.PRODUCTS.items():
            with self.subTest(product=product):
                section = job_section(workflow, f"{product}_ready")
                self.assertIn(f"name: {product.title()} ready", section)
                self.assertIn("needs.source.result == 'success'", section)
                self.assertIn("needs.lint.result == 'success'", section)
                for lane in lanes:
                    self.assertIn(f"needs.source.outputs.{lane} == 'true'", section)
                for name in ci.CHECK_JOBS:
                    self.assertEqual(f"needs.{name}.result == 'success'" in section,
                                     name in ci.required_jobs(product))
                self.assertIn(f"python3 scripts/ci_reuse.py report {product}", section)
                self.assertIn("CHECK_REUSE_PROOF: ${{ needs.source.outputs.reuse_proof }}", section)
                self.assertIn("fetch-depth: 0", section)
                self.assertNotIn("continue-on-error", section)

    def test_separates_light_and_full_lanes(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for name in ("lint", "build", "shared", "desktop", "windows", "host", "mobile", "android"):
            header = job_section(workflow, name).split("    steps:\n", 1)[0]
            self.assertIn("needs.source.outputs.mode == 'full' && ", header)
        self.assertNotIn("outputs.mode", job_section(workflow, "services"))
        macos = {"full": [], "light": []}
        for name in re.findall(r"\n  ([a-z_]+):\n    name:", workflow):
            section = job_section(workflow, name)
            if "runs-on: macos" in section:
                macos["light" if name in ci.LIGHT_JOBS else "full"].append(name)
            if name in ci.LIGHT_JOBS:
                header = section.split("    steps:\n", 1)[0]
                self.assertIn(f"if: needs.source.outputs.{name} == 'true'", header)
                self.assertIn("CHECK_SCOPE: ${{ toJSON(needs.source.outputs) }}", header)
                self.assertNotIn("actions/cache/save", section)
                self.assertNotIn("nextest", section)
                self.assertIn(f"scripts/ci.py lint {ci.LIGHT_JOBS[name]}", section)
                self.assertIn("runs-on: macos-15" if name == "light_macos" else "runs-on: ubuntu-24.04", section)
        self.assertEqual(macos, {"full": ["lint", "build", "shared", "desktop", "mobile"], "light": ["light_macos"]})
        source = job_section(workflow, "source")
        for name in ("mode", *ci.LIGHT_JOBS):
            self.assertIn(f"{name}: ${{{{ steps.scope.outputs.{name} }}}}", source)

    def test_requires_explicit_dispatch_for_full_checks(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        trigger = workflow.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertEqual(re.findall(r"^  ([a-z_]+):", trigger, re.MULTILINE), ["workflow_dispatch"])
        self.assertIn("reuse_run:", trigger)
        self.assertEqual(ci.select_outputs("workflow_dispatch", None), {
            **ci.outputs(ci.LANES), "mode": "full", "light_linux": "false", "light_macos": "false",
        })
        self.assertIn("group: checks-${{ github.workflow }}-${{ github.sha }}", workflow)
        self.assertIn("cancel-in-progress: false", workflow)
        checks = job_section(workflow, "checks")
        self.assertIn("name: Checks passed", checks)
        self.assertIn("if: ${{ !cancelled() }}", checks)

    def test_runs_windows_desktop_lane(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        header, steps = job_section(workflow, "windows").split("    steps:\n", 1)
        for line in ("name: Desktop Windows", "runs-on: windows-2025", "shell: bash",
                     "CHECK_SCOPE: ${{ toJSON(needs.source.outputs) }}",
                     "needs.source.outputs.reuse_windows != 'true'"):
            self.assertIn(line, header)
        self.assertNotIn("OPENSSL", header)
        self.assertLess(steps.index("git config --global core.autocrlf false"), steps.index("actions/checkout@"))
        self.assertIn("python-version: '3.14.6'", steps)
        self.assertIn("target: x86_64-pc-windows-msvc\n          libraries: speech", steps)
        self.assertIn("rustc -vV | awk '/^host: / {print $2}')\" = x86_64-pc-windows-msvc", steps)
        lint = steps.index("run: python3 scripts/ci-run.py -- python3 scripts/ci.py lint windows\n")
        tests = steps.index("run: python3 scripts/ci-run.py -- python3 scripts/ci.py check windows\n")
        self.assertLess(lint, tests)
        production = steps.split("      - name: Desktop without test features\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("if: needs.source.outputs.desktop == 'true'", production)
        self.assertIn("run: python3 scripts/ci.py check desktop-production", production)
        self.assertIn("reuse_windows: ${{ steps.reuse.outputs.windows }}", job_section(workflow, "source"))
        self.assertIn("needs: [source, lint, build, shared, desktop, windows]", job_section(workflow, "desktop_ready"))
        for name in ("host_ready", "mobile_ready"):
            self.assertNotIn("windows", job_section(workflow, name))

    def test_merges_host_lifecycle_into_shared(self):
        workflow = workflow_source()
        build = job_section(workflow, "build")
        shared = job_section(workflow, "shared")
        desktop = job_section(workflow, "desktop")
        host = job_section(workflow, "host")
        lint = job_section(workflow, "lint")
        self.assertNotIn("package(sailry-host)", desktop)
        for package in ("sailry-node-runtime", "sailry-release", "sailry-host", "sailry-desktop"):
            self.assertIn(f"-p {package}", build)
        self.assertIn("cargo nextest archive --locked", build)
        self.assertIn("-p sailry-host --features sailry-node-runtime/test-support --timings", build)
        self.assertIn("not (package(sailry-host) and not binary(lifecycle))", shared)
        self.assertIn("cargo check --locked -p sailry-desktop --jobs", lint)
        # Each Host platform type-checks its own shipped feature set.
        self.assertIn("cargo check --locked -p sailry-host --jobs", host)
        self.assertIn("cargo check --locked -p sailry-host --jobs", lint)
        production = lint.split("      - name: Host without test features\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("if: needs.source.outputs.host == 'true'", production)
        self.assertIn("runs-on: ubuntu-24.04", host)
        self.assertIn("bubblewrap libx11-dev libxi-dev libxtst-dev", host)
        self.assertIn("cargo nextest run --locked -p sailry-host --timings", host)
        combined = shared.split("      - name: Shared and macOS Host contracts\n", 1)[1].split("      - ", 1)[0]
        self.assertNotIn("if: ", combined)
        databases = shared.split("      - name: Isolated database engines\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("if: matrix.partition == 1 && needs.source.outputs.shared == 'true'", databases)
        step = lint.split("      - name: Desktop without test features\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("if: needs.source.outputs.desktop == 'true'", step)
        self.assertIn("bash scripts/check-mobile-boundary.sh aarch64-apple-ios sailry-mobile-bridge --dependencies-only", workflow)
        self.assertIn("bash scripts/check-mobile-android.sh --apk", workflow)

    def test_database_reuses_shared_package_graph(self):
        workflow = workflow_source()
        shared = job_section(workflow, "shared")
        self.assertIn("CHECK_SCOPE: ${{ toJSON(needs.source.outputs) }}", shared)
        self.assertNotIn("scripts/ci.py check host-mac", (ROOT / ".github/workflows/ci.yml").read_text())
        self.assertNotIn("cargo build", shared)
        commands = [line for line in shared.splitlines() if "cargo nextest run --archive-file" in line]
        self.assertEqual(len(commands), 2)
        self.assertTrue(all("--locked" not in command and " -p " not in command for command in commands))
        packages = [re.findall(r"package\(([\w-]+)\)", command) for command in commands]
        self.assertEqual(packages[0], packages[1])
        self.assertIn("sailry-release", packages[1])
        self.assertIn("sailry-host", packages[1])
        self.assertIn("binary(lifecycle)", commands[0])
        self.assertIn("--partition count:1/2 --no-tests=pass", commands[0])
        self.assertIn("binary(databases)", commands[1])
        self.assertNotIn("--partition", commands[1])
        self.assertIn("cargo check --locked -p sailry-host", job_section(workflow, "host"))

    def test_scopes_archive_settings_to_archived_steps(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for name, archived in (("shared", {"Shared and macOS Host contracts", "Plugin contracts",
                                           "Isolated database engines"}),
                               ("desktop", {"Desktop logic and service contracts", "Desktop process lifecycle"})):
            section = job_section(workflow, name)
            header, steps = section.split("    steps:\n", 1)
            self.assertNotIn("CHECK_ARCHIVE", header)
            self.assertNotIn("CHECK_PARTITION", header)
            for step in steps.split("      - ")[1:]:
                title = step.split("\n", 1)[0].removeprefix("name: ")
                with self.subTest(job=name, step=title):
                    self.assertEqual("CHECK_ARCHIVE: .runtime/ci/tests.tar.zst" in step, title in archived)
                    self.assertEqual("CHECK_PARTITION:" in step, title == "Shared and macOS Host contracts")

    def test_source_fixtures_follow_product_owners(self):
        workflow = workflow_source()
        for owner in ci.FIXTURES:
            self.assertIn(f"python3 scripts/ci.py fixtures {owner}", workflow)
        shared = ci.fixture_names(ROOT, "shared")
        self.assertNotIn("test_android_package", shared)
        self.assertNotIn("test_host_install", shared)
        for name in ("test_release_versions", "test_start_release", "test_release_notes",
                     "test_publish_packages", "test_publish_update_feed", "test_mobile_release", "test_ui_tasks"):
            self.assertIn(name, shared)
        self.assertIn("test_onnx_runtime", shared)
        self.assertIn("scripts/check-locales.py --product desktop", workflow)
        self.assertIn("scripts/check-locales.py --product mobile", workflow)

    def test_runs_selected_mobile_phases(self):
        workflow = workflow_source()
        mobile = job_section(workflow, "mobile")
        android = job_section(workflow, "android")
        self.assertIn("flutter analyze --no-pub", mobile)
        self.assertIn("flutter test --no-pub", mobile)
        self.assertIn("flutter build ios --release --no-codesign", mobile)
        self.assertIn("runs-on: macos-15", mobile)
        # Linux runs the Dart FFI fixtures; macOS adds only the Trash-dependent checkpoint contract.
        self.assertIn("needs.source.outputs.mobile_ffi == 'true'", mobile.split("    steps:\n", 1)[0])
        self.assertNotIn("--test dart --timings", mobile)
        checkpoints = mobile.split("      - name: Dart FFI checkpoint recovery\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("!cancelled()", checkpoints)
        self.assertIn("--test dart --jobs", checkpoints)
        self.assertIn("-- --ignored --exact checkpoints::recovers_restore_receipts --nocapture", checkpoints)
        self.assertIn("OPENSSL_NO_VENDOR: '1'", checkpoints)
        self.assertNotIn("OPENSSL_NO_VENDOR", mobile.split("    steps:\n", 1)[0])
        for step in ("uses: ./.github/actions/ghostty-cache", "uses: ./.github/actions/native-cache",
                     "uses: mlugg/setup-zig@"):
            self.assertIn(step, mobile)
        self.assertLess(mobile.index("Unsigned iOS application"), mobile.index("Dart FFI checkpoint recovery"))
        self.assertIn("runs-on: ubuntu-24.04", android)
        self.assertIn("--test dart --timings", android)
        self.assertIn("--skip mcp_peer::stdio_peer --test-threads=4", android)
        prerequisites = android.split("      - name: Native prerequisites\n", 1)[1].split("      - ", 1)[0]
        self.assertEqual(android.count("rustup toolchain install"), 1)
        self.assertEqual(android.count("apt-get install"), 1)
        for pinned in ("postgresql-17", "'mysql-community-server-core=8.4.*'", "noble mysql-8.4-lts",
                       "BCA43417C3B485DD128EC6D4B7B3B788A8D3785C", "echo /usr/lib/postgresql/17/bin >> \"$GITHUB_PATH\""):
            self.assertIn(pinned, prerequisites)
        self.assertIn('if test "$FFI" = true; then sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0; fi',
                      prerequisites)
        self.assertIn("initdb --version | grep -q ' 17\\.'", android)
        self.assertIn("mysqld --version | grep -q ' 8\\.4\\.'", android)
        for section, title, lane in ((android, "Dart FFI contract fixtures", "mobile_ffi"),
                                     (mobile, "Dart FFI checkpoint recovery", "mobile_ffi"),
                                     (android, "ARM64 release APK", "android"),
                                     (android, "Rust boundary and Android initializer", "android"),
                                     (mobile, "Mobile source fixtures", "mobile"),
                                     (mobile, "Flutter analysis and logic tests", "mobile"),
                                     (mobile, "iOS Rust dependency boundary", "mobile"),
                                     (mobile, "Unsigned iOS application", "mobile")):
            step = section.split(f"      - name: {title}\n", 1)[1].split("      - ", 1)[0]
            self.assertIn(f"needs.source.outputs.{lane} == 'true'", step)
        contract = android.split("      - name: Dart FFI contract fixtures\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("!cancelled()", contract)
        self.assertIn("OPENSSL_NO_VENDOR: '1'", contract)
        self.assertNotIn("OPENSSL_NO_VENDOR", android.split("    steps:\n", 1)[0])
        script = (ROOT / "scripts/check-mobile-contract.sh").read_text()
        self.assertIn("scripts/ci.py check mobile-contract", script)
        self.assertNotIn("cargo test", script)
        self.assertIn("uses: ./.github/actions/rust-cache", android)
        self.assertLess(android.index("flutter build apk"), android.index("check-mobile-android.sh"))

    def test_shares_native_cache_and_limits_writes(self):
        workflow = workflow_source()
        action = (ROOT / ".github/actions/native-cache/action.yml").read_text()
        for path in ("target/sherpa-onnx-prebuilt", "target/llama-prebuilt"):
            self.assertIn(path, action)
        self.assertIn('native-key "$PACKAGE_TARGET" shared', action)
        for name in ("shared", "desktop"):
            # Archived test jobs never compile; linked native libraries travel in the archive.
            section = job_section(workflow, name)
            self.assertNotIn("native-cache", section)
            self.assertNotIn("cargo build", section)
            self.assertNotIn("cargo nextest archive", section)
        self.assertNotIn("rust-cache", job_section(workflow, "shared"))
        desktop = job_section(workflow, "desktop")
        restore = "uses: ./.github/actions/rust-cache"
        fetch = "cargo fetch --locked --target aarch64-apple-darwin"
        self.assertIn(restore, desktop)
        self.assertIn(fetch, desktop)
        self.assertLess(desktop.index(restore), desktop.index(fetch))
        self.assertLess(desktop.index(fetch), desktop.index("cargo nextest run --archive-file"))
        self.assertIn("path: ${{ steps.identity.outputs.path }}", action)
        # Compilation downloads only speech libraries, so jobs that do not package save that subset.
        for name, target in (("lint", "aarch64-apple-darwin"), ("build", "aarch64-apple-darwin"),
                             ("mobile", "aarch64-apple-darwin"), ("android", "x86_64-unknown-linux-gnu"),
                             ("windows", "x86_64-pc-windows-msvc")):
            section = job_section(workflow, name)
            self.assertIn("uses: ./.github/actions/native-cache", section)
            self.assertIn(f"target: {target}\n          libraries: speech", section)
            save = section.split("      - name: Save native libraries\n", 1)[1].split("      - ", 1)[0]
            self.assertIn("github.ref == 'refs/heads/main'", save)
            self.assertIn("steps.native-cache.outputs.hit != 'true'", save)
            self.assertNotIn("llama-prebuilt", save)
            self.assertIn("path: ${{ steps.native-cache.outputs.path }}", save)
            self.assertNotIn("always()", save)
        for name, libraries in (("release", "speech"), ("release-host", None)):
            release = (ROOT / f".github/workflows/{name}.yml").read_text()
            self.assertIn("uses: ./.workflow-tools/.github/actions/native-cache", release)
            self.assertNotIn("Save speech native libraries", release)
            self.assertEqual("libraries: speech" in release, libraries == "speech")
            save = release.split("      - name: Save native libraries\n", 1)[1].split("      - ", 1)[0]
            self.assertIn("github.ref == 'refs/heads/main'", save)
            self.assertIn("path: ${{ steps.native-cache.outputs.path }}", save)
            self.assertNotIn("always()", save)
        # Host packaging downloads the decision libraries, so its complete save follows packaging.
        host = job_section((ROOT / ".github/workflows/release-host.yml").read_text(), "sign")
        self.assertLess(host.index("name: Sign, notarize and package"), host.index("name: Save native libraries"))

    def test_caches_ghostty_inputs_for_compiling_jobs(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        action = (ROOT / ".github/actions/ghostty-cache/action.yml").read_text()
        self.assertIn("ci.py\" ghostty-key", action)
        self.assertIn("ci.py\" ghostty-source", action)
        for name in ("lint", "build", "windows", "host", "mobile", "android", "light_linux", "light_macos"):
            section = job_section(workflow, name)
            with self.subTest(job=name):
                self.assertIn("uses: ./.github/actions/ghostty-cache", section)
                self.assertIn("use-cache: false", section)
                save = section.split("      - name: Save Ghostty build inputs\n", 1)
                self.assertEqual(len(save) == 2, name not in ci.LIGHT_JOBS)
                if len(save) == 2:
                    step = save[1].split("      - ", 1)[0]
                    self.assertIn("github.ref == 'refs/heads/main'", step)
                    self.assertIn("steps.ghostty.outputs.hit != 'true'", step)
                    self.assertIn("path: ${{ steps.ghostty.outputs.path }}", step)
        for name in ("shared", "desktop"):
            self.assertNotIn("ghostty", job_section(workflow, name))

    def test_links_system_openssl_only_in_checks(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for name in ("lint", "build", "host", "light_linux", "light_macos"):
            header = job_section(workflow, name).split("    steps:\n", 1)[0]
            with self.subTest(job=name):
                self.assertIn("OPENSSL_NO_VENDOR: '1'", header)
                self.assertEqual("OPENSSL_STATIC: '1'" in header, "runs-on: macos" in header)
        for name in ("lint", "build", "light_macos"):
            self.assertIn('echo "OPENSSL_DIR=$(brew --prefix openssl@3)"', job_section(workflow, name))
        for name in ("host", "light_linux", "android"):
            self.assertIn("libssl-dev", job_section(workflow, name))
        # Mobile release builds and iOS keep the vendored library selection; only the
        # macOS Dart FFI checkpoint contract links the runner's OpenSSL.
        mobile = job_section(workflow, "mobile")
        title = "      - name: Dart FFI checkpoint recovery\n"
        checkpoints = mobile.split(title, 1)[1].split("      - ", 1)[0]
        self.assertNotIn("OPENSSL", mobile.replace(title + checkpoints, ""))
        self.assertIn("OPENSSL_STATIC: '1'", checkpoints)
        self.assertIn('export OPENSSL_DIR="$(brew --prefix openssl@3)"', checkpoints)
        for path in (ROOT / ".github/workflows").glob("release*.yml"):
            self.assertNotIn("OPENSSL", path.read_text(), path.name)

    def test_build_concurrency_matches_runner_capacity(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for name, jobs in (("build", "3"), ("lint", "2"), ("mobile", "2"), ("light_macos", "2"),
                           ("host", "4"), ("android", "4"), ("light_linux", "4"), ("windows", "4")):
            self.assertIn(f"CARGO_BUILD_JOBS: '{jobs}'", job_section(workflow, name).split("    steps:\n", 1)[0])

    def test_pins_reported_tool_versions(self):
        workflows = "".join(path.read_text() for path in (ROOT / ".github/workflows").glob("*.yml"))
        for name, pattern in (("cargo-nextest", r"tool: cargo-nextest@(\S+)"),
                              ("zig", r"(?<![\w-])version: '([^']+)'\n\s+use-cache:"),
                              ("flutter", r"flutter-version: '([^']+)'")):
            with self.subTest(tool=name):
                self.assertEqual(set(re.findall(pattern, workflows)), {ci.TOOLS[name][1]})
        self.assertIsNone(ci.TOOLS["gh"][1])
        self.assertIsNone(ci.TOOLS["rg"][1])
        ci_workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for name, tools in (("source", "gh"), ("shared", "rg cargo-nextest"), ("desktop", "rg cargo-nextest"),
                            ("windows", "zig cargo-nextest"), ("mobile", "flutter"), ("android", "flutter rg")):
            self.assertIn(f"run: python3 scripts/ci.py tools {tools}\n", job_section(ci_workflow, name))

    def test_omits_retired_source_cleanup(self):
        workflow = workflow_source()
        self.assertNotIn("prune-source-cache", workflow)
        self.assertNotIn("\n  source_cache:", workflow)
        self.assertFalse((ROOT / "scripts/prune-source-cache.py").exists())

    def test_always_builds_with_compiler_caching(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        build = job_section(workflow, "build")
        compilation = build.split("      - name: Build native Desktop\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("if: matrix.native == null", compilation)
        self.assertIn("cargo build --locked --release --timings", compilation)
        for legacy in ("reuse_build_run", "reusable-build", "key: packages-", "target/release\n"):
            self.assertNotIn(legacy, workflow)
        self.assertIn("uses: ./.workflow-tools/.github/actions/rust-cache", build)
        cache = build.split("      - name: Rust dependency caches\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("if: matrix.native == null", cache)

    def test_reuses_sources_without_extracted_duplicates(self):
        action = (ROOT / ".github/actions/rust-cache/action.yml").read_text()
        workflows = [(ROOT / f".github/workflows/{name}.yml").read_text()
                     for name in ("ci", "release", "release-host")]
        self.assertIn("path: .cargo-sources", action)
        self.assertIn("source-cache restore", action)
        self.assertNotIn("~/.cargo/registry/src", action)
        self.assertNotIn("~/.cargo/git/checkouts", action)
        self.assertIn("enableCrossOsArchive: true", action)
        identity = action.split("    - name: Dependency source identity\n", 1)[1].split("    - name:", 1)[0]
        self.assertIn("continue-on-error: true", identity)
        self.assertIn('source-key "$GITHUB_WORKSPACE/Cargo.lock"', identity)
        self.assertIn('"$GITHUB_ACTION_PATH/../../../scripts/ci.py"', identity)
        self.assertNotIn("hashFiles('Cargo.lock')", action)
        sources = action.split("    - name: Compressed dependency sources\n", 1)[1].split("    - name:", 1)[0]
        self.assertIn("if: steps.source-key.outcome == 'success'", sources)
        self.assertIn("continue-on-error: true", sources)
        self.assertIn("key: ${{ steps.source-key.outputs.key }}", sources)
        self.assertIn("restore-keys: cargo-sources-v1-", sources)
        self.assertNotIn("runner.os", action)
        self.assertIn("version: v0.18.0", action)
        compiler = action.split("    - name: Compiler cache\n", 1)[1].split("    - name:", 1)[0]
        self.assertIn("continue-on-error: true", compiler)
        fallback = action.split("    - name: Fall back to uncached compilation\n", 1)[1].split("    - name:", 1)[0]
        self.assertIn("if: steps.compiler.outcome != 'success'", fallback)
        self.assertIn("echo 'RUSTC_WRAPPER='", fallback)
        self.assertIn("'refs/heads/main' && 'READ_WRITE' || 'READ_ONLY'", action)
        self.assertNotIn("SCCACHE_GHA_CACHE_TO", action)
        self.assertNotIn("SCCACHE_CACHE_SIZE", action)
        self.assertEqual(workflows[0].count("name: Save compressed dependency sources"), 1)
        self.assertIn("run: cargo fetch --locked", workflows[0])
        self.assertIn("steps.workspace-sources.outcome == 'success'", workflows[0])
        fetch = workflows[0].split("      - name: Fetch locked workspace sources\n", 1)[1].split("      - name:", 1)[0]
        self.assertIn("steps.rust-cache.outputs.source-key != ''", fetch)
        self.assertIn("continue-on-error: true", fetch)
        for workflow in workflows:
            self.assertIn("/.github/actions/rust-cache", workflow)
            self.assertNotIn("Swatinem/rust-cache", workflow)
            self.assertNotIn("target/release\n", workflow)
            self.assertNotIn("target/host-cross\n", workflow)
            self.assertIn("scripts/ci.py cache-usage", workflow)
        for workflow in workflows[1:]:
            self.assertNotIn("Save compilation before signing", workflow)
            self.assertIn("python3 .workflow-tools/scripts/ci-run.py --parsers -- cargo build --locked --release --timings", workflow)


class SourceCacheIdentity(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.lockfile = self.root / "Cargo.lock"
        self.original = (ROOT / "Cargo.lock").read_text()

    def key(self, contents):
        self.lockfile.write_text(contents)
        return ci.source_cache_key(self.lockfile)

    def package(self, values):
        return "[[package]]\n" + "".join(f"{name} = {json.dumps(value)}\n" for name, value in values.items())

    def test_ignores_workspace_version_changes(self):
        header, *packages = self.original.split("[[package]]")
        changed = 0
        for index, contents in enumerate(packages):
            package = tomllib.loads("[[package]]" + contents)["package"][0]
            if "source" not in package:
                packages[index] = re.sub(r'(?m)^version = "[^"]+"$', 'version = "99.0.0"', contents, count=1)
                changed += 1
        self.assertGreater(changed, 0)
        updated = header + "".join("[[package]]" + contents for contents in packages)
        self.assertEqual(self.key(updated), self.key(self.original))

    def test_tracks_external_identity(self):
        package = {"name": "example", "version": "1.0.0",
                   "source": "registry+https://registry.example/index", "checksum": "a" * 64}
        original = self.key(self.package(package))
        for name, value in (("name", "replacement"), ("version", "2.0.0"),
                            ("source", "registry+https://other.example/index"), ("checksum", "b" * 64)):
            with self.subTest(field=name):
                self.assertNotEqual(self.key(self.package({**package, name: value})), original)

    def test_ignores_package_order_and_format(self):
        header, *packages = self.original.split("[[package]]")
        reordered = header + "".join("[[package]]" + contents for contents in reversed(packages))
        self.assertEqual(self.key("# Same external sources\n" + reordered), self.key(self.original))

    def test_tracks_git_sources_without_checksums(self):
        package = {"name": "example", "version": "1.0.0", "source": "git+https://git.example/repo#" + "a" * 40}
        original = self.key(self.package(package))
        package["source"] = "git+https://git.example/repo#" + "b" * 40
        self.assertNotEqual(self.key(self.package(package)), original)

    def test_reads_product_lock_from_each_tool_location(self):
        action = (ROOT / ".github/actions/rust-cache/action.yml").read_text()
        identity = action.split("    - name: Dependency source identity\n", 1)[1].split("    - name:", 1)[0]
        command = identity.split("      run: ", 1)[1].strip()
        # Match setup-python on CI even when the local system Python is older.
        command = f'python3() {{ {shlex.quote(sys.executable)} "$@"; }}\n{command}'
        other = self.package({"name": "other", "version": "1.0.0", "source": "git+https://git.example/other#123"})
        expected = self.key(self.original)
        self.assertNotEqual(self.key(other), expected)
        for separate in (False, True):
            with self.subTest(separate_tooling=separate):
                product = self.root / str(separate) / "product checkout"
                product.mkdir(parents=True)
                (product / "Cargo.lock").write_text(self.original)
                tooling = product / ".workflow-tools" if separate else product
                script = tooling / "scripts/ci.py"
                script.parent.mkdir(parents=True)
                shutil.copyfile(ROOT / "scripts/ci.py", script)
                if separate:
                    (tooling / "Cargo.lock").write_text(other)
                action_path = tooling / ".github/actions/rust-cache"
                action_path.mkdir(parents=True)
                working = product / "different working directory"
                working.mkdir()
                (working / "Cargo.lock").write_text(other)
                output = product / "output"
                result = subprocess.run(
                    ["bash", "--noprofile", "--norc", "-e", "-c", command],
                    cwd=working, capture_output=True, text=True,
                    env={**os.environ, "GITHUB_WORKSPACE": str(product),
                         "GITHUB_ACTION_PATH": str(action_path), "GITHUB_OUTPUT": str(output)},
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected + "\n")
                self.assertEqual(output.read_text(), f"key={expected}\n")


class CacheUsage(unittest.TestCase):
    def test_rebuilds_readonly_stage_without_changing_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace = root / "workspace"
            workspace.mkdir()
            home = root / ".cargo"
            relative = Path("git/db/example/objects/pack/data.pack")
            source = home / relative
            source.parent.mkdir(parents=True)
            source.write_bytes(b"current pack")
            source.chmod(0o444)
            sentinel = home / "registry/src/keep.rs"
            sentinel.parent.mkdir(parents=True)
            sentinel.write_bytes(b"unpacked source")
            ci.cache_sources("save", workspace, home)
            archive = workspace / ".cargo-sources"
            stale = archive / "git/db/retired/objects/pack/old.pack"
            stale.parent.mkdir(parents=True)
            stale.write_bytes(b"obsolete pack")
            stale.chmod(0o444)
            source_mode = source.stat().st_mode
            ci.cache_sources("save", workspace, home)
            self.assertEqual((archive / relative).read_bytes(), b"current pack")
            self.assertFalse((archive / "git/db/retired").exists())
            self.assertEqual(source.read_bytes(), b"current pack")
            self.assertEqual(source.stat().st_mode, source_mode)
            self.assertEqual(sentinel.read_bytes(), b"unpacked source")

    def test_preserves_cargo_home_when_it_is_stage(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory)
            home = workspace / ".cargo-sources"
            source = home / "git/db/example/objects/pack/data.pack"
            source.parent.mkdir(parents=True)
            source.write_bytes(b"original pack")
            with self.assertRaises(ValueError):
                ci.cache_sources("save", workspace, home)
            self.assertEqual(source.read_bytes(), b"original pack")

    def test_restores_sources_across_runner_homes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = root / "macos/.cargo"
            workspace = root / "workspace"
            workspace.mkdir()
            for relative in ("registry/index/crates/.cache/ex/am/example",
                             "registry/index/crates/config.json", "registry/cache/crates/example-1.0.crate",
                             "git/db/example/objects/pack/data.pack", "registry/src/example/src.rs",
                             "git/checkouts/example/src.rs"):
                path = first / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"fixture")
            metadata = b'\x03\x02\0\0\0etag: "fixture"\0' + b'{"name":"example","vers":"1.0.0"}\0'
            (first / "registry/index/crates/.cache/ex/am/example").write_bytes(metadata)
            ci.cache_sources("save", workspace, first)
            self.assertFalse((workspace / ".cargo-sources/registry/src").exists())
            self.assertFalse((workspace / ".cargo-sources/git/checkouts").exists())
            second = root / "linux/.cargo"
            sentinel = second / "registry/src/keep.rs"
            sentinel.parent.mkdir(parents=True)
            sentinel.write_bytes(b"unpacked source")
            ci.cache_sources("restore", workspace, second)
            self.assertEqual(sentinel.read_bytes(), b"unpacked source")
            self.assertEqual((second / "registry/index/crates/.cache/ex/am/example").read_bytes(), metadata)
            self.assertEqual((second / "registry/index/crates/config.json").read_bytes(), b"fixture")
            self.assertEqual((second / "registry/cache/crates/example-1.0.crate").read_bytes(), b"fixture")
            self.assertEqual((second / "git/db/example/objects/pack/data.pack").read_bytes(), b"fixture")

    def test_reports_usage_without_mutation(self):
        usage = {"active_caches_size_in_bytes": 10 * 2**30, "active_caches_count": 6966}
        with patch.dict(os.environ, {"GH_REPO": "fixture/product", "GITHUB_STEP_SUMMARY": ""}), \
                patch.object(ci.subprocess, "run", return_value=SimpleNamespace(stdout=json.dumps(usage))) as run, \
                patch("builtins.print") as output:
            ci.cache_usage()
        run.assert_called_once_with(
            ["gh", "api", "repos/fixture/product/actions/cache/usage"],
            check=True, text=True, capture_output=True,
        )
        output.assert_called_once_with("Repository cache: 10.000 GiB (6966 entries)")


class PhaseProgress(unittest.TestCase):
    def module(self):
        spec = importlib.util.spec_from_file_location("ci_run", ROOT / "scripts/ci-run.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_preserves_command_result_and_arguments(self):
        module = self.module()
        command = [sys.executable, "-c", "import sys; sys.exit(23)"]
        with patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": ""}), patch("builtins.print"):
            self.assertEqual(module.run(command), 23)

    def test_observes_live_phase_without_restarting_it(self):
        module = self.module()
        process = Mock()
        process.wait.side_effect = [subprocess.TimeoutExpired(["cargo"], 60), 0]
        process.__enter__ = Mock(return_value=process)
        process.__exit__ = Mock(return_value=False)
        with patch.object(module.subprocess, "Popen", return_value=process) as spawn, \
                patch.object(module, "progress") as progress, \
                patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": ""}), patch("builtins.print"):
            self.assertEqual(module.run(["cargo", "build", "--locked"]), 0)
        spawn.assert_called_once_with(["cargo", "build", "--locked"], env=None)
        progress.assert_called_once()


class SpeechCache(unittest.TestCase):
    def fixture(self, root, requirement="=1.2.3", system="1.2.3"):
        (root / "Cargo.toml").write_text(
            "[workspace]\n[workspace.dependencies]\n"
            f'sherpa-onnx = {{ version = "{requirement}", default-features = false }}\n',
            encoding="utf-8",
        )
        (root / "Cargo.lock").write_text(
            "version = 4\n\n"
            '[[package]]\nname = "sherpa-onnx"\nversion = "1.2.3"\n\n'
            f'[[package]]\nname = "sherpa-onnx-sys"\nversion = "{system}"\n',
            encoding="utf-8",
        )

    def test_uses_versions_target_and_link_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            self.assertEqual(
                ci.speech_cache_key(root, "aarch64-apple-darwin", "shared"),
                "speech-native-v1-sherpa-1.2.3-sys-1.2.3-aarch64-apple-darwin-shared",
            )
            before = ci.speech_cache_key(root, "aarch64-apple-darwin", "shared")
            with (root / "Cargo.lock").open("a", encoding="utf-8") as lock:
                lock.write('\n[[package]]\nname = "unrelated"\nversion = "9.9.9"\n')
            self.assertEqual(ci.speech_cache_key(root, "aarch64-apple-darwin", "shared"), before)
            self.fixture(root, system="1.2.4")
            self.assertIn("sys-1.2.4", ci.speech_cache_key(root, "x86_64-unknown-linux-gnu", "shared"))

    def test_native_key_tracks_llama_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            lock = root / "scripts/package/llama-runtime.lock.json"
            lock.parent.mkdir(parents=True)
            lock.write_text('{"revision":"first"}')
            first = ci.native_cache_key(root, "aarch64-apple-darwin", "shared")
            self.assertNotEqual(first, ci.native_cache_key(root, "x86_64-apple-darwin", "shared"))
            self.assertNotEqual(first, ci.native_cache_key(root, "aarch64-apple-darwin", "static"))
            lock.write_text('{"revision":"second"}')
            self.assertNotEqual(first, ci.native_cache_key(root, "aarch64-apple-darwin", "shared"))

    def test_requires_exact_workspace_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, requirement="1.2.3")
            with self.assertRaisesRegex(ValueError, "must remain pinned"):
                ci.speech_cache_key(root, "aarch64-apple-darwin", "shared")


class Profiles(unittest.TestCase):
    def test_retries_only_integration_binaries(self):
        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text())
        for name in ("ci", "databases", "lifecycle"):
            with self.subTest(profile=name):
                profile = config["profile"][name]
                self.assertEqual(profile["retries"], 0)
                retried = [override for override in profile.get("overrides", []) if "retries" in override]
                self.assertEqual(retried, [{"filter": "kind(test)", "retries": 1}])
                self.assertFalse(profile["fail-fast"])
                self.assertNotIn("default-filter", profile)
                self.assertEqual(
                    profile["slow-timeout"], {"period": "60s", "terminate-after": 5},
                )

    def test_reports_tests_that_pass_on_retry(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for name, reports in (("shared", {"ci": "tests", "databases": "databases"}),
                              ("desktop", {"ci": "tests", "lifecycle": "lifecycle"}),
                              ("windows", {"ci": "tests"}), ("host", {"ci": "tests"})):
            job = job_section(workflow, name)
            step = job.split("      - name: Flaky tests\n", 1)[1].split("      - ", 1)[0]
            command = " ".join(step.split("run:", 1)[1].replace(">-", "").split())
            expected = " ".join(f'"target/nextest/{profile}/junit.xml=${{{{ steps.{step_id}.outcome }}}}"'
                                for profile, step_id in reports.items())
            with self.subTest(job=name):
                self.assertIn("if: always()", step)
                self.assertEqual(command, f"python3 scripts/ci.py flaky {expected}")
                for step_id in reports.values():
                    self.assertIn(f"        id: {step_id}\n", job)


class FlakyReport(unittest.TestCase):
    REPORT = """<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" failures="0" errors="0">
  <testsuite name="sailry-node-runtime::agent" tests="3" failures="0" errors="0">
    <testcase name="delegation::claim::preserves_fifo" classname="sailry-node-runtime::agent" time="1.2">
      <flakyFailure message="test failure" type="test failure">attempt 1</flakyFailure>
    </testcase>
    <testcase name="paging::restores_turns" classname="sailry-node-runtime::agent" time="0.4"/>
    <testcase name="hooks::bounds_processes" classname="sailry-node-runtime::agent" time="0.1">
      <failure message="test failure" type="test failure"/>
      <rerunFailure message="test failure" type="test failure"/>
    </testcase>
  </testsuite>
</testsuites>
"""

    def test_lists_only_tests_that_passed_on_retry(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "junit.xml"
            report.write_text(self.REPORT)
            summary = Path(directory) / "summary.md"
            with patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": str(summary), "GITHUB_ACTIONS": "true"}), \
                    patch("builtins.print") as output:
                flaky = ci.report_flaky([report, Path(directory) / "missing.xml"])
            self.assertEqual(flaky, [("sailry-node-runtime::agent", "delegation::claim::preserves_fifo", 1)])
            printed = [call.args[0] for call in output.call_args_list]
            self.assertIn("::warning title=Flaky test::sailry-node-runtime::agent "
                          "delegation::claim::preserves_fifo passed after 1 failed attempt(s)", printed)
            self.assertIn("`sailry-node-runtime::agent delegation::claim::preserves_fifo`", summary.read_text())
            self.assertNotIn("hooks::bounds_processes", summary.read_text())

    def test_quiet_without_flaky_tests(self):
        with tempfile.TemporaryDirectory() as directory:
            summary = Path(directory) / "summary.md"
            with patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": str(summary)}), patch("builtins.print"):
                self.assertEqual(ci.report_flaky([Path(directory) / "missing.xml"]), [])
            self.assertFalse(summary.exists())

    def test_prints_plain_text_outside_actions(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "junit.xml"
            report.write_text(self.REPORT)
            environment = {name: value for name, value in os.environ.items()
                           if name not in {"GITHUB_ACTIONS", "GITHUB_STEP_SUMMARY"}}
            with patch.dict(os.environ, environment, clear=True), patch("builtins.print") as output:
                ci.report_flaky([report])
            self.assertIn("Flaky test: sailry-node-runtime::agent delegation::claim::preserves_fifo "
                          "passed after 1 failed attempt(s)", [call.args[0] for call in output.call_args_list])

    def test_reports_missing_after_test_step(self):
        with tempfile.TemporaryDirectory() as directory:
            present = Path(directory) / "junit.xml"
            present.write_text(self.REPORT)
            missing = Path(directory) / "missing.xml"
            cases = (
                ({missing: "success"}, True, "::error title=Missing test report::"),
                ({missing: "failure"}, False, "::warning title=Missing test report::"),
                ({missing: "skipped", present: "success"}, False, None),
            )
            for outcomes, failed, prefix in cases:
                with self.subTest(outcomes=list(outcomes.values())):
                    with patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}), patch("builtins.print") as output:
                        self.assertEqual(ci.report_missing(outcomes), failed)
                    printed = [call.args[0] for call in output.call_args_list]
                    if prefix is None:
                        self.assertEqual(printed, [])
                    else:
                        self.assertEqual(len(printed), 1)
                        self.assertTrue(printed[0].startswith(prefix + str(missing)))

    def test_fails_when_passing_step_left_no_report(self):
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "missing.xml"
            for outcome, code in (("success", 1), ("failure", 0), ("cancelled", 0)):
                with self.subTest(outcome=outcome):
                    result = subprocess.run([sys.executable, str(ROOT / "scripts/ci.py"), "flaky", f"{missing}={outcome}"],
                                            capture_output=True, text=True, env={**os.environ, "GITHUB_STEP_SUMMARY": ""})
                    self.assertEqual(result.returncode, code, result.stdout + result.stderr)


class ToolVersions(unittest.TestCase):
    def outputs(self, values):
        def run(command, **options):
            name = next(name for name, (argv, _) in ci.TOOLS.items() if tuple(command) == argv)
            if values.get(name) is None:
                raise FileNotFoundError(command[0])
            return SimpleNamespace(stdout=values[name])
        return run

    def test_parses_each_tool_banner(self):
        banners = {"cargo-nextest": "cargo-nextest 0.9.143 (abc 2026-10-01)\n", "zig": "0.16.0\n",
                   "flutter": '{"frameworkVersion": "3.44.7", "dartSdkVersion": "3.11.0"}\n',
                   "rg": "ripgrep 14.1.0\n\nfeatures:+pcre2\n", "gh": "gh version 2.96.0 (2026-07-02)\n"}
        with patch.object(ci.subprocess, "run", side_effect=self.outputs(banners)):
            versions = ci.tool_versions(ci.TOOLS)
        self.assertEqual(versions, {"cargo-nextest": "0.9.143", "zig": "0.16.0", "flutter": "3.44.7",
                                    "rg": "14.1.0", "gh": "2.96.0"})
        self.assertEqual(ci.tool_mismatches(versions), [])

    def test_rejects_only_pinned_differences(self):
        versions = {"cargo-nextest": "0.9.140", "zig": None, "flutter": "3.44.7", "rg": "13.0.0", "gh": None}
        self.assertEqual(ci.tool_mismatches(versions), [
            "cargo-nextest 0.9.140 (CI uses 0.9.143)", "zig unavailable (CI uses 0.16.0)",
        ])

    def test_reports_versions_for_ci(self):
        with tempfile.TemporaryDirectory() as directory:
            summary = Path(directory) / "summary.md"
            with patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": str(summary)}), \
                    patch.object(ci.subprocess, "run", side_effect=self.outputs({"gh": "gh version 2.96.0\n"})), \
                    patch("builtins.print"):
                self.assertTrue(ci.report_tools(["gh"]))
                self.assertFalse(ci.report_tools(["zig"]))
            self.assertIn("Tool versions: gh: 2.96.0", summary.read_text())

    def test_plans_only_tools_its_commands_use(self):
        commands = [{"argv": argv} for argv in (["cargo", "fmt", "--all"], [sys.executable, "scripts/check-names.py"],
                                                ["cargo", "clippy", "--locked"], ["flutter", "test"])]
        self.assertEqual(ci.planned_tools(commands), ["flutter", "zig"])
        self.assertEqual(ci.planned_tools([{"argv": ["cargo", "nextest", "run"]}]), ["cargo-nextest", "zig"])


class GhosttyInputs(unittest.TestCase):
    def git(self, *arguments, cwd):
        return subprocess.run(["git", *arguments], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()

    def repository(self, root, name, files):
        path = root / name
        path.mkdir()
        self.git("init", "--quiet", "--initial-branch=main", "--template=", cwd=path)
        self.git("config", "user.email", "fixture@example.invalid", cwd=path)
        self.git("config", "user.name", "Fixture", cwd=path)
        self.git("config", "uploadpack.allowAnySHA1InWant", "true", cwd=path)
        for relative, text in files.items():
            (path / relative).parent.mkdir(parents=True, exist_ok=True)
            (path / relative).write_text(text)
        self.git("add", ".", cwd=path)
        self.git("commit", "--quiet", "-m", "Fixture", cwd=path)
        return path, self.git("rev-parse", "HEAD", cwd=path)

    def lockfile(self, root, source):
        (root / "Cargo.lock").write_text(f'version = 4\n\n[[package]]\nname = "libghostty-vt-sys"\n'
                                         f'version = "0.2.1"\nsource = "{source}"\n')

    def test_keys_by_runner_zig_and_locked_revision(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            revision = "b" * 40
            self.lockfile(root, f"git+https://github.com/example/libghostty-rs.git?rev={revision}#{revision}")
            self.assertEqual(ci.ghostty_cache_key(root, "macOS", "ARM64", "0.16.0"),
                             f"ghostty-v1-macOS-ARM64-zig-0.16.0-{revision}")
            for invalid in ("git+https://github.com/example/libghostty-rs.git?branch=main#" + revision,
                            f"git+https://github.com/example/libghostty-rs.git?rev={revision}#{'c' * 40}"):
                self.lockfile(root, invalid)
                with self.subTest(source=invalid), self.assertRaises(ValueError):
                    ci.ghostty_revision(root)
            with self.assertRaises(ValueError):
                ci.ghostty_cache_key(root, "mac OS", "ARM64", "0.16.0")

    def test_checks_out_pinned_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ghostty, commit = self.repository(root, "ghostty", {"build.zig": "// pinned\n"})
            (ghostty / "build.zig").write_text("// later\n")
            self.git("commit", "--quiet", "-am", "Later", cwd=ghostty)
            script = (f'const GHOSTTY_REPO: &str = "{ghostty.as_uri()}";\n'
                      f'const GHOSTTY_COMMIT: &str = "{commit}";\n'
                      'fn main() { let _ = env::var("GHOSTTY_SOURCE_DIR"); }\n')
            bindings, revision = self.repository(root, "bindings", {"crates/libghostty-vt-sys/build.rs": script})
            workspace = root / "workspace"
            workspace.mkdir()
            self.lockfile(workspace, f"git+{bindings.as_uri()}?rev={revision}#{revision}")
            source = workspace / ci.GHOSTTY_CACHE / "source"
            with patch.object(ci.subprocess, "run", wraps=subprocess.run) as run:
                self.assertTrue(ci.ghostty_source(workspace, source))
            self.assertTrue(all(call.kwargs.get("timeout") == ci.GHOSTTY_TIMEOUT for call in run.call_args_list))
            self.assertEqual(self.git("rev-parse", "HEAD", cwd=source), commit)
            self.assertEqual((source / "build.zig").read_text(), "// pinned\n")
            self.assertFalse(source.with_name("source.partial").exists())
            # A restored checkout is reused without contacting either repository.
            with patch.object(ci.subprocess, "run", side_effect=AssertionError("unexpected fetch")):
                self.assertFalse(ci.ghostty_source(workspace, source))

    def test_requires_source_override(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ghostty, commit = self.repository(root, "ghostty", {"build.zig": "// pinned\n"})
            script = (f'const GHOSTTY_REPO: &str = "{ghostty.as_uri()}";\n'
                      f'const GHOSTTY_COMMIT: &str = "{commit}";\n'
                      'fn main() { let _ = env::var("GHOSTTY_SOURCE_PATH"); }\n')
            bindings, revision = self.repository(root, "bindings", {"crates/libghostty-vt-sys/build.rs": script})
            workspace = root / "workspace"
            workspace.mkdir()
            self.lockfile(workspace, f"git+{bindings.as_uri()}?rev={revision}#{revision}")
            source = workspace / ci.GHOSTTY_CACHE / "source"
            with self.assertRaisesRegex(ValueError, "no longer reads GHOSTTY_SOURCE_DIR"):
                ci.ghostty_source(workspace, source)
            self.assertFalse(source.exists())
            self.assertFalse(source.with_name("source.partial").exists())

    def test_exports_build_environment(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory).resolve()
            cache = workspace / ci.GHOSTTY_CACHE
            (cache / "source").mkdir(parents=True)
            (cache / "source/build.zig").write_text("// cached\n")
            environment = workspace / "github.env"
            result = subprocess.run([sys.executable, str(ROOT / "scripts/ci.py"), "ghostty-source"],
                                    capture_output=True, text=True,
                                    env={**os.environ, "GITHUB_WORKSPACE": str(workspace), "GITHUB_ENV": str(environment)})
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(environment.read_text(),
                             f"GHOSTTY_SOURCE_DIR={cache / 'source'}\nZIG_GLOBAL_CACHE_DIR={cache / 'zig'}\n")
            self.assertTrue((cache / "zig").is_dir())


if __name__ == "__main__":
    unittest.main()
