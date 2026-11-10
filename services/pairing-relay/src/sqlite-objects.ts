import type { ManagedObjects, RecordValue } from "./objects";
export interface Storage {
  sql: { exec<T>(query: string, ...bindings: unknown[]): { toArray(): T[] } };
}

/** Cloudflare's native SQLite implements the same immutable-record boundary. */
export class SqliteObjects implements ManagedObjects {
  constructor(private readonly storage: Storage) {
    storage.sql.exec("CREATE TABLE IF NOT EXISTS pairing_objects (key TEXT PRIMARY KEY, expires INTEGER NOT NULL, value TEXT NOT NULL)");
  }

  async list(cursor = "") {
    const rows = this.storage.sql.exec<{ key: string }>(
      "SELECT key FROM pairing_objects WHERE key LIKE 'g/%' AND key > ?1 ORDER BY key LIMIT 128", cursor,
    ).toArray();
    return { keys: rows.map(row => row.key), cursor: rows.length === 128 ? rows[127].key : undefined };
  }

  async remove(key: string): Promise<void> {
    this.storage.sql.exec("DELETE FROM pairing_objects WHERE key = ?1", key);
  }

  async get(key: string): Promise<RecordValue | null> {
    return this.storage.sql.exec<RecordValue>(
      "SELECT expires, value FROM pairing_objects WHERE key = ?1", key,
    ).toArray()[0] ?? null;
  }

  async create(key: string, record: RecordValue): Promise<boolean> {
    return this.storage.sql.exec<{ key: string }>(
      "INSERT INTO pairing_objects (key, expires, value) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING RETURNING key",
      key, record.expires, record.value,
    ).toArray().length === 1;
  }
}
