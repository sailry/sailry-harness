"""A self-contained, read-only MCP example using only Python's standard library."""
import base64
import json
from pathlib import Path
import struct
import sys
import zlib


def image():
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(
            ">I", zlib.crc32(kind + data)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 1, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(b"\x00\x38\x98\x70\x38\x98\x70"))
    png += chunk(b"IEND", b"")
    return base64.b64encode(png).decode("ascii")


def dispatch(request):
    method = request.get("method")
    if method == "initialize":
        return {
            "protocolVersion": request["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "tool-content", "version": "1"},
        }
    if method == "ping":
        return {}
    if method == "tools/list":
        return {"tools": [{
            "name": "report",
            "description": "Show a sample report with a table, diff and image",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
            "annotations": {"readOnlyHint": True, "idempotentHint": True, "openWorldHint": False},
        }]}
    if method == "tools/call" and request.get("params", {}).get("name") == "report":
        content = json.loads(Path(__file__).with_name("content.json").read_text(encoding="utf-8"))
        return {
            "content": [
                {"type": "text", "text": "Sample report: hello.txt updated; notes.txt unchanged"},
                {"type": "image", "mimeType": "image/png", "data": image()},
            ],
            "structuredContent": {"sailry_content": content, "sample": True},
        }
    raise ValueError("Unknown method or tool")


def main():
    for line in sys.stdin:
        request = json.loads(line)
        if "id" not in request:
            continue
        response = {"jsonrpc": "2.0", "id": request["id"]}
        try:
            response["result"] = dispatch(request)
        except (KeyError, ValueError) as error:
            response["error"] = {"code": -32602, "message": str(error)}
        print(json.dumps(response), flush=True)


if __name__ == "__main__":
    main()
