PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS metadata (singleton INTEGER PRIMARY KEY CHECK(singleton=1), version INTEGER NOT NULL);
INSERT OR IGNORE INTO metadata VALUES(1,1);
CREATE TABLE IF NOT EXISTS collections (
  id TEXT PRIMARY KEY,
  generation INTEGER NOT NULL,
  scope BLOB NOT NULL,
  rebuild INTEGER NOT NULL,
  model TEXT,
  dimension INTEGER
);
CREATE TABLE IF NOT EXISTS sources (
  id INTEGER PRIMARY KEY,
  collection TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  locator TEXT NOT NULL,
  revision TEXT NOT NULL,
  modified_ms INTEGER NOT NULL,
  tags TEXT NOT NULL,
  embedded INTEGER NOT NULL,
  UNIQUE(collection,locator)
);
CREATE TABLE IF NOT EXISTS chunks (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  source INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  title TEXT NOT NULL,
  text TEXT NOT NULL,
  position TEXT NOT NULL,
  vector BLOB
);
CREATE TABLE IF NOT EXISTS file_stamps (
  source INTEGER PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
  size INTEGER NOT NULL,
  modified_seconds INTEGER NOT NULL,
  modified_nanos INTEGER NOT NULL,
  extractor BLOB NOT NULL DEFAULT X'0000000000000000000000000000000000000000000000000000000000000000'
);
CREATE INDEX IF NOT EXISTS chunks_source ON chunks(source);
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(title,text,cjk_title,cjk_text,tokenize='unicode61 remove_diacritics 2 tokenchars ''+#''');
CREATE TRIGGER IF NOT EXISTS delete_chunk AFTER DELETE ON chunks BEGIN
  DELETE FROM chunks_fts WHERE rowid=old.id;
END;
