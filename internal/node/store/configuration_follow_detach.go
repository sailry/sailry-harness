package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// DetachConfigurationCopies retains the last usable records as local editable
// configuration. Disabling first prevents an in-flight source from reclaiming
// them; a subsequent source sees ordinary ID/key conflicts instead of data loss.
func DetachConfigurationCopies(ctx context.Context, db *sql.Conn, category string) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	status, err := ReadConfigurationFollow(ctx, db)
	if err != nil {
		return err
	}
	if status.Source != nil {
		return fault(protocol.ErrorCodeConflict, "disable configuration following before detaching copies")
	}
	categories := ConfigurationCategories
	if category != "" {
		valid := false
		for _, item := range categories {
			valid = valid || item == category
		}
		if !valid {
			return invalid("unsupported configuration category")
		}
		categories = []string{category}
	}
	for _, category := range categories {
		switch category {
		case "providers":
			_, err = db.ExecContext(ctx, "UPDATE providers SET body=json_remove(body,'$.provider.source') WHERE user=?1 AND id IN (SELECT id FROM configuration_follow_records WHERE user=?1 AND category=?2 AND materialized=1)", user.ID.String(), category)
		case "roles", "teams", "ssh_profiles", "database_profiles":
			_, err = db.ExecContext(ctx, "UPDATE "+category+" SET body=json_remove(body,'$.source') WHERE user=?1 AND id IN (SELECT id FROM configuration_follow_records WHERE user=?1 AND category=?2 AND materialized=1)", user.ID.String(), category)
		case "plugins":
			_, err = db.ExecContext(ctx, "UPDATE plugin_user_installations SET body=json_remove(body,'$.summary.source') WHERE user=?1 AND name IN (SELECT id FROM configuration_follow_records WHERE user=?1 AND category=?2 AND materialized=1)", user.ID.String(), category)
		case "team_settings":
			var copied bool
			err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM configuration_follow_records WHERE user=?1 AND category=?2 AND materialized=1)", user.ID.String(), category).Scan(&copied)
			if err == nil && copied {
				if user.Local {
					_, err = db.ExecContext(ctx, "UPDATE team_settings SET body=json_remove(body,'$.source') WHERE singleton=1")
				} else {
					_, err = db.ExecContext(ctx, "UPDATE user_team_settings SET body=json_remove(body,'$.source') WHERE user=?1", user.ID.String())
				}
			}
		}
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "DELETE FROM configuration_follow_records WHERE user=?1 AND category=?2", user.ID.String(), category); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "DELETE FROM configuration_follow_versions WHERE user=?1 AND category=?2", user.ID.String(), category); err != nil {
			return err
		}
	}
	return nil
}

// RefreshConfigurationAuthorization removes stale pending-authorization notices
// after the target independently signs in, without changing source identity.
func RefreshConfigurationAuthorization(ctx context.Context, db *sql.Conn) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, "SELECT category,id FROM configuration_follow_records WHERE user=?1 AND state='authorization'", user.ID.String())
	if err != nil {
		return err
	}
	type entry struct{ category, id string }
	var entries []entry
	for rows.Next() {
		var value entry
		if err := rows.Scan(&value.category, &value.id); err != nil {
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
			id, err := protocol.ParseProviderID(entry.id)
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
			id, err := protocol.ParseRequestID(entry.id)
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
