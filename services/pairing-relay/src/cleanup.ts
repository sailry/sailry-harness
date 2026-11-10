import { TTL_MS } from "./contract";
import type { ManagedObjects } from "./objects";

/** Bound each pass; generations are never reused, so stale deletes cannot hit new codes. */
export async function cleanup(store: ManagedObjects, now: number, cursor?: string) {
  const page = await store.list(cursor);
  const expired = page.keys.filter(key => {
    const match = /^g\/(\d{12})\//.exec(key);
    return match && (Number(match[1]) + 3) * TTL_MS <= now;
  });
  for (let offset = 0; offset < expired.length; offset += 8) {
    await Promise.all(expired.slice(offset, offset + 8).map(key => store.remove(key)));
  }
  return { removed: expired.length, cursor: page.cursor };
}
