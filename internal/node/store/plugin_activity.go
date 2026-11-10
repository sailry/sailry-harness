package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadActivityCatalog(ctx context.Context, db *sql.Conn, node protocol.NodeID) (protocol.ActivityCatalog, error) {
	value := protocol.DefaultActivityCatalog()
	value.Node = node
	if err := db.QueryRowContext(ctx, "SELECT coalesce(max(cursor),0) FROM events").Scan(&value.Cursor); err != nil {
		return value, err
	}
	var err error
	if value.Projects, err = ReadProjects(ctx, db); err != nil {
		return value, err
	}
	if value.Worktrees, err = ReadWorktrees(ctx, db); err != nil {
		return value, err
	}
	if value.Sessions, err = ReadSessions(ctx, db); err != nil {
		return value, err
	}
	if value.Terminals, err = readBodies[protocol.TerminalInfo](ctx, db, "SELECT body FROM terminals ORDER BY id"); err != nil {
		return value, err
	}
	body, err := json.Marshal(value)
	if err != nil {
		return value, err
	}
	if len(body) > 8*1024*1024 {
		return protocol.ActivityCatalog{}, invalid("activity catalog exceeds the response limit")
	}
	return value, nil
}

func ReadActivityPreviews(ctx context.Context, db *sql.Conn, sessions []protocol.SessionID) ([]protocol.ActivityPreview, error) {
	if len(sessions) > 32 {
		return nil, invalid("activity preview batch exceeds 32 sessions")
	}
	values := make([]protocol.ActivityPreview, 0, len(sessions))
	for _, id := range sessions {
		session, err := ReadSession(ctx, db, id)
		if err != nil {
			return nil, err
		}
		value := protocol.ActivityPreview{Session: id}
		if run := session.Activity.Run; run != nil {
			value.Turn = &run.Turn
			var text string
			err := db.QueryRowContext(ctx, `SELECT substr(json_extract(part.value,'$.text'),1,512)
				FROM agent_events e,json_each(e.body,'$.content') part
				WHERE e.turn=?1 AND json_extract(e.body,'$.provenance.author')!='user'
				AND json_extract(part.value,'$.type')='text'
				AND length(trim(json_extract(part.value,'$.text')))>0
				ORDER BY e.sequence DESC,part.key ASC LIMIT 1`, run.Turn.String()).Scan(&text)
			if err != nil && !errors.Is(err, sql.ErrNoRows) {
				return nil, err
			}
			if err == nil {
				value.Text = &text
			}
		}
		values = append(values, value)
	}
	return values, nil
}
