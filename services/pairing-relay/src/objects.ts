/** Immutable, strongly consistent records. Expired keys must not be reused. */
export interface RecordValue { expires: number; value: string }
export interface Objects {
  get(key: string): Promise<RecordValue | null>;
  create(key: string, record: RecordValue): Promise<boolean>;
}

export interface ManagedObjects extends Objects {
  list(cursor?: string): Promise<{ keys: string[]; cursor?: string }>;
  remove(key: string): Promise<void>;
}

/** Return the existing winner, including after a lost conditional-write response. */
export async function elect(store: Objects, key: string, record: RecordValue): Promise<RecordValue> {
  await store.create(key, record);
  const winner = await store.get(key);
  if (!winner) throw new Error("Conditional write is not visible");
  return winner;
}
