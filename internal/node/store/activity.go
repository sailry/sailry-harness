package store

import (
	"context"
	"database/sql"
	"encoding/json"
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
	// Count active follow-up work and results on their way to an automatic
	// turn. Held results stay available for the next user message.
	err = db.QueryRowContext(ctx, `SELECT count(*) FROM agent_background WHERE session=?1 AND (
		state='running' AND `+backgroundFollowUp+` AND json_extract(body,'$.task.suppressed')=0 OR `+backgroundReady+`
		AND NOT EXISTS(SELECT 1 FROM agent_queues WHERE session=?1 AND paused=1))`, session.String()).Scan(&value.Background)
	if err != nil {
		return value, err
	}
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

func readActivitiesBatch(ctx context.Context, db *sql.Conn, sessionTexts []string, sessionsJSON string) (map[string]protocol.ActivitySummary, error) {
	out := make(map[string]protocol.ActivitySummary, len(sessionTexts))
	parsedIDs := make(map[string]protocol.SessionID, len(sessionTexts))
	for _, s := range sessionTexts {
		out[s] = protocol.DefaultActivitySummary()
		id, err := protocol.ParseSessionID(s)
		if err != nil {
			return nil, err
		}
		parsedIDs[s] = id
	}
	attRows, err := db.QueryContext(ctx, "SELECT session,revision,unread FROM session_attention WHERE session IN (SELECT value FROM json_each(?1))", sessionsJSON)
	if err != nil {
		return nil, err
	}
	for attRows.Next() {
		var sessionText string
		var rev uint64
		var unread bool
		if err = attRows.Scan(&sessionText, &rev, &unread); err != nil {
			_ = attRows.Close()
			return nil, err
		}
		a := out[sessionText]
		a.Attention.Revision, a.Attention.Unread = rev, unread
		out[sessionText] = a
	}
	if err = errors.Join(attRows.Err(), attRows.Close()); err != nil {
		return nil, err
	}

	titles := make(map[string]string, len(sessionTexts))
	titleRows, err := db.QueryContext(ctx, "SELECT owner,body FROM agent_state WHERE owner IN (SELECT value FROM json_each(?1)) AND key=?2", sessionsJSON, sessionTitleKey)
	if err != nil {
		return nil, err
	}
	for titleRows.Next() {
		var owner string
		var body []byte
		if err = titleRows.Scan(&owner, &body); err != nil {
			_ = titleRows.Close()
			return nil, err
		}
		var title string
		if err = json.Unmarshal(body, &title); err != nil {
			_ = titleRows.Close()
			return nil, err
		}
		titles[owner] = title
	}
	if err = errors.Join(titleRows.Err(), titleRows.Close()); err != nil {
		return nil, err
	}
	var untitled []string
	for _, s := range sessionTexts {
		if _, ok := titles[s]; !ok {
			untitled = append(untitled, s)
		}
	}
	if len(untitled) > 0 {
		untitledJSON, err := json.Marshal(untitled)
		if err != nil {
			return nil, err
		}
		firstRows, err := db.QueryContext(ctx, `SELECT session,id FROM (
			SELECT h.session AS session,t.id AS id,ROW_NUMBER() OVER (PARTITION BY h.session ORDER BY t.rowid) AS rn
			FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=t.id
			WHERE h.session IN (SELECT value FROM json_each(?1)) AND json_extract(a.body,'$.kind')='task'
		) WHERE rn=1`, string(untitledJSON))
		if err != nil {
			return nil, err
		}
		firstTurnBySession := map[string]string{}
		firstTurns := make([]string, 0, len(untitled))
		for firstRows.Next() {
			var sessionText, turnID string
			if err = firstRows.Scan(&sessionText, &turnID); err != nil {
				_ = firstRows.Close()
				return nil, err
			}
			firstTurnBySession[sessionText] = turnID
			firstTurns = append(firstTurns, turnID)
		}
		if err = errors.Join(firstRows.Err(), firstRows.Close()); err != nil {
			return nil, err
		}
		if len(firstTurns) > 0 {
			turnsJSON, err := json.Marshal(firstTurns)
			if err != nil {
				return nil, err
			}
			msgRows, err := db.QueryContext(ctx, `SELECT t.id,coalesce(CASE WHEN json_valid(r.body) THEN coalesce(
				json_extract(r.body,'$.command.data.message.text'),
				json_extract(r.body,'$.command.data.response.data.message.text')) END,
				json_extract(a.input,'$.text'))
				FROM turns t LEFT JOIN agent_pending p ON p.turn=t.id JOIN agent_runs a ON a.turn=t.id
				JOIN requests r ON r.id=coalesce(p.request,t.request) AND r.caller=coalesce(p.caller,t.caller)
				WHERE t.id IN (SELECT value FROM json_each(?1))`, string(turnsJSON))
			if err != nil {
				return nil, err
			}
			turnMessages := make(map[string]string, len(firstTurns))
			for msgRows.Next() {
				var turnID string
				var message sql.NullString
				if err = msgRows.Scan(&turnID, &message); err != nil {
					_ = msgRows.Close()
					return nil, err
				}
				if message.Valid {
					turnMessages[turnID] = message.String
				}
			}
			if err = errors.Join(msgRows.Err(), msgRows.Close()); err != nil {
				return nil, err
			}
			for sessionText, turnID := range firstTurnBySession {
				if msg, ok := turnMessages[turnID]; ok {
					titles[sessionText] = msg
					continue
				}
				var fallback sql.NullString
				err = db.QueryRowContext(ctx, `SELECT CASE
					WHEN json_extract(record.body,'$.kind')='message' AND json_extract(part.value,'$.type')='text'
					THEN json_extract(part.value,'$.text') END
					FROM (SELECT body FROM agent_events WHERE turn=?1 AND json_extract(body,'$.provenance.author')='user'
					ORDER BY sequence LIMIT 1) record
					LEFT JOIN json_each(record.body,'$.content') part
					ON json_extract(part.value,'$.index')=coalesce(json_extract(record.body,'$.metadata.input.text_part'),0)
					LIMIT 1`, turnID).Scan(&fallback)
				if err != nil && !errors.Is(err, sql.ErrNoRows) {
					return nil, err
				}
				if fallback.Valid {
					titles[sessionText] = fallback.String
				}
			}
		}
	}
	for _, s := range sessionTexts {
		a := out[s]
		a.Title = shortTitle(titles[s])
		out[s] = a
	}

	bgRows, err := db.QueryContext(ctx, `SELECT agent_background.session, count(*)
		FROM agent_background
		WHERE agent_background.session IN (SELECT value FROM json_each(?1)) AND (
			agent_background.state='running' AND `+backgroundFollowUp+` AND json_extract(agent_background.body,'$.task.suppressed')=0
			OR `+backgroundReady+` AND NOT EXISTS(SELECT 1 FROM agent_queues WHERE agent_queues.session=agent_background.session AND agent_queues.paused=1)
		)
		GROUP BY agent_background.session`, sessionsJSON)
	if err != nil {
		return nil, err
	}
	for bgRows.Next() {
		var sessionText string
		var count uint32
		if err = bgRows.Scan(&sessionText, &count); err != nil {
			_ = bgRows.Close()
			return nil, err
		}
		a := out[sessionText]
		a.Background = count
		out[sessionText] = a
	}
	if err = errors.Join(bgRows.Err(), bgRows.Close()); err != nil {
		return nil, err
	}

	runRows, err := db.QueryContext(ctx, `SELECT session,body FROM (
		SELECT h.session AS session,a.body AS body,
			ROW_NUMBER() OVER (
				PARTITION BY h.session
				ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,
					(json_extract(a.body,'$.status')='queued'),t.rowid DESC
			) AS rn
		FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=t.id
		WHERE h.session IN (SELECT value FROM json_each(?1)) AND json_extract(a.body,'$.joined') IS NULL
	) WHERE rn=1`, sessionsJSON)
	if err != nil {
		return nil, err
	}
	var sessionsWithRun []string
	var activeTurnIDs []string
	activeTurnSet := map[string]struct{}{}
	activeTurnToSessions := map[string][]string{}
	for runRows.Next() {
		var sessionText string
		var body []byte
		if err = runRows.Scan(&sessionText, &body); err != nil {
			_ = runRows.Close()
			return nil, err
		}
		var run protocol.ConversationRun
		if err = json.Unmarshal(body, &run); err != nil {
			_ = runRows.Close()
			return nil, err
		}
		sessionID := parsedIDs[sessionText]
		if run.Session != sessionID {
			origin := run.Session
			run.Origin = &origin
			run.Session = sessionID
		}
		a := out[sessionText]
		a.Run = &run
		out[sessionText] = a
		sessionsWithRun = append(sessionsWithRun, sessionText)
		if run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping {
			turnKey := run.Turn.String()
			activeTurnToSessions[turnKey] = append(activeTurnToSessions[turnKey], sessionText)
			if _, ok := activeTurnSet[turnKey]; !ok {
				activeTurnSet[turnKey] = struct{}{}
				activeTurnIDs = append(activeTurnIDs, turnKey)
			}
		}
	}
	if err = errors.Join(runRows.Err(), runRows.Close()); err != nil {
		return nil, err
	}

	if len(activeTurnIDs) > 0 {
		turnsJSON, err := json.Marshal(activeTurnIDs)
		if err != nil {
			return nil, err
		}
		hasApproval := map[string]bool{}
		appRows, err := db.QueryContext(ctx, "SELECT DISTINCT turn FROM agent_approvals WHERE turn IN (SELECT value FROM json_each(?1)) AND json_extract(body,'$.state')='pending'", string(turnsJSON))
		if err != nil {
			return nil, err
		}
		for appRows.Next() {
			var turnKey string
			if err = appRows.Scan(&turnKey); err != nil {
				_ = appRows.Close()
				return nil, err
			}
			hasApproval[turnKey] = true
		}
		if err = errors.Join(appRows.Err(), appRows.Close()); err != nil {
			return nil, err
		}
		hasQuestion := map[string]bool{}
		qRows, err := db.QueryContext(ctx, "SELECT DISTINCT turn FROM agent_questions WHERE turn IN (SELECT value FROM json_each(?1)) AND json_extract(body,'$.state.kind')='pending'", string(turnsJSON))
		if err != nil {
			return nil, err
		}
		for qRows.Next() {
			var turnKey string
			if err = qRows.Scan(&turnKey); err != nil {
				_ = qRows.Close()
				return nil, err
			}
			hasQuestion[turnKey] = true
		}
		if err = errors.Join(qRows.Err(), qRows.Close()); err != nil {
			return nil, err
		}
		for turnKey, owners := range activeTurnToSessions {
			var waiting *protocol.ActivityWaiting
			if hasApproval[turnKey] {
				w := protocol.ActivityWaitingApproval
				waiting = &w
			} else if hasQuestion[turnKey] {
				w := protocol.ActivityWaitingInput
				waiting = &w
			}
			if waiting != nil {
				for _, sessionText := range owners {
					a := out[sessionText]
					w := *waiting
					a.Waiting = &w
					out[sessionText] = a
				}
			}
		}
	}

	if len(sessionsWithRun) > 0 {
		withRunJSON, err := json.Marshal(sessionsWithRun)
		if err != nil {
			return nil, err
		}
		pendRows, err := db.QueryContext(ctx, "SELECT t.session, count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session IN (SELECT value FROM json_each(?1)) GROUP BY t.session", string(withRunJSON))
		if err != nil {
			return nil, err
		}
		for pendRows.Next() {
			var sessionText string
			var queued uint32
			if err = pendRows.Scan(&sessionText, &queued); err != nil {
				_ = pendRows.Close()
				return nil, err
			}
			a := out[sessionText]
			a.Queued = queued
			out[sessionText] = a
		}
		if err = errors.Join(pendRows.Err(), pendRows.Close()); err != nil {
			return nil, err
		}
	}
	return out, nil
}
