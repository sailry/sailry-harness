import { PreconditionFailedError, type Store } from "@edgeone/pages-blob";
import { afterEach, describe, expect, it } from "vitest";
import { BlobObjects } from "../src/blob";
import { cleanup } from "../src/cleanup";
import { generation, randomToken, requestId, TTL_MS } from "../src/contract";
import { serve } from "../src/edgeone";
import { Exchange, limits } from "../src/exchange";
import { SqliteObjects } from "../src/sqlite-objects";
import { SqliteStorage } from "./storage";
import type { ManagedObjects, RecordValue } from "../src/objects";

const NOW = 1_700_000_000_000;
const databases: SqliteStorage[] = [];
afterEach(() => databases.splice(0).forEach(database => database.close()));

function blob(): ManagedObjects {
  const data = new Map<string, RecordValue>();
  const sdk = {
    get: async (key: string) => structuredClone(data.get(key) ?? null),
    setJSON: async (key: string, value: RecordValue) => {
      await Promise.resolve();
      if (data.has(key)) throw new PreconditionFailedError();
      data.set(key, structuredClone(value));
    },
    list: async ({ cursor = "", limit = 128 }) => {
      const keys = [...data.keys()].sort().filter(key => key > cursor).slice(0, limit);
      return { blobs: keys.map(key => ({ key, etag: "test" })), directories: [], cursor: keys.length === limit ? keys.at(-1) : undefined };
    },
    delete: async (key: string) => { data.delete(key); },
  } as unknown as Pick<Store, "get" | "setJSON" | "list" | "delete">;
  return new BlobObjects(sdk);
}

