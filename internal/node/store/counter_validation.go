package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
)

var counterOwners = map[string]string{
	"events": "cursor", "plugin_conversation_records": "sequence",
	"agent_events": "sequence", "dispatch_events": "sequence",
	"dispatch_jobs": "sequence", "notifications": "sequence",
	"sessions": "rowid", "turns": "rowid", "file_checkpoints": "rowid",
	"artifacts": "rowid", "agent_background": "sequence",
}

func validateCounters(ctx context.Context, connection *sql.Conn) error {
	rows, err := connection.QueryContext(ctx, "SELECT namespace,value FROM counters")
	if err != nil {
		return err
	}
	watermarks := make(map[string]int64)
	for rows.Next() {
		var namespace string
		var value int64
		if err := rows.Scan(&namespace, &value); err != nil {
			rows.Close()
			return err
		}
		if _, known := counterOwners[namespace]; !known || value < 1 {
			rows.Close()
			return errors.New("invalid stored sequence watermark; existing data was not changed")
		}
		watermarks[namespace] = value
	}
	err = errors.Join(rows.Err(), rows.Close())
	if err != nil {
		return err
	}
	for table, column := range counterOwners {
		var maximum int64
		if err := connection.QueryRowContext(ctx, fmt.Sprintf("SELECT coalesce(max(%s),0) FROM %s", column, table)).Scan(&maximum); err != nil {
			return err
		}
		if maximum < 0 || watermarks[table] < maximum {
			return errors.New("invalid stored sequence watermark; existing data was not changed")
		}
	}
	return nil
}
