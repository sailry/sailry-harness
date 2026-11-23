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
	value.ProviderDirectory = true
	user, userErr := CurrentUser(ctx, db)
	if userErr != nil {
		return value, userErr
	}
	value.User = &user
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
	if value.Roles, err = ReadRoles(ctx, db); err != nil {
		return value, err
	}
	if value.Teams, err = ReadTeams(ctx, db); err != nil {
		return value, err
	}
	if value.Terminals, err = ReadTerminals(ctx, db); err != nil {
		return value, err
	}
	if value.Ssh, err = ListSsh(ctx, db); err != nil {
		return value, err
	}
	if value.Databases, err = ListDatabases(ctx, db); err != nil {
		return value, err
	}
	plugins, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return value, err
	}
	for _, info := range plugins {
		value.Plugins = append(value.Plugins, info.Summary)
	}
	if value.Providers, value.ProviderQuotas, err = ReadProviderSnapshot(ctx, db, node); err != nil {
		return value, err
	}
	if value.ModelCatalog, err = ReadProviderCatalogStatus(ctx, db); err != nil {
		return value, err
	}
	power, err := ReadPower(ctx, db)
	if err != nil {
		return value, err
	}
	value.PowerSettingsRevision = power.Revision
	if value.MediaSettings, err = ReadMediaSettings(ctx, db); err != nil {
		return value, err
	}
	if settings, err := readPersonalSetting[protocol.TerminalSettings](ctx, db, "terminal"); err != nil {
		return value, err
	} else if settings != nil {
		value.TerminalSettingsRevision = settings.Revision
	}
	if settings, err := readBody[protocol.SandboxSettings](ctx, db, "SELECT body FROM sandbox_settings WHERE singleton=1"); err != nil {
		return value, err
	} else if settings != nil {
		value.SandboxSettingsRevision = settings.Revision
	}
	if settings, err := readPersonalSetting[searchStored](ctx, db, "search"); err != nil {
		return value, err
	} else if settings != nil {
		value.SearchSettingsRevision = settings.Settings.Revision
	}
	if settings, err := ReadTeamSettings(ctx, db); err != nil {
		return value, err
	} else {
		value.TeamSettingsRevision = settings.Revision
	}
	if settings, err := readPersonalSetting[protocol.ConversationCompactionSettings](ctx, db, "compaction"); err != nil {
		return value, err
	} else if settings != nil {
		value.CompactionSettingsRevision = settings.Revision
	}
	if settings, err := ReadComputerSettings(ctx, db); err != nil {
		return value, err
	} else {
		value.ComputerSettingsRevision = settings.Revision
	}
	if settings, err := readPersonalSetting[protocol.ExternalBrowserSettings](ctx, db, "external_browser"); err != nil {
		return value, err
	} else if settings != nil {
		value.ExternalBrowserSettingsRevision = settings.Revision
	}
	if rev, err := ReadMemoryRevision(ctx, db); err != nil {
		return value, err
	} else {
		value.MemoryRevision = rev
	}
	if skills := nativeSkills(ctx); len(skills) > 0 {
		value.NativeSkills = append([]protocol.NativeSkill(nil), skills...)
	}
	value.Cursor, err = EventCursor(ctx, db)
	return value, err
}

func EventCursor(ctx context.Context, db *sql.Conn) (uint64, error) {
	var cursor uint64
	err := db.QueryRowContext(ctx, "SELECT coalesce(max(cursor),0) FROM events").Scan(&cursor)
	return cursor, err
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
		audience, err := captureEventAudience(ctx, db, event)
		if err != nil {
			return nil, err
		}
		scope, err := json.Marshal(audience)
		if err != nil {
			return nil, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO events(cursor,body,audience) VALUES(?1,?2,?3)", cursor, body, scope); err != nil {
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
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return through, nil, err
	}
	rows, err := db.QueryContext(ctx, "SELECT cursor,body,audience FROM events WHERE cursor>?1 AND cursor<=?2 ORDER BY cursor LIMIT 128", after, through)
	if err != nil {
		return through, nil, err
	}
	defer rows.Close()
	values := []protocol.EventEnvelope{}
	size := 0
	for rows.Next() {
		value := protocol.EventEnvelope{Node: node}
		var body, scope []byte
		if err := rows.Scan(&value.Cursor, &body, &scope); err != nil {
			return through, nil, err
		}
		if len(values) > 0 && size+len(body) > 2*1024*1024 {
			break
		}
		size += len(body)
		if err := json.Unmarshal(body, &value.Event); err != nil {
			return through, nil, err
		}
		var audience eventAudience
		if scope == nil {
			local, err := LocalUser(ctx, db)
			if err != nil {
				return through, nil, err
			}
			audience = eventAudience{Kind: "user", Owner: &local.ID}
		} else if err := json.Unmarshal(scope, &audience); err != nil {
			return through, nil, err
		}
		value.Event, err = filterEvent(ctx, db, value.Event, audience, user)
		if err != nil {
			return through, nil, err
		}
		values = append(values, value)
	}
	return through, values, rows.Err()
}
