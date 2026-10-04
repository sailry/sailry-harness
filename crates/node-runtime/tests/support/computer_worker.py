#!/usr/bin/env python3
"""Deterministic upstream channel fixture; never creates a native runtime."""
import json
import os
from pathlib import Path
import sys

root = Path(__file__).parent
outcomes = json.loads((root / "outcomes.json").read_text())
handles = {}
next_handle = 1


def result(value):
    return {
        "content": [{"type": "text", "text": json.dumps(value, separators=(",", ":"))}],
        "structuredContent": value,
        "isError": False,
    }


def invoke(request):
    bound = handles[request["session_handle"]]
    arguments = request["arguments"].copy()
    supplied = arguments.get("session")
    if isinstance(supplied, str) and supplied != bound:
        message = "public session substitution does not match the bound authorization context"
        return {
            "content": [{"type": "text", "text": message}],
            "isError": True,
            "structuredContent": {
                "status": "refused",
                "refusal": {"code": "permission_denied", "message": message},
            },
        }
    arguments["session"] = bound
    with (root / "requests.jsonl").open("a") as records:
        records.write(json.dumps({
            "operation": "invoke", "tool": request["name"],
            "arguments": arguments, "session_handle": request["session_handle"],
        }) + "\n")
    if request["name"] in outcomes:
        return outcomes[request["name"]]
    if request["name"] == "end_session":
        return result({"session": bound, "active": False})
    if request["name"] == "click":
        return result({
            "effect": "unverifiable",
            "route": "synthetic_events",
            "delivery": {"mode": "background"},
        })
    return result({"ok": True})


for line in sys.stdin:
    request = json.loads(line)
    with (root / "requests.jsonl").open("a") as records:
        records.write(json.dumps(request) + "\n")
    response = {
        "protocol_version": request["protocol_version"],
        "request_id": request["request_id"],
        "generation": request["generation"],
        "ok": True,
        "completion": "completed",
    }
    operation = request["operation"]
    if operation == "initialize":
        value = {
            "ready": True,
            "pid": os.getpid(),
            "host_bundle_id": request["arguments"]["host_bundle_id"],
        }
    elif operation == "bind_session":
        handle = f"fixture-{next_handle}"
        next_handle += 1
        handles[handle] = request["arguments"]["public_session"]
        value = {"session_handle": handle}
    elif operation == "close_session":
        handles.pop(request.get("session_handle"), None)
        value = {"closed": True}
    elif operation == "sessions_list":
        value = {"count": len(handles), "sessions": []}
    elif operation == "shutdown":
        value = {"shutdown": True}
    elif operation == "call" and request.get("session_handle") in handles:
        value = invoke(request)
    else:
        response.update(
            ok=False,
            completion="not_started",
            error="Unknown fixture operation or session handle",
            error_code="invalid_session",
        )
        value = None
    if value is not None:
        response["result"] = value
    print(json.dumps(response), flush=True)
    if operation == "shutdown":
        break
