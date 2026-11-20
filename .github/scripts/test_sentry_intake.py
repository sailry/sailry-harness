from datetime import UTC, datetime, timedelta
import io
import json
import unittest
from unittest.mock import patch
from urllib.parse import parse_qs, urlsplit

import sentry_intake as intake


NOW = datetime(2026, 11, 20, 12, 17, tzinfo=UTC)
SHA = "e" * 40
ROW = {"issue": "SAILRY-1A", "release": "sailry@0.2.4+" + SHA, "project.id": 123,
       "timestamp": "2026-11-20T11:54:00Z"}


class PollTest(unittest.TestCase):
    def collect(self, pages, now=NOW):
        queries = []

        def fetch(path, token):
            url = urlsplit(path)
            self.assertEqual((url.scheme, url.netloc, url.path), ("", "", "/api/0/organizations/example/events/"))
            query = parse_qs(url.query)
            self.assertEqual(query["dataset"], ["errors"])
            self.assertEqual(query["project"], ["123"])
            self.assertEqual(query["field"], ["timestamp", "project.id", "issue", "release"])
            self.assertEqual(query["per_page"], [str(intake.PAGE_SIZE)])
            self.assertEqual(token, "test-only")
            queries.append(query["query"][0])
            return {"data": pages[len(queries) - 1]}

        result = intake.collect("example", "123", "test-only", now, fetch)
        self.assertEqual(len(queries), 3)
        self.assertIn('error.value:"command.guide_queued_turn:conflict" AND sailry.reason_code:recipient_stopping', queries[0])
        self.assertIn("(!has:sailry.reason_code OR !sailry.reason_code:recipient_stopping)", queries[1])
        self.assertIn('error.value:"desktop.workspace_action_failed:*"', queries[2])
        return result

    def test_handoff_only_the_reviewed_route_and_discard_untrusted_values(self):
        raw = '::error:: private source\nNEW_OUTPUT=1'
        row = dict(ROW, message=raw)
        outputs, summary = self.collect([[row, row, dict(row, release=raw)], [row], [row, dict(row, issue=raw)]])
        self.assertEqual(len(outputs), 1)
        self.assertEqual(outputs[0]["target"], SHA)
        self.assertEqual(outputs[0]["template"], "queue-stopping")
        self.assertRegex(outputs[0]["fingerprint"], r"^[0-9a-f]{64}$")
        self.assertEqual(set(outputs[0]), {"fingerprint", "target", "template"})
        self.assertIn("other queue conflicts: 1; workspace observations: 1; invalid groups: 1", summary)
        self.assertNotIn(raw, summary + json.dumps(outputs))
        self.assertNotIn("SAILRY-1A", summary + json.dumps(outputs))
        self.assertEqual(self.collect([[], [row], [row]])[0], [])

    def test_fixed_window_limits_and_no_catchup(self):
        rows = [dict(ROW, issue=f"SAILRY-{i}") for i in range(intake.PAGE_SIZE)]
        outputs, summary = self.collect([rows, [], []])
        self.assertEqual(len(outputs), intake.MAX_DISPATCH)
        self.assertIn("Page limit reached", summary)
        self.assertEqual(self.collect([[dict(ROW, timestamp="2026-11-20T12:00:00Z")], [], []])[0], [])
        self.assertEqual(self.collect([rows, [], []], NOW + timedelta(hours=1))[0], [])
        with self.assertRaises(ValueError):
            self.collect([rows + [ROW], [], []])

    def test_no_unscoped_request(self):
        for org, project in [("a/b", "123"), ("example", "-1"), ("example", "1&project=2")]:
            with self.assertRaises(ValueError):
                intake.collect(org, project, "test-only", NOW, lambda *_: self.fail("unexpected request"))
        with self.assertRaises(ValueError):
            self.collect([[dict(ROW, **{"project.id": "999"})], [], []])

    @patch("sentry_intake.http.client.HTTPSConnection")
    def test_redirects_links_and_response_limits(self, connection):
        client = connection.return_value
        for status, body, valid in [(302, b"", False), (503, b"private error body", False),
                                    (200, b"x" * (intake.MAX_BODY + 1), False), (200, b'{"data":[]}', True)]:
            connection.reset_mock()
            response = client.getresponse.return_value
            response.status = status
            response.headers = {"Location": "https://elsewhere.invalid/private", "Link": '<https://elsewhere.invalid/>; rel="next"'}
            response.read.side_effect = io.BytesIO(body).read
            if valid:
                self.assertEqual(intake.get_page("/fixed", "dummy"), {"data": []})
            else:
                with self.assertRaises(ValueError):
                    intake.get_page("/fixed", "dummy")
            connection.assert_called_once_with("sentry.io", timeout=15)
            client.request.assert_called_once_with("GET", "/fixed", headers={"Authorization": "Bearer dummy", "Accept": "application/json"})
            client.close.assert_called_once()


if __name__ == "__main__":
    unittest.main()
