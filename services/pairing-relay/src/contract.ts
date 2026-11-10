export const MAX_BODY_BYTES = 12 * 1024;
export const MAX_TICKET_BYTES = 8 * 1024;
export const TTL_MS = 60_000;
export const CLOCK_SKEW_MS = 5000;

// The timestamp gives immutable records a non-reusable cleanup generation.
// The remaining 208 random bits retain unguessable retry identifiers.
export function requestId(now = Date.now()): string {
  return now.toString(16).padStart(12, "0") + randomToken().slice(12);
}
export function requestTime(id: string): number { return Number.parseInt(id.slice(0, 12), 16); }
export function generation(time: number): string { return `g/${Math.floor(time / TTL_MS).toString().padStart(12, "0")}/`; }

export type Action = "publish" | "redeem" | "cancel";
export interface Publication { request_id: string; ticket: string; expires_at_ms?: number }
export interface Redemption { request_id: string; code: string }
export interface Cancellation { code: string; cancel_token: string }
export interface Result { status: number; body?: object }

export function action(path: string): Action | undefined {
  if (path === "/v1/codes") return "publish";
  if (path === "/v1/codes/redeem") return "redeem";
  if (path === "/v1/codes/cancel") return "cancel";
}

export function valid(kind: Action, value: unknown): boolean {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const body = value as Record<string, unknown>;
  if (kind === "publish") return Object.keys(body).every(key => ["request_id", "ticket", "expires_at_ms"].includes(key))
    && (body.expires_at_ms === undefined || Number.isSafeInteger(body.expires_at_ms) && Number(body.expires_at_ms) > 0)
    && token(body.request_id) && typeof body.ticket === "string"
    && body.ticket.length > 0 && new TextEncoder().encode(body.ticket).length <= MAX_TICKET_BYTES;
  if (Object.keys(body).length !== 2) return false;
  if (kind === "redeem") return token(body.request_id) && code(body.code);
  return code(body.code) && token(body.cancel_token);
}

export function code(value: unknown): value is string {
  return typeof value === "string" && /^\d{6}$/.test(value);
}
export function token(value: unknown): value is string {
  return typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
}
export function randomToken(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(32)), value => value.toString(16).padStart(2, "0")).join("");
}
export function randomCode(): string {
  const random = new Uint32Array(1);
  do { crypto.getRandomValues(random); } while (random[0] >= 4_294_000_000);
  return (random[0] % 1_000_000).toString().padStart(6, "0");
}
export function failure(status: number, error: string): Result { return { status, body: { error } }; }
