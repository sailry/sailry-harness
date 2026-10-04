import { afterEach, describe, expect, it, vi } from "vitest";
import { handle, readBody } from "../src/http";
import { Exchange } from "../src/exchange";
import { requestId, TTL_MS } from "../src/contract";
import { SqliteObjects } from "../src/sqlite-objects";
import { SqliteStorage } from "./storage";

afterEach(() => vi.restoreAllMocks());
describe("HTTP boundary", () => {
  it("delayed bodies recheck expiry", async () => {
    const database = new SqliteStorage();
    try {
      const now = 1_700_000_000_000;
      const exchange = new Exchange(new SqliteObjects(database), undefined, undefined, () => 0);
      const created = await exchange.execute("publish", { request_id: requestId(now), ticket: "opaque" }, "host", now);
      const { code } = created.body as { code: string };
      const clock = vi.spyOn(Date, "now").mockReturnValue(now + TTL_MS - 1);
      const body = new ReadableStream<Uint8Array>({ pull(controller) {
        clock.mockReturnValue(now + TTL_MS);
        controller.enqueue(new TextEncoder().encode(JSON.stringify({ code, request_id: requestId(now + TTL_MS) })));
        controller.close();
      } });
      const response = await handle(new Request("https://pair.example/v1/codes/redeem", { method: "POST", body, duplex: "half" } as RequestInit), exchange, "client");
      expect(response.status).toBe(404);
      expect(response.headers.get("cache-control")).toBe("no-store");
    } finally { database.close(); }
  });

  it("rejects query parameters before execution", async () => {
    const execute = vi.fn();
    expect((await handle(new Request("https://pair.example/v1/codes?pin=123456", { method: "POST" }), { execute }, "client")).status).toBe(404);
    expect(execute).not.toHaveBeenCalled();
  });

  it("oversized chunked bodies cancel the reader", async () => {
    let cancelled = false;
    const body = new ReadableStream<Uint8Array>({ pull(controller) { controller.enqueue(new Uint8Array(8192)); }, cancel() { cancelled = true; } });
    await expect(readBody(new Request("https://pair.example/v1/codes", { method: "POST", body, duplex: "half" } as RequestInit))).rejects.toMatchObject({ status: 413 });
    expect(cancelled).toBe(true);
  });
});
