import { BlobObjects } from "./blob";
import { cleanup } from "./cleanup";
import { generation, TTL_MS } from "./contract";
import { Exchange } from "./exchange";
import { handle, json } from "./http";
import type { ManagedObjects } from "./objects";

interface Context { request: Request; clientIp?: string }

export async function serve(context: Context, objects: ManagedObjects): Promise<Response> {
  const { request } = context;
  const url = new URL(request.url);
  if (request.method === "GET" && url.pathname === "/healthz" && !url.search) return json(200, { ok: true });
  try {
    if (request.method === "GET" && url.pathname === "/internal/cleanup" && !url.search) {
      const now = Date.now();
      // A public, idempotent maintenance route cannot delete live generations.
      // Native schedules call it; one pass per minute bounds abusive invocations.
      if (!await objects.create(`${generation(now)}maintenance`, { expires: now + TTL_MS, value: "1" })) return json(204);
      let cursor: string | undefined;
      for (let page = 0; page < 16 && Date.now() - now < 45_000; page++) {
        const result = await cleanup(objects, now, cursor);
        cursor = result.cursor;
        if (!cursor) break;
      }
      return json(204);
    }
    // Request-driven cleanup also drains old data while the deployment is busy.
    // Unknown routes never cause storage writes or maintenance work.
    if (request.method === "POST" && /^\/v1\/codes(?:\/redeem|\/cancel)?$/.test(url.pathname) && !url.search)
      await cleanup(objects, Date.now());
    return await handle(request, new Exchange(objects), context.clientIp ?? "unknown");
  } catch { return json(503, { error: "unavailable" }); }
}

export default async function onRequest(context: Context): Promise<Response> {
  // Construct in the request scope: deployed SDK credentials are platform-owned.
  try { return await serve(context, new BlobObjects()); }
  catch { return json(503, { error: "unavailable" }); }
}
