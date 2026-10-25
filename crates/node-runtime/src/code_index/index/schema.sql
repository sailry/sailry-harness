CREATE TABLE IF NOT EXISTS metadata (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  version INTEGER NOT NULL,
  revision INTEGER NOT NULL
);
INSERT OR IGNORE INTO metadata VALUES(1,1,0);
CREATE TABLE IF NOT EXISTS files (
  path TEXT PRIMARY KEY,
  fingerprint TEXT NOT NULL,
  hash TEXT,
  state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS symbols (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
  name TEXT NOT NULL,
  language TEXT NOT NULL,
  body TEXT NOT NULL,
  field0 TEXT NOT NULL,
  field1 TEXT NOT NULL,
  cjk0 TEXT NOT NULL,
  cjk1 TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS symbols_path ON symbols(path);
CREATE INDEX IF NOT EXISTS symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS symbols_language ON symbols(language);
CREATE INDEX IF NOT EXISTS symbols_fts ON symbols USING fts(field0,field1,cjk0,cjk1) WITH(tokenizer='whitespace');
