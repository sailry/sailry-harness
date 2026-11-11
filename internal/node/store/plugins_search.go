package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func pluginSearchTable(name string) string {
	return fmt.Sprintf("plugin_search_%x", blake3.Sum256([]byte(name)))
}

func PluginSearchDefinition(name string) string {
	table := pluginSearchTable(name)
	index := table + "_fts"
	return fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %[1]s(id INTEGER PRIMARY KEY,key TEXT NOT NULL UNIQUE,field0 TEXT NOT NULL,field1 TEXT NOT NULL,cjk0 TEXT NOT NULL,cjk1 TEXT NOT NULL,tags TEXT NOT NULL,ordinal INTEGER NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS %[2]s USING fts5(field0,field1,cjk0,cjk1,content='%[1]s',content_rowid='id',tokenize='ascii tokenchars ''+#''');
CREATE TRIGGER IF NOT EXISTS %[2]s_insert AFTER INSERT ON %[1]s BEGIN
INSERT INTO %[2]s(rowid,field0,field1,cjk0,cjk1) VALUES(new.id,new.field0,new.field1,new.cjk0,new.cjk1);
END;
CREATE TRIGGER IF NOT EXISTS %[2]s_delete AFTER DELETE ON %[1]s BEGIN
INSERT INTO %[2]s(%[2]s,rowid,field0,field1,cjk0,cjk1) VALUES('delete',old.id,old.field0,old.field1,old.cjk0,old.cjk1);
END;
CREATE TRIGGER IF NOT EXISTS %[2]s_update AFTER UPDATE ON %[1]s BEGIN
INSERT INTO %[2]s(%[2]s,rowid,field0,field1,cjk0,cjk1) VALUES('delete',old.id,old.field0,old.field1,old.cjk0,old.cjk1);
INSERT INTO %[2]s(rowid,field0,field1,cjk0,cjk1) VALUES(new.id,new.field0,new.field1,new.cjk0,new.cjk1);
END;`, table, index)
}

func pluginSearchExists(ctx context.Context, db *sql.Conn, name string) (bool, error) {
	var exists bool
	err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)", pluginSearchTable(name)).Scan(&exists)
	return exists, err
}

func replacePluginSearch(ctx context.Context, db *sql.Conn, name, key string, index *protocol.PluginStorageIndex) error {
	exists, err := pluginSearchExists(ctx, db, name)
	if err != nil {
		return err
	}
	if !exists {
		if index == nil {
			return nil
		}
		if _, err := db.ExecContext(ctx, PluginSearchDefinition(name)); err != nil {
			return err
		}
	}
	table := pluginSearchTable(name)
	if index == nil {
		_, err = db.ExecContext(ctx, "DELETE FROM "+table+" WHERE key=?1", key)
		return err
	}
	tags, err := json.Marshal(index.Tags)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO "+table+`(key,field0,field1,cjk0,cjk1,tags,ordinal) VALUES(?1,?2,?3,?4,?5,?6,?7)
ON CONFLICT(key) DO UPDATE SET field0=excluded.field0,field1=excluded.field1,cjk0=excluded.cjk0,cjk1=excluded.cjk1,tags=excluded.tags,ordinal=excluded.ordinal`,
		key, knowledge.Field(index.Fields[0]), knowledge.Field(index.Fields[1]), knowledge.Index(index.Fields[0]), knowledge.Index(index.Fields[1]), string(tags), index.Order)
	return err
}

func validatePluginSearch(query protocol.PluginStorageSearch) error {
	if query.Limit == 0 || query.Weights[0] == 0 || query.Weights[1] == 0 ||
		query.Query != nil && strings.ContainsRune(*query.Query, 0) {
		return invalid("invalid plugin search query")
	}
	for _, values := range [][]string{query.Terms, query.All, query.Any} {
		for _, value := range values {
			if value == "" || strings.ContainsRune(value, 0) {
				return invalid("invalid plugin search query")
			}
		}
	}
	return nil
}

// pluginSearchRows runs the keyword search of one package's indexed values.
// It returns no rows when the package has no index or the terms match
// nothing.
func pluginSearchRows(ctx context.Context, db *sql.Conn, name string, query protocol.PluginStorageSearch, columns string, limit, offset int) (*sql.Rows, error) {
	exists, err := pluginSearchExists(ctx, db, name)
	if err != nil || !exists {
		return nil, err
	}
	expressions := []string{}
	for _, term := range query.Terms {
		if expression := knowledge.Phrase(term); expression != "" {
			expressions = append(expressions, expression)
		}
	}
	if query.Query != nil {
		if expression := knowledge.Query(*query.Query); expression != "" {
			expressions = append(expressions, "("+expression+")")
		}
	}
	expression := strings.Join(expressions, " OR ")
	if len(query.Terms) != 0 && expression == "" {
		return nil, nil
	}
	table := pluginSearchTable(name)
	index := table + "_fts"
	parameters := []any{name}
	parameter := func(value any) string {
		parameters = append(parameters, value)
		return fmt.Sprintf("?%d", len(parameters))
	}
	where := []string{"v.name=?1", "v.value IS NOT NULL"}
	join := table + " AS f JOIN plugin_values AS v ON v.key=f.key"
	order := "f.ordinal DESC,v.key ASC"
	if expression != "" {
		join += " JOIN " + index + " ON " + index + ".rowid=f.id"
		where = append(where, index+" MATCH "+parameter(expression))
		order = fmt.Sprintf("bm25(%s,%d,%d,%d,%d) ASC,", index, query.Weights[0], query.Weights[1], query.Weights[0], query.Weights[1]) + order
	}
	for _, tag := range query.All {
		where = append(where, "EXISTS(SELECT 1 FROM json_each(f.tags) WHERE value="+parameter(tag)+")")
	}
	if len(query.Any) != 0 {
		choices := []string{}
		for _, tag := range query.Any {
			choices = append(choices, parameter(tag))
		}
		where = append(where, "EXISTS(SELECT 1 FROM json_each(f.tags) WHERE value IN("+strings.Join(choices, ",")+"))")
	}
	sqlText := "SELECT " + columns + " FROM " + join + " WHERE " + strings.Join(where, " AND ") +
		" ORDER BY " + order + " LIMIT " + parameter(limit) + " OFFSET " + parameter(offset)
	return db.QueryContext(ctx, sqlText, parameters...)
}

// PluginValueRank is one keyword hit of a semantic plan: the key and the
// revision it was ranked at.
type PluginValueRank struct {
	Key      string
	Revision uint64
}

// rankedPluginValues ranks every keyword match of a package. Semantic fusion
// receives keys and revisions only, not the values.
func rankedPluginValues(ctx context.Context, db *sql.Conn, name string, query protocol.PluginStorageSearch) ([]PluginValueRank, error) {
	if err := validatePluginSearch(query); err != nil {
		return nil, err
	}
	rows, err := pluginSearchRows(ctx, db, name, query, "v.key,v.revision", maxPluginKeys, 0)
	if err != nil || rows == nil {
		return nil, err
	}
	defer rows.Close()
	var ranked []PluginValueRank
	for rows.Next() {
		var rank PluginValueRank
		if err := rows.Scan(&rank.Key, &rank.Revision); err != nil {
			return nil, err
		}
		if rank.Revision == 0 {
			return nil, invalid("invalid stored plugin search value")
		}
		ranked = append(ranked, rank)
	}
	return ranked, rows.Err()
}

func SearchPluginValues(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, query protocol.PluginStorageSearch) (protocol.PluginStorageSearchPage, error) {
	page := protocol.DefaultPluginStorageSearchPage()
	page.Mode, page.NowMs = protocol.KnowledgeModeKeyword, uint64(time.Now().UnixMilli())
	if err := validatePluginSearch(query); err != nil {
		return page, err
	}
	info, err := ResolvePluginInfo(ctx, db, reference)
	if err != nil {
		return page, err
	}
	limit := min(query.Limit, 100)
	rows, err := pluginSearchRows(ctx, db, reference.Name, query, "v.key,v.revision,v.value", int(limit)+1, int(query.Offset))
	if err != nil || rows == nil {
		return page, err
	}
	defer rows.Close()
	next := func() {
		value := uint16(min(uint32(query.Offset)+uint32(len(page.Entries)), math.MaxUint16))
		page.Next = &value
	}
	for rows.Next() {
		if len(page.Entries) == int(limit) {
			next()
			break
		}
		entry := protocol.DefaultPluginStorageEntry()
		entry.Present = true
		if err := rows.Scan(&entry.Key, &entry.Revision, &entry.Value); err != nil {
			return page, err
		}
		if entry.Revision == 0 || !json.Valid(entry.Value) {
			return page, invalid("invalid stored plugin search value")
		}
		entry.Value, err = plugins.ProjectStorageValue(plugins.StorageCollection(info, protocol.PluginStorageScopeNode, entry.Key), entry.Value)
		if err != nil {
			return page, err
		}
		page.Entries = append(page.Entries, entry)
		body, err := json.Marshal(page)
		if err != nil {
			return page, err
		}
		if len(body)+len(page.Entries)*2+8 > 512*1024 {
			page.Entries = page.Entries[:len(page.Entries)-1]
			next()
			break
		}
	}
	return page, rows.Err()
}

func PluginSearchDefinitions(ctx context.Context, db *sql.Conn) ([]string, error) {
	rows, err := db.QueryContext(ctx, "SELECT DISTINCT name FROM plugin_values WHERE index_data IS NOT NULL UNION SELECT name FROM plugins ORDER BY name")
	if err != nil {
		return nil, err
	}
	names := []string{}
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			rows.Close()
			return nil, err
		}
		names = append(names, name)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return nil, err
	}
	definitions := []string{}
	for _, name := range names {
		exists, err := pluginSearchExists(ctx, db, name)
		if err != nil {
			return nil, err
		}
		if exists {
			definitions = append(definitions, PluginSearchDefinition(name))
		}
	}
	return definitions, nil
}
