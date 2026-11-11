package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func agentInput(command protocol.Command) (protocol.ConversationInput, error) {
	switch v := command.Data.(type) {
	case *protocol.CommandPluginTransactionPayload:
		for _, operation := range v.Operations {
			if operation.Kind != "submit" && operation.Kind != "continue" {
				continue
			}
			nested, err := pluginTransactionCommand(operation, protocol.PluginReference{})
			if err != nil {
				return protocol.ConversationInput{}, err
			}
			return agentInput(nested)
		}
		return protocol.ConversationInput{}, invalid("transaction has no pending input")
	case *protocol.ConversationStart:
		return v.Message, nil
	case *protocol.CommandSubmitTaskPayload:
		return v.Message, nil
	case *protocol.CommandSubmitTurnPayload:
		return v.Message, nil
	case *protocol.CommandQueueTurnPayload:
		return v.Message, nil
	case *protocol.CommandGuideTurnPayload:
		return v.Message, nil
	case *protocol.CommandContinueTurnPayload:
		return v.Message, nil
	case *protocol.CommandDeliverBackgroundPayload:
		return protocol.DefaultConversationInput(), nil
	case *protocol.CommandEditQueuedTurnPayload:
		return v.Message, nil
	case *protocol.CommandReplaceTurnPayload:
		return v.Message, nil
	case *protocol.CommandCompactContextPayload:
		m := protocol.DefaultConversationInput()
		if v.Focus != nil {
			m.Text = *v.Focus
		}
		if len(m.Text) > 64*1024 {
			return m, invalid("context compaction focus is too long")
		}
		return m, nil
	case *protocol.CommandResolveQuestionPayload:
		if v.Response.Kind == "start_coding" {
			return v.Response.Data.(*protocol.ConversationQuestionResponseStartCodingPayload).Message, nil
		}
	}
	return protocol.ConversationInput{}, invalid("pending input is not a message")
}
func validateAgentInput(message protocol.ConversationInput) error {
	seen := map[protocol.AttachmentID]bool{}
	for _, id := range message.Attachments {
		if seen[id] {
			return invalid("input must have distinct attachments")
		}
		seen[id] = true
	}
	if strings.TrimSpace(message.Text) == "" && len(message.Attachments) == 0 && len(message.References) == 0 || len(message.Text) > 1024*1024 || len(message.Attachments) > 8 || len(message.References) > 32 {
		return invalid("input must be nonempty with at most one MiB of text, eight distinct attachments and 32 references")
	}
	return nil
}
func bumpQueue(ctx context.Context, db *sql.Conn, session protocol.SessionID) error {
	_, err := db.ExecContext(ctx, "INSERT INTO agent_queues(session,revision,paused) VALUES(?1,1,0) ON CONFLICT(session) DO UPDATE SET revision=revision+1", session.String())
	return err
}
func setQueuePaused(ctx context.Context, db *sql.Conn, session protocol.SessionID, paused bool) error {
	if err := bumpQueue(ctx, db, session); err != nil {
		return err
	}
	_, err := db.ExecContext(ctx, "UPDATE agent_queues SET paused=?2 WHERE session=?1", session.String(), paused)
	return err
}
func readQueuedMessage(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationQueuedMessage, error) {
	value := protocol.DefaultConversationQueuedMessage()
	var body []byte
	err := db.QueryRowContext(ctx, "SELECT p.revision,r.body FROM agent_pending p JOIN requests r ON r.id=p.request AND r.caller=p.caller WHERE p.turn=?1", turn.String()).Scan(&value.Revision, &body)
	if errors.Is(err, sql.ErrNoRows) {
		return value, fault(protocol.ErrorCodeConflict, "turn input is no longer queued")
	}
	if err != nil {
		return value, err
	}
	var request protocol.Request
	if err = json.Unmarshal(body, &request); err != nil {
		return value, err
	}
	value.Message, err = agentInput(request.Command)
	if err != nil {
		return value, err
	}
	value.Turn, err = readTurnCapture(ctx, db, turn)
	return value, err
}
func ReadQueue(ctx context.Context, db *sql.Conn, session protocol.SessionID) (protocol.ConversationQueue, error) {
	value := protocol.DefaultConversationQueue()
	if err := requireSession(ctx, db, session); err != nil {
		return value, err
	}
	err := db.QueryRowContext(ctx, "SELECT revision,paused FROM agent_queues WHERE session=?1", session.String()).Scan(&value.Revision, &value.Paused)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	rows, err := db.QueryContext(ctx, "SELECT a.body,p.revision,a.ready,r.body,p.recipient,a.input FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN agent_runs a ON a.turn=p.turn JOIN requests r ON r.id=p.request AND r.caller=p.caller WHERE t.session=?1 AND (json_extract(a.body,'$.status')='queued' OR (json_extract(a.body,'$.status') IN ('failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=p.turn AND json_extract(e.body,'$.provenance.author')='user'))) ORDER BY p.position,t.rowid", session.String())
	if err != nil {
		return value, err
	}
	defer rows.Close()
	for rows.Next() {
		var body, requestBody, capturedInput []byte
		var revision uint64
		var ready bool
		var recipient sql.NullString
		if err = rows.Scan(&body, &revision, &ready, &requestBody, &recipient, &capturedInput); err != nil {
			return value, err
		}
		var run protocol.ConversationRun
		var request protocol.Request
		if err = json.Unmarshal(body, &run); err != nil {
			return value, err
		}
		if err = json.Unmarshal(requestBody, &request); err != nil {
			return value, err
		}
		message, e := agentInput(request.Command)
		if len(capturedInput) > 0 {
			e = json.Unmarshal(capturedInput, &message)
		}
		if e != nil {
			return value, e
		}
		text := []rune(message.Text)
		pending := protocol.DefaultConversationPending()
		pending.Turn = run.Turn
		pending.Notice = run.Notice
		pending.Request = &request.ID
		pending.Kind = run.Kind
		pending.Revision = revision
		pending.ConfigRevision = run.Revision
		pending.Ready = ready
		pending.Preview = string(text[:min(len(text), 256)])
		pending.Truncated = len(text) > 256
		if recipient.Valid {
			id, e := protocol.ParseTurnID(recipient.String)
			if e != nil {
				return value, e
			}
			pending.Guidance = &id
		}
		pending.Attachments, err = ListTurnAttachments(ctx, db, run.Turn)
		if err != nil {
			return value, err
		}
		value.Items = append(value.Items, pending)
	}
	return value, rows.Err()
}
func checkQueueItem(ctx context.Context, db *sql.Conn, turn protocol.TurnID, expected uint64) (protocol.ConversationRun, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return run, err
	}
	var revision uint64
	if err = db.QueryRowContext(ctx, "SELECT revision FROM agent_pending WHERE turn=?1", turn.String()).Scan(&revision); errors.Is(err, sql.ErrNoRows) {
		return run, fault(protocol.ErrorCodeConflict, "turn is no longer queued")
	}
	if err != nil {
		return run, err
	}
	if err = checkRevision(revision, expected); err != nil {
		return run, err
	}
	return run, nil
}
func startQueued(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationRun, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return run, err
	}
	if run.Status != protocol.ConversationStatusQueued {
		var retained bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending WHERE turn=?1)", turn.String()).Scan(&retained); err != nil {
			return run, err
		}
		if !retained {
			return run, fault(protocol.ErrorCodeConflict, "turn input has already been consumed")
		}
		run.Status = protocol.ConversationStatusQueued
		run.Error = nil
		run.StartedMs = nil
		run.FinishedMs = nil
		if err = putAgentRun(ctx, db, run); err != nil {
			return run, err
		}
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET ready=1 WHERE turn=?1", turn.String()); err != nil {
		return run, err
	}
	return run, setQueuePaused(ctx, db, run.Session, false)
}
func stopAgentTurn(ctx context.Context, db *sql.Conn, turn protocol.TurnID, now time.Time) (protocol.ConversationRun, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return run, err
	}
	var retained bool
	if run.Status != protocol.ConversationStatusQueued {
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn WHERE p.turn=?1 AND json_extract(a.body,'$.status') IN ('failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=p.turn AND json_extract(e.body,'$.provenance.author')='user'))", turn.String()).Scan(&retained); err != nil {
			return run, err
		}
	}
	if run.Status == protocol.ConversationStatusQueued || retained {
		if _, err = ReleaseBackground(ctx, db, turn, true); err != nil {
			return run, err
		}
		if _, err = db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", turn.String()); err != nil {
			return run, err
		}
		if err = bumpQueue(ctx, db, run.Session); err != nil {
			return run, err
		}
	}
	switch run.Status {
	case protocol.ConversationStatusQueued:
		run.Status = protocol.ConversationStatusCancelled
		ms := now.UnixMilli()
		run.FinishedMs = &ms
	case protocol.ConversationStatusRunning:
		run.Status = protocol.ConversationStatusStopping
		if err = setQueuePaused(ctx, db, run.Session, true); err != nil {
			return run, err
		}
	}
	return run, putAgentRun(ctx, db, run)
}
func reorderQueue(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, before *protocol.TurnID) error {
	queue, err := ReadQueue(ctx, db, session)
	if err != nil {
		return err
	}
	ids := []protocol.TurnID{}
	found := false
	for _, item := range queue.Items {
		if item.Turn == turn {
			found = true
			continue
		}
		ids = append(ids, item.Turn)
	}
	if !found {
		return fault(protocol.ErrorCodeConflict, "turn is no longer queued")
	}
	index := len(ids)
	if before != nil {
		index = slices.Index(ids, *before)
		if *before == turn {
			return nil
		}
		if index < 0 {
			return invalid("queue destination is not in this session")
		}
	}
	ids = append(ids, protocol.TurnID{})
	copy(ids[index+1:], ids[index:])
	ids[index] = turn
	for i, id := range ids {
		if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET position=?2 WHERE turn=?1", id.String(), i+1); err != nil {
			return err
		}
	}
	return bumpQueue(ctx, db, session)
}
func tagGuidance(ctx context.Context, db *sql.Conn, turn, recipient protocol.TurnID) error {
	source, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return err
	}
	target, err := ReadAgentRun(ctx, db, recipient)
	if err != nil {
		return err
	}
	if target.Status != protocol.ConversationStatusRunning || target.Session != source.Session || target.Worktree != source.Worktree {
		return fault(protocol.ErrorCodeConflict, "guidance belongs to another execution scope")
	}
	a, err := readTurnCapture(ctx, db, turn)
	if err != nil {
		return err
	}
	b, err := readTurnCapture(ctx, db, recipient)
	if err != nil {
		return err
	}
	aa, _ := encode(a.Config)
	bb, _ := encode(b.Config)
	source.Guidance = &recipient
	if err = putAgentRun(ctx, db, source); err != nil {
		return err
	}
	if !bytes.Equal(aa, bb) {
		_, err = startQueued(ctx, db, turn)
		return err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET recipient=?2,revision=revision+1 WHERE turn=?1", turn.String(), recipient.String()); err != nil {
		return err
	}
	_, err = startQueued(ctx, db, turn)
	return err
}
func TakeGuidance(ctx context.Context, db *sql.Conn, recipient protocol.TurnID) ([]AgentGuidance, []protocol.Event, error) {
	target, err := ReadAgentRun(ctx, db, recipient)
	if err != nil {
		return nil, nil, err
	}
	if target.Status != protocol.ConversationStatusRunning {
		return nil, nil, fault(protocol.ErrorCodeConflict, "guidance recipient is no longer running")
	}
	rows, err := db.QueryContext(ctx, "SELECT p.turn FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE p.recipient=?1 ORDER BY p.position,t.rowid", recipient.String())
	if err != nil {
		return nil, nil, err
	}
	ids := []protocol.TurnID{}
	for rows.Next() {
		var text string
		if err = rows.Scan(&text); err != nil {
			rows.Close()
			return nil, nil, err
		}
		id, e := protocol.ParseTurnID(text)
		if e != nil {
			rows.Close()
			return nil, nil, e
		}
		ids = append(ids, id)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, nil, err
	}
	out := []AgentGuidance{}
	for _, id := range ids {
		message, e := queuedInvocationInput(ctx, db, id)
		if e != nil {
			return nil, nil, e
		}
		run, e := ReadAgentRun(ctx, db, id)
		if e != nil {
			return nil, nil, e
		}
		run.Joined = &recipient
		run.Guidance = &recipient
		run.Status = protocol.ConversationStatusCompleted
		run.StartedMs = target.StartedMs
		run.FinishedMs = target.StartedMs
		if e = putAgentRun(ctx, db, run); e != nil {
			return nil, nil, e
		}
		body, e := encode(message.Message)
		if e != nil {
			return nil, nil, e
		}
		if _, e = db.ExecContext(ctx, "UPDATE agent_runs SET input=coalesce(input,?2),ready=0 WHERE turn=?1", id.String(), body); e != nil {
			return nil, nil, e
		}
		if _, e = db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", id.String()); e != nil {
			return nil, nil, e
		}
		out = append(out, AgentGuidance{Turn: message.Turn, Message: message.Message})
	}
	if len(ids) == 0 {
		return out, nil, nil
	}
	if err = bumpQueue(ctx, db, target.Session); err != nil {
		return nil, nil, err
	}
	events, err := activityEvent(ctx, db, target.Session)
	return out, events, err
}
