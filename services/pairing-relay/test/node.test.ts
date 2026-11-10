import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { expect, it } from "vitest";
import { start } from "../src/node/server";
import { requestId } from "../src/contract";

it("bounded exchanges and claim recovery after restart", async () => {
  const directory = await mkdtemp(join(tmpdir(), "sailry-relay-"));
  const path = join(directory, "codes.sqlite3");
  let service = await start(path, 0);
  const post = (route: string, body: object, source = "192.0.2.1") => fetch(service.address + route, {
    method: "POST", headers: { "Content-Type": "application/json", "CF-Connecting-IP": source }, body: JSON.stringify(body),
  });
  try {
    expect((await fetch(service.address + "/healthz")).status).toBe(200);
    const published = await post("/v1/codes", { request_id: requestId(), ticket: "opaque-fixture" });
    expect(published.status).toBe(201);
    const { code } = await published.json() as { code: string };
    const claims = Array.from({ length: 8 }, () => ({ code, request_id: requestId() }));
    const responses = await Promise.all(claims.map((claim, i) => post("/v1/codes/redeem", claim, `192.0.2.${i + 2}`)));
    expect(responses.filter(response => response.status === 200)).toHaveLength(1);
    const winner = responses.findIndex(response => response.status === 200);
    await service.close();
    service = await start(path, 0);
    const retried = await post("/v1/codes/redeem", claims[winner], `192.0.2.${winner + 2}`);
    expect(await retried.json()).toMatchObject({ ticket: "opaque-fixture" });
    expect((await post("/v1/codes/redeem", { code, request_id: requestId() }, "192.0.2.99")).status).toBe(404);
    expect((await post("/v1/codes", { content: "x".repeat(65536) })).status).toBe(413);
  } finally { await service.close(); await rm(directory, { recursive: true }); }
});
