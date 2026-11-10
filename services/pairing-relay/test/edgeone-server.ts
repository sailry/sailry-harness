// Local Node handler fixture. SQLite emulates Blob primitives, not the EdgeOne cloud.
import { createServer } from "node:http";
import { Readable } from "node:stream";
import { PreconditionFailedError, type Store } from "@edgeone/pages-blob";
import { BlobObjects } from "../src/blob";
import { serve } from "../src/edgeone";
import { SqliteObjects } from "../src/sqlite-objects";
import { SqliteStorage } from "./storage";
import type { RecordValue } from "../src/objects";

export async function start() {
  const database = new SqliteStorage();
  const records = new SqliteObjects(database);
  const sdk = {
    get: (key: string) => records.get(key),
    setJSON: async (key: string, value: RecordValue) => {
      if (!await records.create(key, value)) throw new PreconditionFailedError();
    },
    delete: (key: string) => records.remove(key),
    list: async ({ cursor }: { cursor?: string }) => {
      const page = await records.list(cursor);
      return { blobs: page.keys.map(key => ({ key, etag: "fixture" })), directories: [], cursor: page.cursor };
    },
  } as unknown as Pick<Store, "get" | "setJSON" | "list" | "delete">;
  const objects = new BlobObjects(sdk);
  const server = createServer(async (incoming, outgoing) => {
    try {
      const init = { method: incoming.method, headers: incoming.headers,
        body: incoming.method === "GET" || incoming.method === "HEAD" ? undefined : Readable.toWeb(incoming), duplex: "half" } as RequestInit;
      const request = new Request(`http://127.0.0.1${incoming.url}`, init);
      const response = await serve({ request, clientIp: incoming.socket.remoteAddress }, objects);
      outgoing.writeHead(response.status, Object.fromEntries(response.headers));
      outgoing.end(new Uint8Array(await response.arrayBuffer()));
    } catch { outgoing.writeHead(503).end(); }
  });
  await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Missing fixture address");
  return { ready: new URL(`http://127.0.0.1:${address.port}`), dispose: async () => {
    await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    database.close();
  } };
}
