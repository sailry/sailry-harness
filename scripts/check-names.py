#!/usr/bin/env python3
"""Check concise names in Sailry-owned source, excluding generated bindings."""

import ast
from bisect import bisect_left
from dataclasses import dataclass
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
IDENTIFIER_WORDS = 7
LABEL_WORDS = 12
SOURCE_ROOTS = {"apps", "crates", "scripts", "services"}
EXTENSIONS = {".rs", ".dart", ".py", ".ts", ".js", ".mjs", ".java", ".kt", ".swift"}

# Preserve offsets while ignoring comments and literal contents. Rust raw strings
# and Dart triple-quoted strings may contain entire synthetic source files.
TOKENS = re.compile(
    r"//[^\n]*|/\*.*?\*/|"
    r'\b(?:br|r)(?P<hashes>\#{0,16})".*?"(?P=hashes)|'
    r"[rR]?(?:'''(?:.*?)'''|\"\"\"(?:.*?)\"\"\")|"
    r'"(?:\\.|[^"\\])*"|'
    r"'(?:\\.|[^'\\\n])*'",
    re.DOTALL,
)
RUST_TOKENS = re.compile(
    r"//[^\n]*|/\*.*?\*/|"
    r'\b(?:br|r)(?P<hashes>\#{0,16})".*?"(?P=hashes)|'
    r'"(?:\\.|[^"\\])*"|'
    r"'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'",
    re.DOTALL,
)
RUST_FUNCTION = re.compile(r"\bfn\s+([A-Za-z_]\w*)\s*(?:\(|<)")
NAMED_FUNCTION = re.compile(r"\b(?:function|func|fun)\s+([A-Za-z_]\w*)\s*(?:\(|<)")
TYPED_FUNCTION = re.compile(
    r"(?m)^[ \t]*(?:(?:public|private|protected|override|static|final|external|async)\s+)*"
    r"[\w.]+(?:\s*<[^;{}()=]+>)?\??[ \t]+([A-Za-z_]\w*)\s*(?:<[^;{}()=]+>)?\s*\("
)
ARROW_FUNCTION = re.compile(
    r"\b(?:const|let|var)\s+([A-Za-z_]\w*)\s*=\s*(?:async\s+)?"
    r"(?:\([^;{}]*?\)|[A-Za-z_]\w*)\s*=>"
)
METHOD = re.compile(
    r"(?m)^[ \t]*(?:async\s+)?([A-Za-z_]\w*)\s*\([^;{}]*?\)\s*(?::[^;{}]+)?\s*\{"
)
TEST_CALL = re.compile(r"\b(?:testWidgets|test|it)(?:\.(?:only|skip|todo))?\s*\(")
GROUP_CALL = re.compile(r"\b(?:group|describe)(?:\.(?:only|skip))?\s*\(")
SCOPES = re.compile(
    r"\b(mod|impl|trait|class|extension)\s*(?:<[^{};]+?>)?\s+([^{};]+)\{|[{}]"
)
GENERIC_SCOPES = {"tests", "test", "lib", "main", "mod", "api", "src", "hook"}


@dataclass(frozen=True)
class Name:
    line: int
    value: str
    label: bool = False
    scope: str | None = None


def owned(path):
    return (
        path.parts[0] in SOURCE_ROOTS
        and path.suffix in EXTENSIONS
        and "generated" not in path.parts
        and not path.name.startswith("frb_generated")
        and "GeneratedPluginRegistrant" not in path.name
    )


def words(value):
    expanded = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", value)
    expanded = re.sub(r"([A-Z])([A-Z][a-z])", r"\1 \2", expanded)
    return re.findall(r"[A-Za-z0-9]+", expanded)


def mask(source, suffix):
    literals = {}

    def replace(match):
        text = match.group()
        if not text.startswith(("//", "/*")):
            literals[match.start()] = text
        return re.sub(r"[^\n]", " ", text)

    tokens = RUST_TOKENS if suffix == ".rs" else TOKENS
    return tokens.sub(replace, source), literals


