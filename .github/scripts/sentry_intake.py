import hashlib
import http.client
import json
import os
import re
import sys
from collections import Counter
from datetime import UTC, datetime, timedelta
from urllib.parse import urlencode


HOST = "sentry.io"
PAGE_SIZE = 20
MAX_DISPATCH = 2
MAX_BODY = 64 * 1024
SOURCE = ('platform:native AND application:desktop AND error.type:SailryError '
          'AND (product:harness OR (!has:product AND release:sailry@*))')
QUEUE = 'error.value:"command.guide_queued_turn:conflict"'
WORKSPACE = ["workspace_action_failed", "save_all_documents", "edit_markdown_source", "search_files", "write_file", "list_directory", "inspect_git", "list_worktrees"]
QUERIES = {
    "queue-stopping": QUEUE + " AND sailry.reason_code:recipient_stopping",
    "queue-unclassified": QUEUE + " AND (!has:sailry.reason_code OR !sailry.reason_code:recipient_stopping)",
    "workspace-unclassified": "(" + " OR ".join(f'error.value:"desktop.{op}:*"' for op in WORKSPACE) + ")",
}
FIELDS = ["timestamp", "project.id", "issue", "release", "product"]
RELEASE = re.compile(r"sailry@v?[0-9]{1,6}\.[0-9]{1,6}\.[0-9]{1,6}(?:-[a-zA-Z0-9.-]{1,64})?\+([0-9a-f]{40})")


def get_page(path, token):
    # Direct HTTPS only: neither Location nor Link can change the destination.
    conn = http.client.HTTPSConnection(HOST, timeout=15)
    try:
        conn.request("GET", path, headers={"Authorization": "Bearer " + token, "Accept": "application/json"})
        response = conn.getresponse()
        if response.status != 200:
            raise ValueError("Sentry rejected query")
        body = response.read(MAX_BODY + 1)
        if len(body) > MAX_BODY:
            raise ValueError("oversize response")
        return json.loads(body)
    finally:
        conn.close()


def collect(org, project, token, now, fetch=get_page):
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,62}", org) or not re.fullmatch(r"[1-9][0-9]{0,19}", project) or not token:
        raise ValueError("invalid intake configuration")
    # Sample the last closed hour with five minutes of indexing grace. Never
    # backfill missed windows or accept a caller-selected time range or cursor.
    end = (now - timedelta(minutes=5)).replace(minute=0, second=0, microsecond=0)
    start = end - timedelta(hours=1)
    groups = Counter()
    counts = Counter()
    full = []
    for kind, predicate in QUERIES.items():
        query = urlencode({"dataset": "errors", "project": project, "field": FIELDS,
                           "query": SOURCE + " AND " + predicate, "sort": "-timestamp",
                           "start": start.isoformat(), "end": end.isoformat(), "per_page": PAGE_SIZE}, doseq=True)
        page = fetch(f"/api/0/organizations/{org}/events/?{query}", token)
        rows = page["data"]
        if not isinstance(rows, list) or len(rows) > PAGE_SIZE:
            raise ValueError("invalid page")
        if len(rows) == PAGE_SIZE:
            full.append(kind)
        for row in rows:
            if not isinstance(row, dict) or str(row.get("project.id")) != project:
                raise ValueError("unexpected project")
            seen = datetime.fromisoformat(row["timestamp"])
            if not start <= seen < end:
                continue
            issue = row.get("issue")
            release = row.get("release")
            matched = isinstance(release, str) and RELEASE.fullmatch(release)
            product = row.get("product")
            if product != "harness" and not (product in (None, "") and matched):
                continue
            if not isinstance(issue, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,80}-[A-Za-z0-9]{1,16}", issue):
                counts["invalid group"] += 1
                continue
            group = hashlib.sha256(f"sentry:{org}:{project}:{issue}".encode()).hexdigest()
            target = matched.group(1) if matched else None
            groups[kind, group, target] += 1
            counts[kind] += 1
    selected = [{"template": kind, "fingerprint": group, "target": target}
                for kind, group, target in groups if kind == "queue-stopping" and target][:MAX_DISPATCH]
    summary = [f"Sentry sample: {start:%Y-%m-%d %H:%M} to {end:%H:%M} UTC (end excluded).",
               f"Queue candidates: {counts['queue-stopping']}; other queue conflicts: {counts['queue-unclassified']}; "
               f"workspace observations: {counts['workspace-unclassified']}; invalid groups: {counts['invalid group']}.",
               f"Selected for verification: {len(selected)} (limit {MAX_DISPATCH}). Unclassified observations are diagnostic only."]
    if full:
        summary.append("Page limit reached (no pagination): " + ", ".join(full) + ".")
    for (kind, group, target), count in groups.items():
        summary.append(f"- {kind}: `{group}`, revision `{target or 'unknown'}`, sampled events: {count}")
    return selected, "\n".join(summary)


def main():
    selected, summary = collect(os.environ["SENTRY_ORG"], os.environ["SENTRY_PROJECT_ID"],
                                os.environ["SENTRY_READ_TOKEN"], datetime.now(UTC))
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as stream:
        print(summary, file=stream)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
        print("candidates=" + json.dumps(selected, separators=(",", ":")), file=stream)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, TypeError, KeyError, OSError, RecursionError, http.client.HTTPException):
        sys.exit("Sentry intake failed; check configuration, API availability and response shape.")
