package store

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
)

const legacyCodeIndexFixture = `
CREATE TABLE metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,revision INTEGER NOT NULL);
INSERT INTO metadata VALUES(1,1,11);
CREATE TABLE files(path TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,hash TEXT,state TEXT NOT NULL);
CREATE TABLE symbols(id INTEGER PRIMARY KEY,path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,name TEXT NOT NULL,language TEXT NOT NULL,body TEXT NOT NULL,field0 TEXT NOT NULL,field1 TEXT NOT NULL,cjk0 TEXT NOT NULL,cjk1 TEXT NOT NULL);
CREATE INDEX symbols_path ON symbols(path);
CREATE INDEX symbols_name ON symbols(name);
CREATE INDEX symbols_language ON symbols(language);
CREATE INDEX symbols_fts ON symbols USING fts(field0,field1,cjk0,cjk1) WITH(tokenizer='whitespace');
CREATE TABLE refs(id INTEGER PRIMARY KEY,path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,name TEXT NOT NULL,kind TEXT NOT NULL,start_line INTEGER NOT NULL,start_column INTEGER NOT NULL,start_byte INTEGER NOT NULL,receiver INTEGER NOT NULL,scope TEXT,scope_kind TEXT,scope_line INTEGER,scope_end_line INTEGER);
CREATE INDEX refs_path ON refs(path,start_byte);
CREATE INDEX refs_name ON refs(name);
CREATE TABLE imports(path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,module TEXT NOT NULL);
CREATE INDEX imports_path ON imports(path);
INSERT INTO files VALUES('src/old.go','v1','hash','{}');
INSERT INTO symbols VALUES(1,'src/old.go','Old','go','{}','old','','','');
`

func TestOpenCodeIndexRebuildsOnlyRecognizedCache(t *testing.T) {
	ctx := context.Background()
	for _, fixture := range []struct {
		name, extra string
		rebuild     bool
	}{
		{name: "legacy", rebuild: true},
		{name: "future", extra: "UPDATE metadata SET version=3"},
		{name: "foreign table", extra: "CREATE TABLE project_notes(body TEXT); INSERT INTO project_notes VALUES('keep')"},
		{name: "foreign column", extra: "ALTER TABLE files ADD COLUMN notes TEXT; UPDATE files SET notes='keep'"},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "index.turso")
			old, err := OpenHandle(ctx, path)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = old.Close() })
			schemaExecute(t, old, legacyCodeIndexFixture+fixture.extra)
			if err := old.Close(); err != nil {
				t.Fatal(err)
			}
			index, err := OpenCodeIndex(ctx, path)
			if index != nil {
				t.Cleanup(func() { _ = index.Close() })
			}
			if !fixture.rebuild {
				if !errors.Is(err, errCodeIndexVersion) || index != nil {
					t.Fatalf("expected incompatible schema, got index=%v error=%v", index, err)
				}
				preserved, err := OpenHandle(ctx, path)
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = preserved.Close() })
				if got := schemaInteger(t, preserved, "SELECT count(*) FROM files WHERE path='src/old.go'"); got != 1 {
					t.Fatal("unsupported cache was removed")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if version := schemaInteger(t, index, "SELECT version FROM metadata WHERE singleton=1"); version != codeIndexVersion {
				t.Fatalf("rebuilt schema version %d", version)
			}
			if count := schemaInteger(t, index, "SELECT count(*) FROM files"); count != 0 {
				t.Fatalf("rebuilt cache contains %d stale files", count)
			}
			if columns := schemaInteger(t, index, "SELECT count(*) FROM pragma_table_info('symbols') WHERE name='file_id'"); columns != 1 {
				t.Fatal("rebuilt symbols are missing their file identity")
			}
		})
	}
}
