"""An actual stdio MCP fixture with no external Python dependencies."""
import json
import os
import sys

trace = os.path.join(os.environ["PLUGIN_DATA"], "python-trace.jsonl")
with open(trace, "w", encoding="utf-8") as stream:
    stream.write(json.dumps({"python": sys.executable,
                             "version": list(sys.version_info[:3])}) + "\n")

count = 0
for line in sys.stdin:
    request = json.loads(line)
    if count < 16:
        with open(trace, "a", encoding="utf-8") as stream:
            stream.write(json.dumps({"method": request.get("method")}) + "\n")
        count += 1
    if "id" not in request:
        continue
    method = request["method"]
    if method == "initialize":
        result = {
            "protocolVersion": request["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "managed-python", "version": "1.0.0"},
        }
    elif method == "tools/list":
        result = {"tools": [{"name": "read", "description": "Read the Python fixture",
                            "inputSchema": {"type": "object"},
                            "annotations": {"readOnlyHint": True}}]}
    elif method == "tools/call":
        output = {"version": list(sys.version_info[:3]), "python": sys.executable,
                  "path": os.environ["PATH"]}
        result = {"content": [{"type": "text", "text": json.dumps(output)}],
                  "structuredContent": output}
    elif method == "ping":
        result = {}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"],
                          "error": {"code": -32601, "message": "Unknown fixture method"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
