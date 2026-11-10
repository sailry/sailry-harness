import { TTL_MS } from "./contract";
import { handle, json } from "./http";
import { Exchange } from "./exchange";
import { SqliteObjects, type Storage } from "./sqlite-objects";
import { cleanup } from "./cleanup";

interface Context {
  storage: Storage & { setAlarm(time: number): Promise<void> };
  blockConcurrencyWhile<T>(callback: () => Promise<T>): Promise<T>;
}
interface Env {
  CODES: { idFromName(name: string): unknown; get(id: unknown): { fetch(request: Request): Promise<Response> } };
}

export class PairingCodes {
  private readonly registry: Exchange;
  private readonly objects: SqliteObjects;
  private pending = 0;
  constructor(private readonly context: Context) {
    this.objects = new SqliteObjects(context.storage);
    this.registry = new Exchange(this.objects);
  }
  async fetch(request: Request): Promise<Response> {
    if (this.pending >= 64) return json(503, { error: "capacity" });
    this.pending++;
    try {
      await cleanup(this.objects, Date.now());
      // Set the expiry alarm before admitting secrets, including idle objects.
      await this.context.storage.setAlarm(Date.now() + TTL_MS * 3);
      return await handle(request, this.registry, request.headers.get("CF-Connecting-IP") ?? "unknown");
    } finally { this.pending--; }
  }
  async alarm(): Promise<void> {
    let cursor: string | undefined;
    // One active minute is bounded by admission limits. Drain older generations.
    do {
      const result = await cleanup(this.objects, Date.now(), cursor);
      cursor = result.cursor;
    } while (cursor);
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "GET" && new URL(request.url).pathname === "/healthz") return json(200, { ok: true });
    return env.CODES.get(env.CODES.idFromName("short-codes-v1")).fetch(request);
  },
};
