import { PreconditionFailedError, type Store } from "@edgeone/pages-blob";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BlobObjects } from "../src/blob";
import { elect, type Objects, type RecordValue } from "../src/objects";
import { SqliteObjects } from "../src/sqlite-objects";
import { SqliteStorage } from "./storage";
import { handle } from "../src/http";

const records = new Map<string, RecordValue>();
const get = vi.fn(async (key: string) => structuredClone(records.get(key) ?? null));
const setJSON = vi.fn(async (key: string, value: unknown) => {
  // Yield before the atomic operation to exercise competing requests.
  await Promise.resolve();
  if (records.has(key)) throw new PreconditionFailedError();
  records.set(key, structuredClone(value as RecordValue));
});
const sdk = { get, setJSON } as unknown as Pick<Store, "get" | "setJSON" | "list" | "delete">;
const databases: SqliteStorage[] = [];
afterEach(() => {
  records.clear(); vi.clearAllMocks();
  databases.splice(0).forEach(database => database.close());
});

describe.each(["native SQLite", "Blob SDK contract"])("immutable records: %s", backend => {
  function setup(): () => Objects {
    if (backend === "Blob SDK contract") return () => new BlobObjects(sdk);
    const database = new SqliteStorage(); databases.push(database);
    return () => new SqliteObjects(database);
  }

  it("independent adapters elect one candidate", async () => {
    const open = setup();
    const candidates = Array.from({ length: 32 }, (_, index) => ({ expires: 60_000, value: String(index) }));
    const winners = await Promise.all(candidates.map(candidate => elect(open(), "claim/unique-generation", candidate)));
    expect(new Set(winners.map(winner => winner.value)).size).toBe(1);
    expect(winners[0]).toEqual(candidates[0]);
  });

  it("expired records remain immutable", async () => {
    const open = setup();
    const original = { expires: 1, value: "first" };
    expect(await open().create("generation/key", original)).toBe(true);
    expect(await open().create("generation/key", { expires: 120_000, value: "replacement" })).toBe(false);
    expect(await open().get("generation/key")).toEqual(original);
    expect(await open().get("missing")).toBeNull();
  });

  it("lost responses recover the existing winner", async () => {
    const open = setup();
    const original = { expires: 60_000, value: "stable-request-id" };
    await open().create("claim/key", original);
    expect(await elect(open(), "claim/key", original)).toEqual(original);
    expect(await elect(open(), "claim/key", { ...original, value: "competitor" })).toEqual(original);
  });
});

describe("Blob adapter", () => {
  it("strong reads and conditional writes", async () => {
    const store = new BlobObjects(sdk);
    const record = { expires: 60_000, value: "opaque" };
    await store.create("key", record);
    await store.get("key");
    expect(setJSON).toHaveBeenCalledWith("key", record, { onlyIfNew: true, cacheControl: "no-store" });
    expect(get).toHaveBeenCalledWith("key", { type: "json", consistency: "strong" });
  });

  it("propagates quota and network failures", async () => {
    const error = new Error("private storage diagnostic");
    setJSON.mockRejectedValueOnce(error);
    await expect(new BlobObjects(sdk).create("key", { expires: 1, value: "x" })).rejects.toBe(error);
  });

  it("rejects malformed stored records", async () => {
    get.mockResolvedValueOnce({ expires: NaN, value: "x" });
    await expect(new BlobObjects(sdk).get("key")).rejects.toThrow("Invalid pairing storage record");
  });

  it("rejects invisible conditional writes", async () => {
    await expect(elect({ create: async () => false, get: async () => null }, "key", { expires: 1, value: "x" }))
      .rejects.toThrow("Conditional write is not visible");
  });
});

describe("asynchronous HTTP boundary", () => {
  const request = () => new Request("https://pair.example/v1/codes", { method: "POST", body: "{}" });
  it("awaits durable completion", async () => {
    const response = await handle(request(), { execute: async () => ({ status: 201, body: { code: "123456" } }) }, "test");
    expect(response.status).toBe(201);
    expect(await response.json()).toEqual({ code: "123456" });
  });
  it("redacts storage failures", async () => {
    const response = await handle(request(), { execute: async () => { throw new Error("private ticket"); } }, "test");
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "unavailable" });
    expect(response.headers.get("cache-control")).toBe("no-store");
  });
});
