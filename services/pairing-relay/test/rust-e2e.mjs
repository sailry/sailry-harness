import { build } from "esbuild";
import { Miniflare } from "miniflare";
import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

// Real workerd + SQLite and two isolated Rust Nodes; no deployed cloud resources.
const output = await build({ entryPoints: ["src/worker.ts"], bundle: true, write: false,
  format: "esm", platform: "browser" });
const desktop = process.argv.includes("--desktop");
const host = process.argv.includes("--host");
const edgeone = process.argv.includes("--edgeone");
let directory;
let start;
if (edgeone) {
  directory = await mkdtemp(join(tmpdir(), "sailry-edgeone-fixture-"));
  const outfile = join(directory, "fixture.mjs");
  await build({ entryPoints: ["test/edgeone-server.ts"], bundle: true, outfile, format: "esm", platform: "node" });
  start = (await import(pathToFileURL(outfile).href)).start;
}
try {
for (const test of host ? ["short_code_pairs_a_headless_process"] : desktop ? ["sharing_lifecycle"] : ["exchanges_code_and_authenticates_peer", "stops_sharing_after_pairing"]) {
const worker = edgeone ? await start() : new Miniflare({ modules: true, script: output.outputFiles[0].text,
  host: "127.0.0.1", port: 0, compatibilityDate: "2025-01-01",
  durableObjects: { CODES: { className: "PairingCodes", useSQLite: true } } });
try {
  const address = await worker.ready;
  const args = host ? ["test", "-p", "sailry-host", "--test", "lifecycle", test, "--", "--ignored"] : desktop ? ["test", "-p", "sailry-desktop", test, "--", "--ignored"] : ["test", "-p", "sailry-node-runtime", "--test", "short_codes", test, "--", "--ignored"];
  const child = spawn("cargo", args, {
    cwd: "../..", stdio: "inherit", env: { ...process.env, SAILRY_TEST_RELAY: address.toString() },
  });
  process.exitCode = await new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("exit", code => resolve(code ?? 1));
  });
} finally { await worker.dispose(); }
if (process.exitCode) break;
}
} finally { if (directory) await rm(directory, { recursive: true }); }
