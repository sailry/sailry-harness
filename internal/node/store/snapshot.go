package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Snapshot reads all fields from the caller's pinned read transaction.
func Snapshot(ctx context.Context, db *sql.Conn, node protocol.NodeID) (protocol.Snapshot, error) {
	value := protocol.DefaultSnapshot()
	value.Node = node
	var err error
	if value.Defaults, err = ReadDefaults(ctx, db); err != nil {
		return value, err
	}
	if value.Projects, err = ReadProjects(ctx, db); err != nil {
		return value, err
	}
	if value.Worktrees, err = ReadWorktrees(ctx, db); err != nil {
		return value, err
	}
	if value.Sessions, err = ReadSessions(ctx, db); err != nil {
		return value, err
	}
	if value.Notifications, err = ReadNotifications(ctx, db); err != nil {
		return value, err
	}
	if value.Roles, err = readBodies[protocol.RoleProfile](ctx, db, "SELECT body FROM roles ORDER BY key"); err != nil {
		return value, err
	}
	if value.Teams, err = readBodies[protocol.TeamProfile](ctx, db, "SELECT body FROM teams ORDER BY key"); err != nil {
		return value, err
	}
	if value.Terminals, err = readBodies[protocol.TerminalInfo](ctx, db, "SELECT body FROM terminals ORDER BY rowid"); err != nil {
		return value, err
	}
	if value.Ssh, err = readBodies[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles ORDER BY rowid"); err != nil {
		return value, err
	}
	if value.Databases, err = readBodies[protocol.DatabaseProfile](ctx, db, "SELECT body FROM database_profiles ORDER BY rowid"); err != nil {
		return value, err
	}
	plugins, err := readBodies[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE installed=1 ORDER BY name")
	if err != nil {
		return value, err
	}
	for _, info := range plugins {
		value.Plugins = append(value.Plugins, info.Summary)
	}
	providers, err := readBodies[providerConfiguration](ctx, db, "SELECT body FROM providers ORDER BY rowid")
	if err != nil {
		return value, err
	}
	for _, saved := range providers {
		if saved.Quota != nil {
			if err := validateProviderQuota(saved.ID, *saved.Quota); err != nil {
				return value, err
			}
		}
		if saved.Listed && saved.Provider != nil {
			value.Providers = append(value.Providers, *saved.Provider)
			if saved.Quota != nil && providerQuotaPresent(*saved.Quota) && providerQuotaMatches(node, saved, *saved.Quota) {
				value.ProviderQuotas = append(value.ProviderQuotas, *saved.Quota)
			}
		}
	}
	if value.ModelCatalog, err = ReadProviderCatalogStatus(ctx, db); err != nil {
		return value, err
	}
	power, err := ReadPower(ctx, db)
	if err != nil {
		return value, err
	}
	value.PowerSettingsRevision = power.Revision
	if media, err := readBody[protocol.MediaSettings](ctx, db, "SELECT body FROM media_settings WHERE singleton=1"); err != nil {
		return value, err
	} else if media != nil {
		value.MediaSettings = *media
	}
	if settings, err := readBody[protocol.TerminalSettings](ctx, db, "SELECT body FROM terminal_settings WHERE singleton=1"); err != nil {
		return value, err
	} else if settings != nil {
		value.TerminalSettingsRevision = settings.Revision
	}
	if settings, err := readBody[protocol.SandboxSettings](ctx, db, "SELECT body FROM sandbox_settings WHERE singleton=1"); err != nil {
		return value, err
	} else if settings != nil {
		value.SandboxSettingsRevision = settings.Revision
	}
	if settings, err := readBody[searchStored](ctx, db, "SELECT body FROM search_settings WHERE singleton=1"); err != nil {
		return value, err
	} else if settings != nil {
		value.SearchSettingsRevision = settings.Settings.Revision
	}
	if settings, err := readBody[protocol.TeamSettings](ctx, db, "SELECT body FROM team_settings WHERE singleton=1"); err != nil {
		return value, err
	} else if settings != nil {
		value.TeamSettingsRevision = settings.Revision
	}
	if settings, err := readBody[protocol.ConversationCompactionSettings](ctx, db, "SELECT body FROM compaction_settings WHERE singleton=1"); err != nil {
		return value, err
	} else if settings != nil {
		value.CompactionSettingsRevision = settings.Revision
	}
	err = db.QueryRowContext(ctx, "SELECT coalesce(max(cursor),0) FROM events").Scan(&value.Cursor)
	return value, err
}

func RecordEvents(ctx context.Context, db *sql.Conn, node protocol.NodeID, changes []protocol.Event) ([]protocol.EventEnvelope, error) {
	values := make([]protocol.EventEnvelope, 0, len(changes))
	for _, event := range changes {
		cursor, err := NextCounter(ctx, db, CounterEvents)
		if err != nil {
			return nil, err
		}
		body, err := encode(event)
		if err != nil {
			return nil, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO events(cursor,body) VALUES(?1,?2)", cursor, body); err != nil {
			return nil, err
		}
		values = append(values, protocol.EventEnvelope{Node: node, Cursor: cursor, Event: event})
	}
	return values, nil
}

// EventPage bounds each read while preserving the committed cursor watermark.
func EventPage(ctx context.Context, db *sql.Conn, node protocol.NodeID, after, through uint64) (uint64, []protocol.EventEnvelope, error) {
	if through == 0 {
		if err := db.QueryRowContext(ctx, "SELECT coalesce(max(cursor),0) FROM events").Scan(&through); err != nil {
			return 0, nil, err
		}
	}
	rows, err := db.QueryContext(ctx, "SELECT cursor,body FROM events WHERE cursor>?1 AND cursor<=?2 ORDER BY cursor LIMIT 128", after, through)
	if err != nil {
		return through, nil, err
	}
	defer rows.Close()
	values := []protocol.EventEnvelope{}
	size := 0
	for rows.Next() {
		value := protocol.EventEnvelope{Node: node}
		var body []byte
		if err := rows.Scan(&value.Cursor, &body); err != nil {
			return through, nil, err
		}
		if len(values) > 0 && size+len(body) > 2*1024*1024 {
			break
		}
		size += len(body)
		if err := json.Unmarshal(body, &value.Event); err != nil {
			return through, nil, err
		}
		values = append(values, value)
	}
	return through, values, rows.Err()
}
