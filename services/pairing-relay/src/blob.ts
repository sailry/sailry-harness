import { getStore, PreconditionFailedError, type Store } from "@edgeone/pages-blob";
import { failed } from "./diagnostics";
import type { ManagedObjects, RecordValue } from "./objects";

/** Native EdgeOne storage; no public object URLs or eventual-consistency reads. */
export class BlobObjects implements ManagedObjects {
  constructor(private readonly store: Pick<Store, "get" | "setJSON" | "list" | "delete"> = getStore("sailry-pairing-v1")) {}

  async list(cursor?: string) {
    try {
      const page = await this.store.list({ prefix: "g/", cursor, limit: 128, paginate: false, consistency: "strong" });
      return { keys: page.blobs.map(blob => blob.key), cursor: page.cursor };
    } catch (error) { return failed("storage_list", error); }
  }

  async remove(key: string): Promise<void> {
    try { await this.store.delete(key); }
    catch (error) { failed("storage_delete", error); }
  }

  async get(key: string): Promise<RecordValue | null> {
    try {
      const record: unknown = await this.store.get(key, { type: "json", consistency: "strong" });
      if (record === null) return null;
      if (typeof record !== "object" || !("expires" in record) || !("value" in record)
        || !Number.isSafeInteger(record.expires) || typeof record.value !== "string") {
        throw new Error("Invalid pairing storage record");
      }
      return record as RecordValue;
    } catch (error) { return failed("storage_get", error); }
  }

  async create(key: string, record: RecordValue): Promise<boolean> {
    try {
      await this.store.setJSON(key, record, { onlyIfNew: true, cacheControl: "no-store" });
      return true;
    } catch (error) {
      if (error instanceof PreconditionFailedError) return false;
      // Timeouts and quota failures are not evidence that another writer won.
      return failed("storage_put", error);
    }
  }
}
