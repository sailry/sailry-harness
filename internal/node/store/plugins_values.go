package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func pluginKey(key string, prefix bool) error {
	if (!prefix && key == "") || len(key) > 128 || strings.ContainsRune(key, 0) || !utf8.ValidString(key) {
		return invalid("invalid plugin value key")
	}
	return nil
}

func pluginValueRevision(actual, expected uint64) error {
	if actual != expected {
		return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin value changed",
			Details: &protocol.FaultDetails{Kind: "plugin_value_conflict"}}
	}
	return nil
}

func readPluginValue(ctx context.Context, db *sql.Conn, name, key string) (protocol.PluginStorageEntry, error) {
	entry := protocol.DefaultPluginStorageEntry()
	entry.Key = key
	var value []byte
	err := db.QueryRowContext(ctx, "SELECT revision,value FROM plugin_values WHERE name=?1 AND key=?2", name, key).Scan(&entry.Revision, &value)
	if errors.Is(err, sql.ErrNoRows) {
		return entry, nil
	}
	if err != nil {
		return entry, err
	}
	if entry.Revision == 0 || value != nil && !json.Valid(value) {
		return entry, invalid("invalid stored plugin value")
	}
	entry.Present = value != nil
	if value != nil {
		entry.Value = value
	}
	return entry, nil
}

func ReadPluginValue(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, key string) (protocol.PluginStorageEntry, error) {
	if err := pluginKey(key, false); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	info, err := ResolvePluginInfo(ctx, db, reference)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	entry, err := readPluginValue(ctx, db, reference.Name, key)
	if err == nil && entry.Present {
		entry.Value, err = plugins.ProjectStorageValue(plugins.StorageCollection(info, protocol.PluginStorageScopeNode, key), entry.Value)
	}
	return entry, err
}

func WritePluginValue(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, key string, value json.RawMessage, expectedRevision uint64, index *protocol.PluginStorageIndex) (protocol.PluginStorageEntry, error) {
	if err := pluginKey(key, false); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	info, err := ResolvePluginInfo(ctx, db, reference)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	current, err := readPluginValue(ctx, db, reference.Name, key)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if err := pluginValueRevision(current.Revision, expectedRevision); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	var previous json.RawMessage
	if current.Present {
		previous = current.Value
	}
	value, err = plugins.NormalizeStorageValue(plugins.StorageCollection(info, protocol.PluginStorageScopeNode, key), previous, value)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	var indexData []byte
	if index != nil {
		if err := plugins.ValidateStorageIndex(*index); err != nil {
			return protocol.PluginStorageEntry{}, err
		}
		indexData, err = json.Marshal(index)
		if err != nil {
			return protocol.PluginStorageEntry{}, err
		}
	}
	var count, total, old int64
	if err := db.QueryRowContext(ctx, "SELECT count(*),coalesce(sum(length(value)+coalesce(length(index_data),0)),0) FROM plugin_values WHERE name=?1 AND value IS NOT NULL", reference.Name).Scan(&count, &total); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if err := db.QueryRowContext(ctx, "SELECT coalesce(sum(coalesce(length(value),0)+coalesce(length(index_data),0)),0) FROM plugin_values WHERE name=?1 AND key=?2", reference.Name, key).Scan(&old); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if !current.Present {
		count++
	}
	if count > 4096 || total-old+int64(len(value))+int64(len(indexData)) > 128*1024*1024 {
		return protocol.PluginStorageEntry{}, fault(protocol.ErrorCodeBusy, "plugin storage is full")
	}
	revision, err := nextRevision(current.Revision)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES(?1,?2,?3,?4,?5)
ON CONFLICT(name,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,index_data=excluded.index_data`,
		reference.Name, key, revision, []byte(value), indexData); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if err := replacePluginSearch(ctx, db, reference.Name, key, index); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	return protocol.PluginStorageEntry{Key: key, Revision: revision, Value: value, Present: true}, nil
}

func RemovePluginValue(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, key string, expectedRevision uint64) (protocol.PluginStorageEntry, error) {
	if err := pluginKey(key, false); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if _, err := ResolvePluginInfo(ctx, db, reference); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	current, err := readPluginValue(ctx, db, reference.Name, key)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if err := pluginValueRevision(current.Revision, expectedRevision); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if current.Revision == 0 {
		return current, nil
	}
	revision, err := nextRevision(current.Revision)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if _, err := db.ExecContext(ctx, "UPDATE plugin_values SET revision=?1,value=NULL,index_data=NULL WHERE name=?2 AND key=?3", revision, reference.Name, key); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if err := replacePluginSearch(ctx, db, reference.Name, key, nil); err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	return protocol.PluginStorageEntry{Key: key, Revision: revision, Value: json.RawMessage("null")}, nil
}

func ListPluginKeys(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, prefix string, after *string, limit uint16) (protocol.PluginStoragePage, error) {
	page := protocol.DefaultPluginStoragePage()
	if err := pluginKey(prefix, true); err != nil {
		return page, err
	}
	if limit == 0 || after != nil && (pluginKey(*after, false) != nil || !strings.HasPrefix(*after, prefix)) {
		return page, invalid("invalid plugin key page")
	}
	if _, err := ResolvePluginInfo(ctx, db, reference); err != nil {
		return page, err
	}
	cursor := ""
	if after != nil {
		cursor = *after
	}
	limit = min(limit, 100)
	rows, err := db.QueryContext(ctx, "SELECT key FROM plugin_values WHERE name=?1 AND value IS NOT NULL AND key>?2 AND substr(key,1,length(?3))=?3 ORDER BY key LIMIT ?4",
		reference.Name, cursor, prefix, int(limit)+1)
	if err != nil {
		return page, err
	}
	defer rows.Close()
	for rows.Next() {
		var key string
		if err := rows.Scan(&key); err != nil {
			return page, err
		}
		if len(page.Keys) == int(limit) {
			last := page.Keys[len(page.Keys)-1]
			page.After = &last
			break
		}
		page.Keys = append(page.Keys, key)
	}
	return page, rows.Err()
}
