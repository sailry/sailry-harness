import os
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import sentry_repair as repair


class RepairTest(unittest.TestCase):
    def test_dispatch_contract(self):
        good = dict(fingerprint="a" * 64, template="queue-stopping", target="v0.2.4")
        for name, key in [("workflow_dispatch", "inputs"), ("repository_dispatch", "client_payload")]:
            event = {key: good, "action": "sentry-repair"}
            self.assertEqual(repair.fields(name, event), good)
            for change in [dict(ref="main"), dict(test="TestOther"), dict(publish=True),
                           dict(target="refs/pull/1/head"), dict(target="a" * 10),
                           dict(target="v1.0.0\n"), dict(fingerprint=[]), dict(template="other")]:
                with self.subTest(name=name, change=change), self.assertRaises(ValueError):
                    repair.fields(name, {**event, key: {**good, **change}})

    def test_requires_named_test_and_exact_assertion(self):
        def events(result, assertion=None):
            rows = [{"Package": repair.PACKAGE, "Test": repair.TEST, "Action": "run"}]
            if assertion:
                rows.append({**rows[0], "Action": "output", "Output": f"    {repair.PROBE.name}:42: {assertion}\n"})
            return rows + [{**rows[0], "Action": result}, {"Package": repair.PACKAGE, "Action": result}]

        self.assertEqual(repair.classify(1, events("fail", repair.ASSERTION)), "red")
        self.assertEqual(repair.classify(0, events("pass")), "green")
        for code, rows in [(1, events("fail", "fixture did not invoke SendQueued")),
                           (0, events("skip")), (0, events("pass")[-1:]),
                           (1, [{"Action": "build-fail"}]), (1, events("fail", repair.ASSERTION)[:-1])]:
            self.assertEqual(repair.classify(code, rows), "inconclusive")

    def test_only_target_red_base_red_candidate_green_is_eligible(self):
        for states, candidate, expected in [(["red", "red", "green"], True, True),
                                             (["red", "red", "red"], True, False),
                                             (["red", "red", "inconclusive"], True, False),
                                             (["red", "red"], False, False),
                                             (["red", "green"], True, False),
                                             (["inconclusive"], True, False), (["green"], True, False)]:
            with self.subTest(states=states), TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                probe, fix = root / "probe", root / "candidate.patch"
                probe.touch()
                if candidate:
                    fix.touch()
                def git(*args, cwd=None):
                    return {"apply": "", "diff": "internal/desktop/live/conversation.go",
                            "write-tree": "1" * 40, "rev-parse": "2" * 40}[args[0]]
                with (patch.dict(os.environ, GITHUB_SHA="1" * 40, TARGET="2" * 40, RUNNER_TEMP=tmp),
                      patch.object(repair, "PROBE", probe), patch.object(repair, "PATCH", fix),
                      patch.object(repair, "git", git), patch.object(repair, "checkout"),
                      patch.object(repair, "matches_index", return_value=True),
                      patch.object(repair, "test", side_effect=states) as test,
                      patch.object(repair, "note"), patch.object(repair, "append") as emit):
                    repair.verify()
                self.assertEqual(test.call_count, len(states))
                self.assertTrue(all(call.args[1] == probe for call in test.call_args_list))
                if expected:
                    emit.assert_called_once_with("GITHUB_OUTPUT", "tree=" + "1" * 40)
                else:
                    emit.assert_not_called()


if __name__ == "__main__":
    unittest.main()
