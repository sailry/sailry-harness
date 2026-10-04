import { getStore, PreconditionFailedError, type Store } from "@edgeone/pages-blob";
import type { ManagedObjects, RecordValue } from "./objects";

/** Native EdgeOne storage; no public object URLs or eventual-consistency reads. */
export class BlobObjects implements ManagedObjects {
  constructor(private readonly store: Pick<Store, "get" | "setJSON" | "list" | "delete"> = getStore("sailry-pairing-v1")) {}

  async list(cursor?: string) {
    const page = await this.store.list({ prefix: "g/", cursor, limit: 128, paginate: false, consistency: "strong" });
    return { keys: page.blobs.map(blob => blob.key), cursor: page.cursor };
  }

  async remove(key: string): Promise<void> { await this.store.delete(key); }

  async get(key: string): Promise<RecordValue | null> {
    const record: unknown = await this.store.get(key, { type: "json", consistency: "strong" });
    if (record === null) return null;
    if (typeof record !== "object" || !("expires" in record) || !("value" in record)
      || !Number.isSafeInteger(record.expires) || typeof record.value !== "string") {
      throw new Error("Invalid pairing storage record");
    }
    return record as RecordValue;
  }

  async create(key: string, record: RecordValue): Promise<boolean> {
    try {
      await this.store.setJSON(key, record, { onlyIfNew: true, cacheControl: "no-store" });
      return true;
    } catch (error) {
      if (error instanceof PreconditionFailedError) return false;
      // Timeouts and quota failures are not evidence that another writer won.
      throw error;
    }
  }
}
