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
	stored := err == nil
	if !stored {
		// A command can finish storing its outcome after the terminal exits.
		// The final event prevents that older result from recreating its row.
		err = db.QueryRowContext(ctx, "SELECT json_extract(body,'$.data') FROM events WHERE json_extract(body,'$.kind')='terminal_changed' AND json_extract(body,'$.data.id')=?1 ORDER BY cursor DESC LIMIT 1", value.ID.String()).Scan(&body)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return nil, err
		}
	}
	if err == nil {
		var previous protocol.TerminalInfo
		if err := json.Unmarshal(body, &previous); err != nil {
			return nil, err
		}
		if previous.Revision > value.Revision || previous.Revision == value.Revision && !event && !(stored && terminalFinished(value.Status.Kind)) || terminalFinished(previous.Status.Kind) && value.Status.Kind == "running" {
			return nil, nil
		}
	}
	next, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	if bytes.Equal(body, next) && !(stored && terminalFinished(value.Status.Kind)) {
		return nil, nil
	}
	if terminalFinished(value.Status.Kind) {
		if err := retainResourceAudience(ctx, db, "terminal", value.ID.String()); err != nil {
			return nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM terminals WHERE id=?1", value.ID.String())
	} else {
		var worktree any
		if value.Worktree != nil {
			worktree = value.Worktree.String()
		}
		user, userErr := CurrentUser(ctx, db)
		if userErr != nil {
			return nil, userErr
		}
		_, err = db.ExecContext(ctx, "INSERT INTO terminals(id,worktree,body,owner) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET worktree=excluded.worktree,body=excluded.body", value.ID.String(), worktree, next, user.ID.String())
	}
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
	changes := make([]protocol.Event, 0, len(values))
	for _, value := range values {
		if value.Status.Kind == "running" {
			value.Status = protocol.TerminalStatus{Kind: "stopped"}
			value.Revision, err = nextRevision(value.Revision)
			if err != nil {
				return err
			}
		}
		value.Owner = nil
		changes = append(changes, protocol.Event{Kind: "terminal_changed", Data: value})
	}
	if len(changes) == 0 {
		return nil
	}
	// PTYs belong to one Node process. Keep request outcomes and the event
	// cursor intact while retiring the preceding process's inventory.
	node, err := nodeIdentity(ctx, db)
	if err != nil {
		return err
	}
	if _, err := RecordEvents(ctx, db, node, changes); err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "DELETE FROM terminals")
	return err
}

func terminalFinished(status string) bool {
	switch status {
	case "closed", "stopped", "exited", "failed":
		return true
	}
	return false
}
