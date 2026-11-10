package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func SaveTerminal(ctx context.Context, db *sql.Conn, value protocol.TerminalInfo, event bool) ([]protocol.Event, error) {
	var body []byte
	err := db.QueryRowContext(ctx, "SELECT body FROM terminals WHERE id=?1", value.ID.String()).Scan(&body)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}
	if err == nil {
		var previous protocol.TerminalInfo
		if err := json.Unmarshal(body, &previous); err != nil {
			return nil, err
		}
		if previous.Revision > value.Revision || previous.Revision == value.Revision && (!event || previous.Status.Kind != "running" && value.Status.Kind == "running") {
			return nil, nil
		}
	}
	next, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	if bytes.Equal(body, next) {
		return nil, nil
	}
	var worktree any
	if value.Worktree != nil {
		worktree = value.Worktree.String()
	}
	_, err = db.ExecContext(ctx, "INSERT INTO terminals(id,worktree,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET worktree=excluded.worktree,body=excluded.body", value.ID.String(), worktree, next)
	if err != nil {
		return nil, err
	}
	return []protocol.Event{{Kind: "terminal_changed", Data: value}}, nil
}

func ReadTerminal(ctx context.Context, db *sql.Conn, id protocol.TerminalID) (protocol.TerminalInfo, error) {
	value, err := readBody[protocol.TerminalInfo](ctx, db, "SELECT body FROM terminals WHERE id=?1", id.String())
	if err != nil {
		return protocol.TerminalInfo{}, err
	}
	if value == nil {
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeNotFound, "terminal is unavailable")
	}
	return *value, nil
}

func RecoverTerminals(ctx context.Context, db *sql.Conn) error {
	values, err := readBodies[protocol.TerminalInfo](ctx, db, "SELECT body FROM terminals ORDER BY rowid")
	if err != nil {
		return err
	}
	for _, value := range values {
		if value.Status.Kind != "running" {
			continue
		}
		value.Status = protocol.TerminalStatus{Kind: "stopped"}
		value.Owner = nil
		value.Revision++
		if _, err := SaveTerminal(ctx, db, value, false); err != nil {
			return err
		}
	}
	return nil
}
