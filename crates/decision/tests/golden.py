#!/usr/bin/env python3
"""Opt-in preparation and b11756 server goldens; never trains or converts weights."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import socket
import struct
import subprocess
import tarfile
import time
import urllib.error
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[3]
FIXTURES = Path(__file__).with_name("fixtures")
CACHE = ROOT / ".runtime/local-model-host"
TEMPLATE = (
    "{{ state }}\n{{ type }}: {{ instructions }}\n"
    "{% for o in options %}{{ 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'[loop.index0] }}: "
    "{{ o.description if o.description else o.key }}\n{% endfor %}Answer:"
)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def download(url, path, size, sha256):
    if path.exists() and path.stat().st_size == size and digest(path) == sha256:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".partial")
    with urllib.request.urlopen(url, timeout=30) as source, temporary.open("wb") as output:
        shutil.copyfileobj(source, output)
    if temporary.stat().st_size != size or digest(temporary) != sha256:
        raise ValueError("Pinned download size or SHA-256 does not match")
    os.replace(temporary, path)


class Reader:
    def __init__(self, data):
        self.data = memoryview(data)
        self.position = 0

    def number(self, kind):
        size = struct.calcsize("<" + kind)
        value = struct.unpack_from("<" + kind, self.data, self.position)[0]
        self.position += size
        return value

    def string(self):
        length = self.number("Q")
        value = bytes(self.data[self.position:self.position + length])
        if len(value) != length:
            raise ValueError("Truncated fixture string")
        self.position += length
        return value

    def value(self, kind):
        formats = {0: "B", 1: "b", 2: "H", 3: "h", 4: "I", 5: "i", 6: "f", 7: "?", 10: "Q", 11: "q", 12: "d"}
        if kind == 8:
            return self.string()
        if kind == 9:
            element, count = self.number("I"), self.number("Q")
            if element == 9 or count > 1_048_576:
                raise ValueError("Invalid fixture array")
            for _ in range(count):
                self.value(element)
            return None
        return self.number(formats[kind])


def string(value):
    value = value.encode()
    return struct.pack("<Q", len(value)) + value


def tensors(data):
    reader = Reader(data)
    if bytes(reader.data[:8]) != b"GGUF\x03\x00\x00\x00":
        raise ValueError("Expected a GGUF v3 fixture")
    reader.position = 8
    count, fields = reader.number("Q"), reader.number("Q")
    entries, alignment = [], 32
    for _ in range(fields):
        key = reader.string().decode()
        start = reader.position
        kind = reader.number("I")
        value = reader.value(kind)
        if key == "general.alignment":
            alignment = value
        entries.append((key, bytes(reader.data[start:reader.position])))
    table = reader.position
    spans = []
    for _ in range(count):
        name = reader.string().decode()
        dimensions = [reader.number("Q") for _ in range(reader.number("I"))]
        kind, offset = reader.number("I"), reader.number("Q")
        block, size = {0: (1, 4), 1: (1, 2), 8: (32, 34), 30: (1, 2)}[kind]
        elements = 1
        for dimension in dimensions:
            elements *= dimension
        if elements % block:
            raise ValueError("Fixture tensor is not block aligned")
        spans.append((name, offset, elements // block * size))
    start = (reader.position + alignment - 1) // alignment * alignment
    hashes = {name: hashlib.sha256(reader.data[start + offset:start + offset + size]).hexdigest()
              for name, offset, size in spans}
    return entries, bytes(reader.data[table:reader.position]), start, alignment, hashes


def synthetic(source, output):
    data = source.read_bytes()
    entries, table, start, alignment, hashes = tensors(data)
    changes = {
        "general.name": (8, "Synthetic OpenJev compatibility fixture (SmolLM2)"),
        "general.description": (8, "Untrained compatibility fixture; no decision quality claim"),
        "llama.decision.type": (8, "openjev"),
        "tokenizer.chat_template.systemone": (8, TEMPLATE),
        "llama.decision.temperature.noul": (6, 1.25),
        "llama.decision.temperature.choice.3_5": (6, 0.8),
        "llama.decision.temperature.score": (6, 0.9),
        "llama.decision.score_bias.3.1": (6, -0.1),
        "llama.decision.score_bias.3.2": (6, 0.2),
    }
    entries = [(key, value) for key, value in entries if key not in changes]
    for key, (kind, value) in changes.items():
        entries.append((key, struct.pack("<I", kind) + (string(value) if kind == 8 else struct.pack("<f", value))))
    count = struct.unpack_from("<Q", data, 8)[0]
    header = b"GGUF" + struct.pack("<IQQ", 3, count, len(entries))
    header += b"".join(string(key) + value for key, value in entries) + table
    header += b"\0" * (-len(header) % alignment)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(".gguf.partial")
    with temporary.open("wb") as target:
        target.write(header)
        target.write(memoryview(data)[start:])
    _, _, _, _, transformed = tensors(temporary.read_bytes())
    if transformed != hashes:
        raise ValueError("Synthetic fixture changed a weight tensor")
    os.replace(temporary, output)
    return {"version": 1, "purpose": "Synthetic compatibility fixture; not trained openjev or a recommended model",
            "source_sha256": digest(source), "size": output.stat().st_size, "sha256": digest(output),
            "metadata_changes": {key: value for key, (_, value) in changes.items()}, "tensor_sha256": hashes}


def prepare_models():
    specification = json.loads((FIXTURES / "models.json").read_text())
    paths = {}
    for name, model in specification["models"].items():
        source = CACHE / "models" / model["file"]
        url = f"https://huggingface.co/{model['repository']}/resolve/{model['revision']}/{model['file']}"
        download(url, source, model["size"], model["sha256"])
        output = CACHE / "models" / name / "model.gguf"
        output.parent.mkdir(parents=True, exist_ok=True)
        if name == "synthetic-openjev":
            manifest = synthetic(source, output)
            expected = FIXTURES / "synthetic-openjev.json"
            if expected.exists() and json.loads(expected.read_text()) != manifest:
                raise ValueError("Synthetic fixture differs from its committed provenance")
            (CACHE / "golden").mkdir(parents=True, exist_ok=True)
            (CACHE / "golden/synthetic-openjev.json").write_text(json.dumps(manifest, indent=2) + "\n")
        elif not output.exists():
            try:
                os.link(source, output)
            except OSError:
                shutil.copyfile(source, output)
        if name != "synthetic-openjev" and digest(output) != model["sha256"]:
            raise ValueError("Prepared model does not match its original")
        paths[name] = output
    return paths


def server_binary():
    machine = platform.machine().lower()
    target = {
        ("Darwin", "arm64"): "aarch64-apple-darwin", ("Darwin", "x86_64"): "x86_64-apple-darwin",
        ("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
        ("Windows", "amd64"): "x86_64-pc-windows-msvc", ("Windows", "arm64"): "aarch64-pc-windows-msvc",
    }[(platform.system(), machine)]
    lock = json.loads((ROOT / "scripts/package/llama-runtime.lock.json").read_text())
    asset = lock["targets"][target]
    archive = ROOT / "target/llama-prebuilt" / asset["archive"]
    download(lock["source"] + asset["archive"], archive, asset["size"], asset["sha256"])
    output = CACHE / "golden/server"
    output.mkdir(parents=True, exist_ok=True)
    executable = "llama-server.exe" if platform.system() == "Windows" else "llama-server"
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as source:
            contents = {Path(name).name: source.read(name) for name in source.namelist()
                        if Path(name).name == executable or name.endswith(".dll")}
    else:
        with tarfile.open(archive) as source:
            members = {member.name: member for member in source.getmembers()}

            def content(member):
                if member.issym():
                    name = PurePosixPath(member.name).parent / member.linkname
                    if name.is_absolute() or ".." in name.parts:
                        raise ValueError("Invalid server archive link")
                    return content(members[str(name)])
                if not member.isfile():
                    raise ValueError("Server archive member is not a file")
                return source.extractfile(member).read()

            contents = {Path(name).name: content(member) for name, member in members.items()
                        if Path(name).name == executable or ".dylib" in name or ".so" in name}
    for name, data in contents.items():
        path = output / name
        path.write_bytes(data)
        path.chmod(0o755)
    if platform.system() == "Darwin":
        for name in contents:
            subprocess.run(["codesign", "--force", "--timestamp=none", "--sign", "-", str(output / name)],
                           check=True, capture_output=True)
    return output / executable


@contextlib.contextmanager
def server(binary, model):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    log = (CACHE / "golden/server.log").open("wb")
    slots = CACHE / "golden/slots"
    slots.mkdir(parents=True, exist_ok=True)
    process = subprocess.Popen([str(binary), "--model", str(model), "--host", "127.0.0.1", "--port", str(port),
        "--device", "none", "--n-gpu-layers", "0", "--threads", "4", "--threads-batch", "4",
        "--no-op-offload", "--no-kv-offload", "--no-repack",
        "--ctx-size", "1024", "--batch-size", "1024", "--ubatch-size", "1024", "--parallel", "1",
        "--pooling", "none", "--no-warmup", "--slot-save-path", str(slots)], stdout=log, stderr=log,
        env={**os.environ, "LLAMA_SERVER_SLOTS_DEBUG": "1"})
    url = f"http://127.0.0.1:{port}"
    try:
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError("Pinned llama-server exited; see the private server.log")
            try:
                with urllib.request.urlopen(url + "/health", timeout=1):
                    break
            except (OSError, urllib.error.HTTPError):
                time.sleep(0.1)
        else:
            raise TimeoutError("Pinned llama-server startup timed out")
        yield url
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        log.close()


def post(url, path, body):
    request = urllib.request.Request(url + path, json.dumps(body, ensure_ascii=False).encode(),
                                     {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def evaluate(url, body, laya):
    result, answers, input_tokens = {}, {}, 0
    marker = post(url, "/tokenize", {"content": "<mask>", "add_special": False,
                                    "parse_special": True})["tokens"] if laya else []
    for name, question in body["questions"].items():
        # SystemOne tasks keep the server's default prefix-cache policy. Erase it
        # explicitly so every oracle question matches Engine's independent forward.
        post(url, "/slots/0?action=erase", {})
        answer = post(url, "/v1/systemone", {"state": body["state"], "questions": {name: question}})
        answers.update(answer["answers"])
        input_tokens += answer["usage"]["input_tokens"]
        if answer["usage"]["output_tokens"] != 0:
            raise ValueError("Decision oracle generated output tokens")
        with urllib.request.urlopen(url + "/slots", timeout=10) as response:
            slots = json.load(response)
        prompt = slots[0]["prompt"]
        tokens = post(url, "/tokenize", {"content": prompt, "add_special": False,
                                        "parse_special": True})["tokens"]
        if len(tokens) != answer["usage"]["input_tokens"]:
            raise ValueError("Server prompt does not round-trip through its tokenizer")
        markers = [index for index, token in enumerate(tokens) if marker and token == marker[0]]
        result[name] = {"prompt": prompt, "tokens": tokens, "markers": markers}
    return {"answers": answers, "usage": {"input_tokens": input_tokens, "output_tokens": 0}}, result


def generate(paths):
    binary = server_binary()
    requests = json.loads((FIXTURES / "requests.json").read_text())
    goldens = {}
    for name, model in paths.items():
        with server(binary, model) as url:
            shared = post(url, "/v1/systemone", requests[0]) if name == "synthetic-openjev" else None
            responses, prompts = [], []
            for body in requests:
                result, prompt = evaluate(url, body, name == "julia")
                responses.append(result)
                prompts.append(prompt)
            goldens[name] = {"model_sha256": digest(model), "responses": responses, "inputs": prompts}
            if shared is not None:
                goldens[name]["shared_prefix_reference"] = {"request": 0, "answers": shared["answers"], "usage": shared["usage"]}
    policy = {"questions": "independent_forward", "slot_erase": True, "device": "none", "gpu_layers": 0,
              "threads": 4, "context": 1024, "batch": 1024, "ubatch": 1024,
              "op_offload": False, "kv_offload": False, "repack": False}
    (FIXTURES / "golden.json").write_text(json.dumps({"version": 1, "llama_build": "b11756", "policy": policy,
                                                    "models": goldens}, indent=2, ensure_ascii=False) + "\n")
    shutil.copyfile(CACHE / "golden/synthetic-openjev.json", FIXTURES / "synthetic-openjev.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--generate", action="store_true", help="replace committed server goldens for review")
    arguments = parser.parse_args()
    paths = prepare_models()
    if arguments.generate:
        generate(paths)
    for name, path in paths.items():
        print(name, path.stat().st_size, digest(path))


if __name__ == "__main__":
    main()
