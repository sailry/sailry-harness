import { CLOCK_SKEW_MS, TTL_MS, failure, generation, randomCode, randomToken, requestTime, valid,
  type Action, type Cancellation, type Publication, type Redemption, type Result } from "./contract";
import { elect, type Objects, type RecordValue } from "./objects";

export interface Limits { entries: number; publish: number; redeem: number; global: number }
export const limits: Limits = { entries: 256, publish: 6, redeem: 5, global: 120 };

interface Entry { code: string; request: string; ticket: string; cancel: string; expires: number }

/** Shared short-code logic. Only immutable create/read primitives arbitrate races. */
export class Exchange {
  constructor(private readonly store: Objects, private readonly budget: Limits = limits,
    private readonly nextCode = randomCode, private readonly clock = Date.now) {
    for (const value of Object.values(budget)) {
      if (!Number.isSafeInteger(value) || value <= 0) throw new Error("Invalid exchange limit");
    }
  }

  async execute(kind: Action, body: unknown, source: string, now: number): Promise<Result> {
    if (!valid(kind, body) || !Number.isSafeInteger(now) || now < 0 || source.length > 128)
      return failure(400, "invalid_request");
    const started = this.clock();
    // Include time spent awaiting storage without requiring a mutable test clock.
    const current = () => now + Math.max(0, this.clock() - started);
    if (kind !== "cancel") {
      const time = requestTime((body as Publication | Redemption).request_id);
      if (time > now + CLOCK_SKEW_MS) return failure(400, "invalid_request");
      if (time + TTL_MS <= now) return failure(kind === "publish" ? 410 : 404, "code_unavailable");
    }
    const prefix = generation(now);
    // Admit globally before creating source records, bounding unique-source growth.
    if (!await this.slot(`${prefix}rate/${kind}/global/`, this.budget.global, now)
      || !await this.slot(`${prefix}rate/${kind}/${await digest(source)}/`,
        kind === "publish" ? this.budget.publish : this.budget.redeem, now))
      return failure(429, "rate_limited");
    if (kind === "publish") return this.publish(body as Publication, current);
    if (kind === "redeem") return this.redeem(body as Redemption, current);
    return this.cancel(body as Cancellation, current());
  }

  private async publish(body: Publication, now: () => number): Promise<Result> {
    const time = requestTime(body.request_id);
    const expires = Math.min(time + TTL_MS, now() + TTL_MS, body.expires_at_ms ?? Infinity);
    if (expires <= now()) return failure(410, "code_expired");
    const prefix = generation(time);
    const publication = `${prefix}publish/${body.request_id}`;
    let record = await this.store.get(publication);
    if (!record) {
      const residue = Math.floor(time / TTL_MS) % 3;
      for (let attempt = 0; attempt < 16; attempt++) {
        if (expires <= now()) return failure(410, "code_expired");
        const random = Number(this.nextCode());
        if (!Number.isInteger(random) || random < 0 || random >= 999_999) continue;
        const code = (Math.floor(random / 3) * 3 + residue).toString().padStart(6, "0");
        if (!await this.slot(`${prefix}capacity/`, this.budget.entries, now())) return failure(503, "capacity");
        const entry: Entry = { code, request: body.request_id, ticket: body.ticket, cancel: randomToken(), expires };
        const candidate = { expires, value: JSON.stringify(entry) };
        if (!await this.store.create(`${prefix}pin/${code}`, candidate)) continue;
        // Orphan candidates are never redeemable: redemption verifies this election.
        record = await elect(this.store, publication, candidate);
        break;
      }
    }
    if (!record) return failure(503, "capacity");
    const entry = JSON.parse(record.value) as Entry;
    if (entry.expires <= now()) return failure(410, "code_expired");
    if (entry.ticket !== body.ticket || await this.store.get(`${prefix}cancel/${entry.code}`))
      return failure(409, "request_conflict");
    return { status: 201, body: { code: entry.code, cancel_token: entry.cancel, expires_at_ms: entry.expires } };
  }

  private async redeem(body: Redemption, now: () => number): Promise<Result> {
    const found = await this.find(body.code, now());
    if (!found) return failure(404, "code_unavailable");
    const { entry, prefix } = found;
    const publication = await this.store.get(`${prefix}publish/${entry.request}`);
    if (!publication || (JSON.parse(publication.value) as Entry).code !== body.code
      || await this.store.get(`${prefix}cancel/${body.code}`)) return failure(404, "code_unavailable");
    const claim = `${generation(requestTime(body.request_id))}request/${body.request_id}`;
    const bound = await elect(this.store, claim, { expires: requestTime(body.request_id) + TTL_MS, value: `${prefix}${body.code}` });
    if (bound.value !== `${prefix}${body.code}`) return failure(409, "request_conflict");
    const winner = await elect(this.store, `${prefix}claim/${body.code}`, { expires: entry.expires, value: body.request_id });
    // Recheck cancellation and expiry after asynchronous arbitration.
    if (winner.value !== body.request_id || entry.expires <= now()
      || await this.store.get(`${prefix}cancel/${body.code}`)) return failure(404, "code_unavailable");
    if (entry.expires <= now() || requestTime(body.request_id) + TTL_MS <= now()) return failure(404, "code_unavailable");
    return { status: 200, body: { ticket: entry.ticket, expires_at_ms: entry.expires } };
  }

  private async cancel(body: Cancellation, now: number): Promise<Result> {
    const found = await this.find(body.code, now);
    if (!found) return { status: 204 };
    if (found.entry.cancel !== body.cancel_token) return failure(403, "invalid_cancel_token");
    await this.store.create(`${found.prefix}cancel/${body.code}`, { expires: found.entry.expires, value: "cancelled" });
    return { status: 204 };
  }

  private async find(code: string, now: number): Promise<{ entry: Entry; prefix: string } | null> {
    const bucket = Math.floor(now / TTL_MS);
    // Residues distinguish all potentially live generations, including clock skew.
    const selected = [bucket - 1, bucket, bucket + 1].find(value => value >= 0 && value % 3 === Number(code) % 3);
    if (selected === undefined) return null;
    const prefix = generation(selected * TTL_MS);
    const record = await this.store.get(`${prefix}pin/${code}`);
    if (!record || record.expires <= now) return null;
    return { entry: JSON.parse(record.value) as Entry, prefix };
  }

  private async slot(prefix: string, maximum: number, now: number): Promise<boolean> {
    const expires = (Math.floor(now / TTL_MS) + 1) * TTL_MS;
    for (let index = 0; index < maximum; index++) {
      // Reads avoid unnecessary conditional-write failures on occupied slots.
      const key = `${prefix}${index}`;
      if (!await this.store.get(key) && await this.store.create(key, { expires, value: "1" })) return true;
    }
    return false;
  }
}

async function digest(value: string): Promise<string> {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return Array.from(new Uint8Array(bytes), byte => byte.toString(16).padStart(2, "0")).join("");
}
