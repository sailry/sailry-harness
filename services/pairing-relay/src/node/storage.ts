import { createRequire } from "node:module";
import type { DatabaseSync as Database, SQLInputValue } from "node:sqlite";
import type { Storage } from "../sqlite-objects";

// Vite 5 predates this Node builtin; let Node resolve it, not Vite's resolver.
const { DatabaseSync } = createRequire(import.meta.url)("node:sqlite") as typeof import("node:sqlite");

// The standalone service shares the immutable SQLite records with the Worker.
export class SqliteStorage implements Storage {
  private readonly database: Database;
  constructor(path = ":memory:") { this.database = new DatabaseSync(path); this.database.exec("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;"); }
  readonly sql = {
    exec: <T>(query: string, ...bindings: unknown[]) => {
      let rows: T[] = [];
      if (query.trimStart().startsWith("CREATE")) this.database.exec(query);
      else {
        const statement = this.database.prepare(query);
        const named = Object.fromEntries(bindings.map((value, index) => [`?${index + 1}`, value as SQLInputValue]));
        rows = statement.all(named) as T[];
      }
      return { toArray: () => rows, [Symbol.iterator]: () => rows[Symbol.iterator]() };
    },
  };
  close(): void { this.database.close(); }
}