describe.each(["SQLite", "Blob contract"])("shared exchange: %s", backend => {
  function setup(budget = limits, nextCode?: () => string) {
    let store: ManagedObjects;
    if (backend === "SQLite") {
      const database = new SqliteStorage(); databases.push(database);
      store = new SqliteObjects(database);
    } else store = blob();
    const exchange = new Exchange(store, budget, nextCode, () => 0);
    return { store, exchange };
  }
  async function publish(exchange: Exchange, now = NOW, ticket = "opaque-iroh-ticket") {
    const request = { request_id: requestId(now), ticket };
    const result = await exchange.execute("publish", request, "host", now);
    expect(result.status).toBe(201);
    return { request, value: result.body as { code: string; cancel_token: string; expires_at_ms: number } };
  }

  it("expires after sixty seconds across minute boundaries", async () => {
    const { exchange } = setup();
    const now = Math.floor(NOW / TTL_MS) * TTL_MS + 59_999;
    const { value } = await publish(exchange, now);
    expect(value.code).toMatch(/^\d{6}$/);
    expect(value.expires_at_ms).toBe(now + TTL_MS);
    expect(await exchange.execute("redeem", { request_id: requestId(now + 59_999), code: value.code }, "client", now + 59_999))
      .toEqual({ status: 200, body: { ticket: "opaque-iroh-ticket", expires_at_ms: value.expires_at_ms } });
    expect((await exchange.execute("redeem", { request_id: requestId(now + TTL_MS), code: value.code }, "client", now + TTL_MS)).status).toBe(404);
  });

  it("recovers the concurrent claim winner", async () => {
    const { exchange, store } = setup();
    const { value } = await publish(exchange);
    const requests = Array.from({ length: 24 }, () => ({ request_id: requestId(NOW), code: value.code }));
    const results = await Promise.all(requests.map((body, index) => new Exchange(store, limits, undefined, () => 0).execute("redeem", body, `client-${index}`, NOW)));
    expect(results.filter(result => result.status === 200)).toHaveLength(1);
    expect(results.filter(result => result.status === 404)).toHaveLength(23);
    const winner = results.findIndex(result => result.status === 200);
    expect(await exchange.execute("redeem", requests[winner], "retry", NOW)).toEqual(results[winner]);
  });

  it("concurrent retries share one publication", async () => {
    const { exchange } = setup();
    const request = { request_id: requestId(NOW), ticket: "opaque" };
    const results = await Promise.all(Array.from({ length: 5 }, () => exchange.execute("publish", request, "host", NOW)));
    expect(results.every(result => result.status === 201)).toBe(true);
    expect(new Set(results.map(result => JSON.stringify(result.body))).size).toBe(1);
    expect((await exchange.execute("publish", { ...request, ticket: "different" }, "other", NOW)).status).toBe(409);
  });

  it("binds a claim ID to one code", async () => {
    const { exchange } = setup();
    const first = await publish(exchange), second = await publish(exchange);
    const request_id = requestId(NOW);
    expect((await exchange.execute("redeem", { request_id, code: first.value.code }, "client", NOW)).status).toBe(200);
    expect((await exchange.execute("redeem", { request_id, code: second.value.code }, "client", NOW)).status).toBe(409);
  });

  it("losing publications remain inaccessible", async () => {
    const { exchange, store } = setup();
    const request = { request_id: requestId(NOW), ticket: "opaque" };
    const original = store.get.bind(store);
    let arrived = 0;
    let release!: () => void;
    const ready = new Promise<void>(resolve => { release = resolve; });
    store.get = async key => {
      if (key.endsWith(`/publish/${request.request_id}`) && arrived < 5) {
        const value = await original(key);
        if (++arrived === 5) release();
        await ready;
        return value;
      }
      return original(key);
    };
    const results = await Promise.all(Array.from({ length: 5 }, () => exchange.execute("publish", request, "host", NOW)));
    const winner = (results[0].body as { code: string }).code;
    const page = await store.list();
    const candidates = page.keys.filter(key => key.includes("/pin/")).map(key => key.split("/").at(-1)!);
    expect(candidates.length).toBeGreaterThan(1);
    for (const code of candidates) {
      const result = await exchange.execute("redeem", { code, request_id: requestId(NOW) }, `client-${code}`, NOW);
      expect(result.status).toBe(code === winner ? 200 : 404);
    }
  });

  it("clock skew across generation boundaries", async () => {
    const { exchange } = setup();
    const now = Math.floor(NOW / TTL_MS) * TTL_MS + 59_000;
    const result = await exchange.execute("publish", { request_id: requestId(now + 2000), ticket: "opaque" }, "host", now);
    expect(result.status).toBe(201);
    const { code, expires_at_ms } = result.body as { code: string; expires_at_ms: number };
    expect(expires_at_ms).toBe(now + TTL_MS);
    expect((await exchange.execute("redeem", { code, request_id: requestId(now) }, "client", now)).status).toBe(200);
  });

  it("cancellation prevents ticket recovery", async () => {
    const { exchange } = setup();
    const { value } = await publish(exchange);
    const claim = { code: value.code, request_id: requestId(NOW) };
    expect((await exchange.execute("redeem", claim, "client", NOW)).status).toBe(200);
    expect((await exchange.execute("cancel", { code: value.code, cancel_token: randomToken() }, "host", NOW)).status).toBe(403);
    expect((await exchange.execute("cancel", { code: value.code, cancel_token: value.cancel_token }, "host", NOW)).status).toBe(204);
    expect((await exchange.execute("redeem", claim, "client", NOW)).status).toBe(404);
  });

  it("issuer deadline bounds publication retries", async () => {
    const { exchange } = setup();
    const request = { request_id: requestId(NOW), ticket: "opaque", expires_at_ms: NOW + 500 };
    expect((await exchange.execute("publish", request, "host", NOW)).body).toMatchObject({ expires_at_ms: NOW + 500 });
    expect((await exchange.execute("publish", request, "host", NOW + 500)).status).toBe(410);
  });

  it("code collisions preserve the original", async () => {
    const { exchange } = setup(limits, () => "000001");
    const first = await publish(exchange);
    expect((await exchange.execute("publish", { request_id: requestId(NOW), ticket: "other" }, "host", NOW)).status).toBe(503);
    expect((await exchange.execute("redeem", { request_id: requestId(NOW), code: first.value.code }, "client", NOW)).body).toMatchObject({ ticket: "opaque-iroh-ticket" });
  });

  it("capacity resets with the generation", async () => {
    const { exchange } = setup({ ...limits, entries: 1 });
    await publish(exchange);
    expect((await exchange.execute("publish", { request_id: requestId(NOW), ticket: "other" }, "host", NOW)).status).toBe(503);
    await publish(exchange, NOW + TTL_MS * 2);
  });

  it("source limits survive reconstruction", async () => {
    const { exchange, store } = setup();
    const body = { request_id: requestId(NOW), code: "999999" };
    for (let index = 0; index < 5; index++) expect((await exchange.execute("redeem", body, "client", NOW)).status).toBe(404);
    expect((await new Exchange(store, limits, undefined, () => 0).execute("redeem", body, "client", NOW)).status).toBe(429);
  });

  it("cleanup preserves reused codes", async () => {
    const { exchange, store } = setup(limits, () => "123456");
    const first = await publish(exchange);
    const later = NOW + TTL_MS * 3;
    const second = await publish(exchange, later, "new-ticket");
    expect(first.value.code).toBe(second.value.code);
    let cursor: string | undefined;
    do { cursor = (await cleanup(store, later, cursor)).cursor; } while (cursor);
    expect(await store.get(`${generation(NOW)}pin/${first.value.code}`)).toBeNull();
    expect((await exchange.execute("redeem", { request_id: requestId(later), code: second.value.code }, "client", later)).body).toMatchObject({ ticket: "new-ticket" });
    expect((await exchange.execute("publish", first.request, "host", later)).status).toBe(410);
  });

  it("rejects invalid fields and future request IDs", async () => {
    const { exchange } = setup();
    for (const body of [
      { request_id: requestId(NOW), code: 123456 },
      { request_id: requestId(NOW), code: "123456", extra: true },
      { request_id: requestId(NOW + TTL_MS), code: "123456" },
    ]) expect((await exchange.execute("redeem", body, "client", NOW)).status).toBe(400);
    expect((await exchange.execute("publish", { request_id: requestId(NOW), ticket: "x".repeat(8193) }, "host", NOW)).status).toBe(400);
  });
});

