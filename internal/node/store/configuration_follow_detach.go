package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// RefreshConfigurationAuthorization removes stale pending-authorization notices
// after the target independently signs in, without changing source identity.
func RefreshConfigurationAuthorization(ctx context.Context, db *sql.Conn) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, "SELECT category,id,CASE WHEN local_id='' THEN id ELSE local_id END FROM configuration_follow_records WHERE user=?1 AND state='authorization'", user.ID.String())
	if err != nil {
		return err
	}
	type entry struct{ category, id, local string }
	var entries []entry
	for rows.Next() {
		var value entry
		if err := rows.Scan(&value.category, &value.id, &value.local); err != nil {
			rows.Close()
			return err
		}
		entries = append(entries, value)
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}
	for _, entry := range entries {
		ready := false
		switch entry.category {
		case "providers":
			id, err := protocol.ParseProviderID(entry.local)
			if err != nil {
				return err
			}
			value, err := readProviderConfiguration(ctx, db, id)
			if err != nil {
				return err
			}
			ready = value != nil && value.Provider != nil && value.Provider.Credential != nil
		case "plugin_settings":
			info, err := ReadPlugin(ctx, db, entry.id)
			if err != nil {
				return err
			}
			if info != nil {
				settings, err := readStoredPluginSettings(ctx, db, *info)
				if err != nil {
					return err
				}
				state := projectPluginSettings(*info, settings, settings.McpTargets)
				ready = state.Ready
			}
		case "connections":
			id, err := protocol.ParseRequestID(entry.local)
			if err != nil {
				return err
			}
			value, err := ReadConnectorRecord(ctx, db, id)
			if err != nil {
				var failure *protocol.Fault
				if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeNotFound {
					return err
				}
			} else {
				ready = value.State == protocol.PluginConnectorStateConnected
			}
		}
		if ready {
			if _, err := db.ExecContext(ctx, "UPDATE configuration_follow_records SET state='applied',message='' WHERE user=?1 AND category=?2 AND id=?3", user.ID.String(), entry.category, entry.id); err != nil {
				return err
			}
		}
	}
	return nil
}
