import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build } from "esbuild";
import { Miniflare } from "miniflare";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { requestId as randomToken } from "../src/contract";

describe("real workerd and durable SQLite", () => {
  let directory: string;
  let script: string;
  let worker: Miniflare;
  function start() {
    return new Miniflare({ modules: true, script, compatibilityDate: "2025-01-01",
      durableObjects: { CODES: { className: "PairingCodes", useSQLite: true } },
      durableObjectsPersist: directory });
  }
  beforeAll(async () => {
    directory = await mkdtemp(join(tmpdir(), "sailry-codes-"));
    const output = await build({ entryPoints: ["src/worker.ts"], bundle: true, write: false, format: "esm", platform: "browser" });
    script = output.outputFiles[0].text;
    worker = start();
    await worker.ready;
  });
  afterAll(async () => {
    await worker?.dispose();
    if (directory) await rm(directory, { recursive: true });
  });
  function post(path: string, body: object, source = "192.0.2.1") {
    return worker.dispatchFetch(`https://relay.test${path}`, { method: "POST",
      headers: { "CF-Connecting-IP": source, "Content-Type": "application/json" }, body: JSON.stringify(body) });
  }

  it("concurrent claim survives process restart", async () => {
    const publication = { request_id: randomToken(), ticket: "iroh-opaque-fixture" };
    const created = await post("/v1/codes", publication);
    expect(created.status).toBe(201);
    const { code, expires_at_ms } = await created.json() as { code: string; expires_at_ms: number };
    expect(code).toMatch(/^\d{6}$/);
    expect(expires_at_ms - Date.now()).toBeGreaterThan(55_000);
    const requests = Array.from({ length: 12 }, () => ({ code, request_id: randomToken() }));
    const results = await Promise.all(requests.map((request, index) => post("/v1/codes/redeem", request, `192.0.2.${index + 2}`)));
    expect(results.filter(response => response.status === 200)).toHaveLength(1);
    expect(results.filter(response => response.status === 404)).toHaveLength(11);
    const winner = results.findIndex(response => response.status === 200);
    expect(await results[winner].json()).toMatchObject({ ticket: publication.ticket });

    await worker.dispose();
    worker = start();
    await worker.ready;
    const retried = await post("/v1/codes/redeem", requests[winner], `192.0.2.${winner + 2}`);
    expect(retried.status).toBe(200);
    expect(await retried.json()).toMatchObject({ ticket: publication.ticket });
    expect((await post("/v1/codes/redeem", { code, request_id: randomToken() }, "192.0.2.50")).status).toBe(404);
    const publicationRetry = await post("/v1/codes", publication);
    expect(publicationRetry.status).toBe(201);
    expect(await publicationRetry.json()).toMatchObject({ code, expires_at_ms });
  });

  it("rate limits survive restart", async () => {
    const source = "198.51.100.1";
    for (let i = 0; i < 5; i++) {
      expect((await post("/v1/codes/redeem", { request_id: randomToken(), code: "999999" }, source)).status).toBe(404);
    }
    await worker.dispose();
    worker = start();
    await worker.ready;
    const blocked = await post("/v1/codes/redeem", { request_id: randomToken(), code: "999999" }, source);
    expect(blocked.status).toBe(429);
    expect(blocked.headers.get("cache-control")).toBe("no-store");
  });
});
