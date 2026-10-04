// SPDX-License-Identifier: GPL-3.0-only
// Bounded body reading adapted from sailry-code 67ae9fa0; see ../NOTICE.
import { MAX_BODY_BYTES, action, type Action, type Result } from "./contract";

export interface Exchange {
  execute(kind: Action, body: unknown, source: string, now: number): Result | Promise<Result>;
}

export async function handle(request: Request, registry: Exchange, source: string, now?: number): Promise<Response> {
  const url = new URL(request.url);
  const kind = action(url.pathname);
  if (request.method !== "POST" || !kind || url.search) return json(404, { error: "not_found" });
  try {
    const body = JSON.parse(await readBody(request));
    const result = await registry.execute(kind, body, source, now ?? Date.now());
    return response(result);
  } catch (error) {
    if (error instanceof BodyError) return json(error.status, { error: "invalid_body" });
    if (error instanceof SyntaxError) return json(400, { error: "invalid_json" });
    // Never reflect bodies, tickets or storage diagnostics into an HTTP response.
    return json(503, { error: "unavailable" });
  }
}

export class BodyError extends Error { constructor(readonly status: number) { super("invalid body"); } }
export async function readBody(request: Request): Promise<string> {
  if (Number(request.headers.get("Content-Length")) > MAX_BODY_BYTES) throw new BodyError(413);
  if (!request.body) throw new BodyError(400);
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let received = 0;
  let expired = false;
  const timer = setTimeout(() => { expired = true; void reader.cancel().catch(() => {}); }, 5000);
  try {
    while (true) {
      const next = await reader.read();
      if (expired) throw new BodyError(408);
      if (next.done) break;
      received += next.value.byteLength;
      if (received > MAX_BODY_BYTES) { void reader.cancel().catch(() => {}); throw new BodyError(413); }
      chunks.push(next.value);
    }
    const bytes = new Uint8Array(received);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    try { return new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
    catch { throw new BodyError(400); }
  } finally { clearTimeout(timer); reader.releaseLock(); }
}

export function response(result: Result): Response { return json(result.status, result.body); }
export function json(status: number, body?: object): Response {
  const headers = new Headers({ "Content-Type": "application/json", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" });
  if (status === 429 || status === 503) headers.set("Retry-After", "60");
  return new Response(status === 204 ? null : JSON.stringify(body), { status, headers });
}
