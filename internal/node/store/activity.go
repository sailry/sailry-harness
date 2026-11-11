package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func readActivity(ctx context.Context, db *sql.Conn, session protocol.SessionID) (protocol.ActivitySummary, error) {
	value := protocol.DefaultActivitySummary()
	err := db.QueryRowContext(ctx, "SELECT revision,unread FROM session_attention WHERE session=?1", session.String()).Scan(&value.Attention.Revision, &value.Attention.Unread)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	// An active run is found through the status index; a turn appends while one exists.
	run, err := readBody[protocol.ConversationRun](ctx, db, `SELECT a.body FROM agent_runs a
		CROSS JOIN conversation_turns h ON h.session=?1 AND h.turn=a.turn JOIN turns t ON t.id=a.turn
		WHERE json_extract(a.body,'$.status') IN ('running','stopping') AND json_extract(a.body,'$.joined') IS NULL
		ORDER BY t.rowid DESC LIMIT 1`, session.String())
	if err != nil {
		return value, err
	}
	// Keep the finishing outcome visible until the next queued turn starts.
	if run == nil {
		run, err = readBody[protocol.ConversationRun](ctx, db, `SELECT a.body FROM conversation_turns h
		JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=t.id
		WHERE h.session=?1 AND json_extract(a.body,'$.joined') IS NULL
		ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,
		(json_extract(a.body,'$.status')='queued'),t.rowid DESC LIMIT 1`, session.String())
		if err != nil {
			return value, err
		}
	}
	title, err := activityTitle(ctx, db, session)
	if err != nil {
		return value, err
	}
	value.Title = shortTitle(title)
	if run == nil {
		return value, nil
	}
	if run.Session != session {
		origin := run.Session
		run.Origin = &origin
		run.Session = session
	}
	value.Run = run
	if run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping {
		var approval, question bool
		err := db.QueryRowContext(ctx, `SELECT
			EXISTS(SELECT 1 FROM agent_approvals WHERE turn=?1 AND json_extract(body,'$.state')='pending'),
			EXISTS(SELECT 1 FROM agent_questions WHERE turn=?1 AND json_extract(body,'$.state.kind')='pending')`, run.Turn.String()).Scan(&approval, &question)
		if err != nil {
			return value, err
		}
		if approval {
			waiting := protocol.ActivityWaitingApproval
			value.Waiting = &waiting
		} else if question {
			waiting := protocol.ActivityWaitingInput
			value.Waiting = &waiting
		}
	}
	err = db.QueryRowContext(ctx, "SELECT count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1", session.String()).Scan(&value.Queued)
	return value, err
}

func activityTitle(ctx context.Context, db *sql.Conn, session protocol.SessionID) (string, error) {
	assigned, err := readBody[string](ctx, db, "SELECT body FROM agent_state WHERE owner=?1 AND key=?2", session.String(), sessionTitleKey)
	if err != nil {
		return "", err
	}
	if assigned != nil {
		return *assigned, nil
	}
	var first string
	err = db.QueryRowContext(ctx, `SELECT t.id FROM conversation_turns h JOIN turns t ON t.id=h.turn
		JOIN agent_runs a ON a.turn=t.id WHERE h.session=?1 AND json_extract(a.body,'$.kind')='task'
		ORDER BY t.rowid LIMIT 1`, session.String()).Scan(&first)
	if errors.Is(err, sql.ErrNoRows) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	// A turn admitted inside a plugin transaction keeps only the digest of
	// its request; its text is the input captured on the run.
	var message sql.NullString
	err = db.QueryRowContext(ctx, `SELECT coalesce(CASE WHEN json_valid(r.body) THEN coalesce(
		json_extract(r.body,'$.command.data.message.text'),
		json_extract(r.body,'$.command.data.response.data.message.text')) END,
		json_extract(a.input,'$.text'))
		FROM turns t LEFT JOIN agent_pending p ON p.turn=t.id JOIN agent_runs a ON a.turn=t.id
		JOIN requests r ON r.id=coalesce(p.request,t.request) AND r.caller=coalesce(p.caller,t.caller)
		WHERE t.id=?1`, first).Scan(&message)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return "", err
	}
	if message.Valid {
		return message.String, nil
	}
	// Canonical input selects its original part index, not its position in content.
	err = db.QueryRowContext(ctx, `SELECT CASE
		WHEN json_extract(record.body,'$.kind')='message' AND json_extract(part.value,'$.type')='text'
		THEN json_extract(part.value,'$.text') END
		FROM (SELECT body FROM agent_events WHERE turn=?1 AND json_extract(body,'$.provenance.author')='user'
		ORDER BY sequence LIMIT 1) record
		LEFT JOIN json_each(record.body,'$.content') part
		ON json_extract(part.value,'$.index')=coalesce(json_extract(record.body,'$.metadata.input.text_part'),0)
		LIMIT 1`, first).Scan(&message)
	if errors.Is(err, sql.ErrNoRows) {
		return "", nil
	}
	return message.String, err
}
