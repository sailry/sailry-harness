import { createServer } from "node:http";
import { Readable } from "node:stream";
import { cleanup } from "../cleanup";
import { Exchange } from "../exchange";
import { handle, json } from "../http";
import { SqliteObjects } from "../sqlite-objects";
import { SqliteStorage } from "./storage";

/** Loopback origin for cloudflared; forwarded client addresses are trusted only here. */
export async function start(path: string, port: number) {
  const database = new SqliteStorage(path);
  const objects = new SqliteObjects(database);
  const exchange = new Exchange(objects);
  let pending = 0;
  let cleaning: Promise<void> | undefined;
  const sweep = () => cleaning ??= (async () => {
    let cursor: string | undefined;
    do { cursor = (await cleanup(objects, Date.now(), cursor)).cursor; } while (cursor);
  })().finally(() => { cleaning = undefined; });
  const timer = setInterval(() => { void sweep().catch(() => console.error("Pairing cleanup failed")); }, 60_000);
  timer.unref();
  const server = createServer(async (incoming, outgoing) => {
    if (pending >= 64) { outgoing.writeHead(503, { "Retry-After": "60" }).end(); return; }
    pending++;
    try {
      const request = new Request(`http://localhost${incoming.url}`, {
        method: incoming.method, headers: incoming.headers,
        body: incoming.method === "GET" || incoming.method === "HEAD" ? undefined : Readable.toWeb(incoming), duplex: "half",
      } as RequestInit);
      const source = incoming.headers["cf-connecting-ip"];
      const response = incoming.method === "GET" && incoming.url === "/healthz"
        ? json(200, { ok: true })
        : await handle(request, exchange, typeof source === "string" ? source : incoming.socket.remoteAddress ?? "unknown");
      outgoing.writeHead(response.status, Object.fromEntries(response.headers));
      outgoing.end(new Uint8Array(await response.arrayBuffer()));
    } catch { outgoing.writeHead(503).end(); }
    finally { pending--; }
  });
  server.requestTimeout = 10_000;
  server.headersTimeout = 10_000;
  try {
    await sweep();
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(port, "127.0.0.1", resolve);
    });
  } catch (error) { clearInterval(timer); database.close(); throw error; }
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Missing service address");
  return {
    address: `http://127.0.0.1:${address.port}`,
    async close() {
      clearInterval(timer);
      await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
      await cleaning;
      database.close();
    },
  };
}
