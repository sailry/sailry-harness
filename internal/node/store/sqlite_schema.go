package store

import (
	"fmt"
	"strings"
)

// sqliteSchema supplies the prescribed FTS5 indexes for the SQLite fallback.
func sqliteSchema(source string) string {
	for _, table := range []string{"chunks", "symbols"} {
		index := table + "_fts"
		native := fmt.Sprintf("CREATE INDEX IF NOT EXISTS %s ON %s USING fts(field0,field1,cjk0,cjk1) WITH(tokenizer='whitespace');", index, table)
		fallback := fmt.Sprintf(`CREATE VIRTUAL TABLE IF NOT EXISTS %[1]s USING fts5(field0,field1,cjk0,cjk1,content='%[2]s',content_rowid='id',tokenize='ascii tokenchars ''+#''');
CREATE TRIGGER IF NOT EXISTS %[1]s_insert AFTER INSERT ON %[2]s BEGIN
 INSERT INTO %[1]s(rowid,field0,field1,cjk0,cjk1) VALUES(new.id,new.field0,new.field1,new.cjk0,new.cjk1);
END;
CREATE TRIGGER IF NOT EXISTS %[1]s_delete AFTER DELETE ON %[2]s BEGIN
 INSERT INTO %[1]s(%[1]s,rowid,field0,field1,cjk0,cjk1) VALUES('delete',old.id,old.field0,old.field1,old.cjk0,old.cjk1);
END;
CREATE TRIGGER IF NOT EXISTS %[1]s_update AFTER UPDATE ON %[2]s BEGIN
 INSERT INTO %[1]s(%[1]s,rowid,field0,field1,cjk0,cjk1) VALUES('delete',old.id,old.field0,old.field1,old.cjk0,old.cjk1);
 INSERT INTO %[1]s(rowid,field0,field1,cjk0,cjk1) VALUES(new.id,new.field0,new.field1,new.cjk0,new.cjk1);
END;`, index, table)
		source = strings.ReplaceAll(source, native, fallback)
	}
	return source
}
