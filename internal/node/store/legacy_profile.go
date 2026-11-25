package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
)

func relocateProfileSettings(ctx context.Context, db *sql.Conn, original, target string) error {
	if original == target {
		return nil
	}
	node, err := storageNode(ctx, db)
	if err != nil {
		return err
	}
	// HostPath stores NodeID's JSON array, not its hexadecimal display string.
	encodedNode, err := json.Marshal(node)
	if err != nil {
		return err
	}
	for _, field := range []struct{ table, column, path, condition string }{
		{"backup_settings", "body", "$.settings.local.directory", "1"},
		{"terminal_settings", "body", "$.shell", "1"},
		{"user_personal_settings", "body", "$.shell", "kind='terminal'"},
		{"database_profiles", "body", "$.connection.path", "json_extract(body,'$.connection.kind') IN ('sqlite','socket') AND COALESCE(json_extract(body,'$.location.node'),?1)=?1"},
		{"database_profiles", "body", "$.location.path", "json_extract(body,'$.connection.kind') IN ('sqlite','socket') AND json_extract(body,'$.location.node')=?1"},
		{"ssh_profiles", "credential", "$.path", "json_extract(credential,'$.kind')='key_path' AND COALESCE(json_extract(credential,'$.location.node'),json_extract(body,'$.authentication_node'),?1)=?1"},
		{"ssh_profiles", "credential", "$.location.path", "json_extract(credential,'$.kind')='key_path' AND json_extract(credential,'$.location.node')=?1"},
	} {
		query := "SELECT rowid,json_extract(" + field.column + ",?2) FROM " + field.table + " WHERE (" + field.condition + ") AND json_type(" + field.column + ",?2)='text'"
		rows, err := db.QueryContext(ctx, query, string(encodedNode), field.path)
		if err != nil {
			return err
		}
		paths := map[int64]string{}
		for rows.Next() {
			var id int64
			var path string
			if err = rows.Scan(&id, &path); err != nil {
				break
			}
			if moved, ok := relocated(path, original, target); ok {
				paths[id] = moved
			}
		}
		if err := errors.Join(err, rows.Err(), rows.Close()); err != nil {
			return err
		}
		for id, path := range paths {
			query := "UPDATE " + field.table + " SET " + field.column + "=json_set(" + field.column + ",?1,?2) WHERE rowid=?3"
			if _, err := db.ExecContext(ctx, query, field.path, path, id); err != nil {
				return err
			}
		}
	}
	return nil
}