def scopes(code, literals):
    groups = {}
    for match in GROUP_CALL.finditer(code):
        offset = match.end()
        while offset < len(code) and code[offset].isspace():
            offset += 1
        # Literal contents are spaces in code; use their original offsets.
        literal = next((value for start, value in literals.items()
                        if match.end() <= start <= offset), None)
        brace = code.find("{", match.end())
        if literal is not None and brace != -1:
            groups[brace] = literal

    stack = []
    for match in SCOPES.finditer(code):
        token = match.group()
        if token == "}":
            if stack:
                stack.pop()
        elif token == "{":
            stack.append(groups.get(match.start()))
        else:
            kind, header = match.group(1, 2)
            header = header.strip()
            if kind == "impl":
                if re.search(r"\bfor\b", header):
                    # A trait implementation must keep the interface's names.
                    owner = "-"
                else:
                    header = header.split(" where ", 1)[0]
                    owner = header.split("<", 1)[0].split("::")[-1]
            else:
                declared = re.match(r"\w+", header)
                owner = declared.group() if declared is not None else "-"
            stack.append(None if owner in GENERIC_SCOPES else owner)
        yield match.end(), next((owner for owner in reversed(stack) if owner), None)


def file_scope(path):
    stem = path.stem.removeprefix("test_").removesuffix("_test").removesuffix("_tests").removesuffix(".test")
    if stem in GENERIC_SCOPES:
        stem = next((part for part in reversed(path.parts[:-1])
                     if part not in GENERIC_SCOPES), "")
    return stem


def repeats(name, scope):
    if not scope:
        return False
    # Plurals do not introduce another ownership scope.
    value = [word.lower().removesuffix("s") for word in words(name)]
    context = [word.lower().removesuffix("s") for word in words(scope)]
    return bool(context) and len(value) > len(context) and value[:len(context)] == context


def names(source, suffix):
    if suffix == ".py":
        def visit(node, owner=None):
            if isinstance(node, ast.ClassDef):
                owner = node.name
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                # unittest requires this discovery prefix; the class gives scope.
                value = node.name.removeprefix("test_")
                yield Name(node.lineno, value, scope=owner)
            for child in ast.iter_child_nodes(node):
                yield from visit(child, owner)
        yield from visit(ast.parse(source))
        return

    code, literals = mask(source, suffix)
    newlines = [match.start() for match in re.finditer("\n", source)]

    def line(offset):
        return bisect_left(newlines, offset) + 1

    owners = list(scopes(code, literals))
    positions = [offset for offset, _ in owners]

    def owner(offset):
        index = bisect_left(positions, offset) - 1
        return owners[index][1] if index >= 0 else None

    patterns = [RUST_FUNCTION] if suffix == ".rs" else [NAMED_FUNCTION, TYPED_FUNCTION]
    if suffix in {".js", ".mjs", ".ts"}:
        patterns += [ARROW_FUNCTION, METHOD]
    found = set()
    for pattern in patterns:
        for match in pattern.finditer(code):
            offset = match.start(1)
            value = match.group(1)
            # Constructors and return expressions are not named declarations.
            prefix = match.group().split()[0]
            if pattern == TYPED_FUNCTION and (
                value[0].isupper() or prefix in {"return", "throw", "new", "const", "await", "yield"}
            ):
                continue
            if offset not in found:
                found.add(offset)
                yield Name(line(offset), value, scope=owner(offset))

    if suffix in {".dart", ".ts", ".js", ".mjs"}:
        for match in TEST_CALL.finditer(code):
            offset = match.end()
            while offset < len(source) and source[offset].isspace():
                offset += 1
            literal = literals.get(offset)
            if literal is not None:
                yield Name(line(offset), literal, label=True, scope=owner(offset))


def violations(source, suffix, path=None):
    result = []
    for name in names(source, suffix):
        limit = LABEL_WORDS if name.label else IDENTIFIER_WORDS
        count = len(words(name.value))
        if count > limit:
            kind = "Test label" if name.label else "Function name"
            result.append((name.line, f"{kind} has {count} words (maximum {limit}): {name.value}"))
        context = name.scope or (file_scope(path) if path is not None else None)
        if repeats(name.value, context):
            result.append((name.line, f"Name repeats its owning scope '{context}': {name.value}"))
    return result


def main():
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
    checked = 0
    failed = False
    for value in tracked:
        if not value:
            continue
        path = Path(value)
        if not owned(path):
            continue
        checked += 1
        for line, message in violations((ROOT / path).read_text(encoding="utf-8"), path.suffix, path):
            print(f"{path}:{line}: {message}")
            failed = True
    if failed:
        print("Use the owning scope and keep the distinguishing behavior; do not truncate names")
        return 1
    print(f"Concise names checked in {checked} source files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
