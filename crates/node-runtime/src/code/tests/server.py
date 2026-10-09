import json
import os
import pathlib
import sys
import subprocess
import time
import urllib.parse

base = pathlib.Path.cwd()
control = base / "control"
log = base / "messages"
documents = {}
root = base

probe_source = """
import json, pathlib, socket, sys

def inspect(settings, name):
    result = {}
    pathlib.Path(name + '-allowed.txt').write_text('workspace')
    for label, path in settings['writes'].items():
        try:
            pathlib.Path(path + '-' + name).write_text('escaped')
            result[label] = True
        except OSError:
            result[label] = False
    try:
        pathlib.Path(settings['readonly']).write_text('escaped')
        result['readonly'] = True
    except OSError:
        result['readonly'] = False
    try:
        pathlib.Path(settings['protected']).read_bytes()
        result['protected'] = True
    except OSError:
        result['protected'] = False
    if settings.get('dependency'):
        try:
            result['dependency'] = pathlib.Path(settings['dependency']).read_text() == 'package dependency'
        except OSError:
            result['dependency'] = False
    try:
        with socket.create_connection(settings['network'], timeout=0.5):
            result['network'] = True
    except OSError:
        result['network'] = False
    return result

if __name__ == '__main__':
    print(json.dumps(inspect(json.loads(sys.argv[1]), sys.argv[2])))
"""

def probe(settings):
    namespace = {"__name__": "fixture_probe"}
    exec(probe_source, namespace)
    parent = namespace["inspect"](settings, "parent")
    child = subprocess.run([sys.executable, "-c", probe_source, json.dumps(settings), "child"],
                           check=True, capture_output=True, text=True)
    (base / "sandbox").write_text(json.dumps({"parent": parent,
                                                 "child": json.loads(child.stdout)}))

def send(value):
    body = json.dumps(value, ensure_ascii=False).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()

def reply(identity, value):
    send({"jsonrpc": "2.0", "id": identity, "result": value})

def publish(uri, document):
    text = document["text"]
    start = text.find("broken")
    items = []
    if start >= 0:
        prefix = text[:start]
        line = prefix.count("\n")
        column = len(prefix.rsplit("\n", 1)[-1].encode("utf-16-le")) // 2
        items = [{"range": {"start": {"line": line, "character": column}, "end": {"line": line, "character": column + 6}}, "severity": 1, "message": "fixture syntax error", "source": "fixture"}]
    send({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "version": document["version"], "diagnostics": items}})
    return items

while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            sys.exit(0)
        if line == b"\r\n":
            break
        name, value = line.decode().split(":", 1)
        headers[name.lower()] = value.strip()
    value = json.loads(sys.stdin.buffer.read(int(headers["content-length"])))
    with log.open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(value, ensure_ascii=False) + "\n")
    method = value.get("method")
    params = value.get("params") or {}
    identity = value.get("id")
    settings = json.loads(control.read_text()) if control.exists() else {}
    if method == "initialize":
        root = pathlib.Path(urllib.parse.unquote(urllib.parse.urlparse(params["rootUri"]).path))
        (base / "pid").write_text(str(os.getpid()))
        if settings.get("sandbox"):
            probe(settings["sandbox"])
        if settings.get("child"):
            child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(3600)"])
            (base / "child_pid").write_text(str(child.pid))
        if settings.get("initial_crash"):
            os._exit(7)
        sys.stderr.write("e" * settings.get("stderr", 0))
        sys.stderr.flush()
        reply(identity, {"capabilities": {"positionEncoding": settings.get("encoding", "utf-16"), "textDocumentSync": 1, "definitionProvider": True, "referencesProvider": True, "hoverProvider": True, "documentSymbolProvider": True, "workspaceSymbolProvider": True, "diagnosticProvider": {"interFileDependencies": True, "workspaceDiagnostics": False}}})
    elif method == "initialized":
        if settings.get("indexing"):
            send({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "index", "value": {"kind": "begin", "title": "fixture index"}}})
        send({"jsonrpc": "2.0", "id": "edit", "method": "workspace/applyEdit", "params": {"edit": {"changes": {}}}})
        send({"jsonrpc": "2.0", "id": "configuration", "method": "workspace/configuration", "params": {"items": [{"section": "fixture.option"}]}})
    elif method in ("textDocument/didOpen", "textDocument/didChange"):
        document = params["textDocument"]
        uri = document["uri"]
        documents[uri] = {"text": document.get("text", params.get("contentChanges", [{}])[0].get("text", "")), "version": document["version"]}
        publish(uri, documents[uri])
    elif method == "textDocument/didClose":
        documents.pop(params["textDocument"]["uri"], None)
    elif method == "textDocument/diagnostic":
        uri = params["textDocument"]["uri"]
        reply(identity, {"kind": "full", "items": publish(uri, documents[uri])})
    elif method == "textDocument/hover":
        if settings.get("hang"):
            continue
        if settings.get("crash"):
            os._exit(7)
        reply(identity, {"contents": {"kind": "plaintext", "value": "fixture signature " + "🙂" * settings.get("hover_size", 0)}})
        if settings.get("finish_index"):
            send({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "index", "value": {"kind": "end"}}})
    elif method == "textDocument/completion":
        reply(identity, settings.get("completions", []))
    elif method in ("textDocument/definition", "textDocument/references"):
        location = {"uri": (root / "target.rs").as_uri(), "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}}
        locations = [location]
        if settings.get("outside"):
            locations.append({"uri": (root.parent / "outside.rs").as_uri(), "range": location["range"]})
        reply(identity, locations)
    elif method == "textDocument/documentSymbol":
        reply(identity, [{"name": "fixture", "kind": 12, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}, "selectionRange": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}}])
    elif method == "workspace/symbol":
        reply(identity, [{"name": "fixture", "kind": 12, "location": {"uri": (root / "target.rs").as_uri(), "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}}}])
    elif method == "shutdown":
        if settings.get("hang_shutdown"):
            continue
        reply(identity, None)
    elif method == "exit":
        sys.exit(0)
    elif identity is not None and method is not None:
        reply(identity, None)