describe("asynchronous races", () => {
  it("slow claim writes recheck expiry", async () => {
    const store = blob();
    let clock = NOW;
    const exchange = new Exchange(store, limits, undefined, () => clock);
    const created = await exchange.execute("publish", { request_id: requestId(NOW), ticket: "opaque" }, "host", NOW);
    const { code } = created.body as { code: string };
    const original = store.create.bind(store);
    store.create = async (key, record) => {
      const result = await original(key, record);
      if (key.includes("/claim/")) clock += TTL_MS;
      return result;
    };
    expect((await exchange.execute("redeem", { code, request_id: requestId(NOW) }, "client", NOW)).status).toBe(404);
  });

  it("cancellation wins claim arbitration", async () => {
    const store = blob();
    const exchange = new Exchange(store, limits, undefined, () => 0);
    const result = await exchange.execute("publish", { request_id: requestId(NOW), ticket: "opaque" }, "host", NOW);
    const value = result.body as { code: string; cancel_token: string };
    const original = store.create.bind(store);
    store.create = async (key, record) => {
      const stored = await original(key, record);
      if (key.includes("/claim/")) expect((await exchange.execute("cancel", { code: value.code, cancel_token: value.cancel_token }, "host", NOW)).status).toBe(204);
      return stored;
    };
    expect((await exchange.execute("redeem", { code: value.code, request_id: requestId(NOW) }, "client", NOW)).status).toBe(404);
  });
});

describe("EdgeOne handler", () => {
  it("shared exchange and bounded maintenance", async () => {
    const store = blob();
    const request = new Request("https://pair.example/v1/codes", { method: "POST", body: JSON.stringify({ request_id: requestId(), ticket: "opaque" }) });
    const response = await serve({ request, clientIp: "192.0.2.1" }, store);
    expect(response.status).toBe(201);
    const value = await response.json() as { code: string };
    const redeemed = await serve({ request: new Request("https://pair.example/v1/codes/redeem", { method: "POST", body: JSON.stringify({ code: value.code, request_id: requestId() }) }), clientIp: "192.0.2.2" }, store);
    expect((await redeemed.json()).ticket).toBe("opaque");
    for (let index = 0; index < 2; index++) expect((await serve({ request: new Request("https://pair.example/internal/cleanup") }, store)).status).toBe(204);
    expect((await serve({ request: new Request("https://pair.example/unknown") }, store)).status).toBe(404);
  });
});
