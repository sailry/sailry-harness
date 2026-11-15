package store

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// MergePluginUsers preserves source revisions and values for admitted work.
// A target installation, connector or storage key (including a removal
// tombstone) wins a conflict; source originals remain in the merge archive.
func MergePluginUsers(ctx context.Context, db *sql.Conn, source, target protocol.User) error {
	statements := []string{
		`INSERT INTO plugin_user_settings(user,plugin,revision,body)
SELECT ?2,s.plugin,s.revision,s.body FROM plugin_user_settings s
WHERE s.user=?1 AND NOT EXISTS(SELECT 1 FROM plugin_user_installations t WHERE t.user=?2 AND t.name=s.plugin)
ON CONFLICT(user,plugin,revision) DO NOTHING`,
		`INSERT INTO plugin_user_installations(user,name,revision,settings_revision,installed,body)
SELECT ?2,name,revision,settings_revision,installed,body FROM plugin_user_installations WHERE user=?1
ON CONFLICT(user,name) DO NOTHING`,
		`UPDATE plugin_connections SET user=?2 WHERE user=?1 AND NOT EXISTS(
SELECT 1 FROM plugin_connections t WHERE t.user=?2 AND t.plugin=plugin_connections.plugin AND t.connector=plugin_connections.connector)`,
	}
	for _, statement := range statements {
		if _, err := db.ExecContext(ctx, statement, source.ID.String(), target.ID.String()); err != nil {
			return err
		}
	}
	sourceSpace, targetSpace := "user:"+source.ID.String(), "user:"+target.ID.String()
	// Read only records that will be promoted, before the canonical copy makes
	// them indistinguishable from target records for rebuilding derived indexes.
	rows, err := db.QueryContext(ctx, `SELECT name,key,index_data FROM plugin_space_values s WHERE space=?1
AND NOT EXISTS(SELECT 1 FROM plugin_space_values t WHERE t.space=?2 AND t.name=s.name AND t.key=s.key)`, sourceSpace, targetSpace)
	if err != nil {
		return err
	}
	type indexed struct {
		name, key string
		body      []byte
	}
	values := []indexed{}
	for rows.Next() {
		var value indexed
		if err := rows.Scan(&value.name, &value.key, &value.body); err != nil {
			rows.Close()
			return err
		}
		values = append(values, value)
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return err
	}
	if err := rows.Close(); err != nil {
		return err
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO plugin_space_values(space,name,key,revision,value,index_data)
SELECT ?2,name,key,revision,value,index_data FROM plugin_space_values WHERE space=?1
ON CONFLICT(space,name,key) DO NOTHING`, sourceSpace, targetSpace); err != nil {
		return err
	}
	ctx = WithCapturedUser(WithUser(ctx, target), target.ID)
	for _, value := range values {
		var index *protocol.PluginStorageIndex
		if len(value.body) != 0 {
			if err := json.Unmarshal(value.body, &index); err != nil {
				return err
			}
		}
		if err := replacePluginSearch(ctx, db, value.name, value.key, index); err != nil {
			return err
		}
	}
	return nil
}
