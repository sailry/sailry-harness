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
	_, err := db.ExecContext(ctx, "UPDATE agent_queues SET paused=?2,pause_reason=NULL WHERE session=?1", session.String(), paused)
	return err
}

// pauseQueue pauses a session's queue and records why, for its clients.
func pauseQueue(ctx context.Context, db *sql.Conn, session protocol.SessionID, reason protocol.ConversationQueuePause) error {
	if err := bumpQueue(ctx, db, session); err != nil {
		return err
	}
	_, err := db.ExecContext(ctx, "UPDATE agent_queues SET paused=1,pause_reason=?2 WHERE session=?1", session.String(), string(reason))
	return err
}
func readQueuedMessage(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationQueuedMessage, error) {
	value := protocol.DefaultConversationQueuedMessage()
	var captured, requestBody []byte
	err := db.QueryRowContext(ctx, "SELECT p.revision,a.input,r.body FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn JOIN requests r ON r.id=p.request AND r.caller=p.caller WHERE p.turn=?1", turn.String()).Scan(&value.Revision, &captured, &requestBody)
	if errors.Is(err, sql.ErrNoRows) {
		return value, fault(protocol.ErrorCodeConflict, "turn input is no longer queued")
	}
	if err != nil {
		return value, err
	}
	value.Message, err = decodeQueuedInput(captured, requestBody)
	if err != nil {
		return value, err
	}
	value.Turn, err = readTurnCapture(ctx, db, turn)
	return value, err
}

// A plugin transaction keeps only its digest in the request ledger, so its
// queued message is captured on the run. Ordinary queued input remains in its
// request until claimed.
func decodeQueuedInput(captured, requestBody []byte) (protocol.ConversationInput, error) {
	if len(captured) > 0 {
		var message protocol.ConversationInput
		err := json.Unmarshal(captured, &message)
		return message, err
	}
	var request protocol.Request
	if err := json.Unmarshal(requestBody, &request); err != nil {
		return protocol.ConversationInput{}, err
	}
	return agentInput(request.Command)
}

func ReadQueue(ctx context.Context, db *sql.Conn, session protocol.SessionID) (protocol.ConversationQueue, error) {
	value := protocol.DefaultConversationQueue()
	if err := requireSession(ctx, db, session); err != nil {
		return value, err
	}
	var reason sql.NullString
	err := db.QueryRowContext(ctx, "SELECT revision,paused,pause_reason FROM agent_queues WHERE session=?1", session.String()).Scan(&value.Revision, &value.Paused, &reason)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	// The reason is read only while paused, so every resume clears it.
	if value.Paused && reason.Valid {
		value.PauseReason = new(protocol.ConversationQueuePause(reason.String))
	}
	rows, err := db.QueryContext(ctx, "SELECT a.body,p.revision,a.ready,r.body,p.recipient,a.input,p.request FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN agent_runs a ON a.turn=p.turn JOIN requests r ON r.id=p.request AND r.caller=p.caller WHERE t.session=?1 AND (json_extract(a.body,'$.status')='queued' OR (json_extract(a.body,'$.status') IN ('failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=p.turn AND json_extract(e.body,'$.provenance.author')='user'))) ORDER BY p.position,t.rowid", session.String())
	if err != nil {
		return value, err
	}
	defer rows.Close()
	for rows.Next() {
		var body, requestBody, capturedInput []byte
		var revision uint64
		var ready bool
		var recipient sql.NullString
		var requestID string
		if err = rows.Scan(&body, &revision, &ready, &requestBody, &recipient, &capturedInput, &requestID); err != nil {
			return value, err
		}
		var run protocol.ConversationRun
		if err = json.Unmarshal(body, &run); err != nil {
			return value, err
		}
		id, e := protocol.ParseRequestID(requestID)
		if e != nil {
			return value, e
		}
		message, e := decodeQueuedInput(capturedInput, requestBody)
		if e != nil {
			return value, e
		}
		text := []rune(message.Text)
		pending := protocol.DefaultConversationPending()
		pending.Turn = run.Turn
		pending.Notice = run.Notice
		pending.Request = &id
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
		return run, new(queueItemNotPending)
	}
	if err != nil {
		return run, err
	}
	if revision != expected {
		return run, new(queueRevisionChanged)
	}
	return run, nil
}

// retainedTurn reports whether a turn that ended before its input reached the
// conversation still holds that input. A running or stopping turn keeps its
// pending row until the input is recorded, so the row alone is not enough.
func retainedTurn(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (bool, error) {
	var retained bool
	err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn WHERE p.turn=?1 AND json_extract(a.body,'$.status') IN ('failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=p.turn AND json_extract(e.body,'$.provenance.author')='user'))", turn.String()).Scan(&retained)
	return retained, err
}
func startQueued(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationRun, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return run, err
	}
	if run.Status != protocol.ConversationStatusQueued {
		retained, err := retainedTurn(ctx, db, turn)
		if err != nil {
			return run, err
		}
		if !retained {
			return run, new(queueInputConsumed)
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
	var retained, child bool
	if run.Status != protocol.ConversationStatusQueued {
		if retained, err = retainedTurn(ctx, db, turn); err != nil {
			return run, err
		}
	} else if child, err = unstartedChild(ctx, db, turn); err != nil {
		return run, err
	}
	if run.Status == protocol.ConversationStatusQueued || retained {
		if _, err = ReleaseBackground(ctx, db, turn, new(protocol.ConversationBackgroundHoldStopped), true); err != nil {
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
		// A stopped child that never started keeps its finish time unset: the
		// claim pass finishes it, which closes its completion record.
		if !child {
			ms := now.UnixMilli()
			run.FinishedMs = &ms
		}
	case protocol.ConversationStatusRunning:
		run.Status = protocol.ConversationStatusStopping
		if err = pauseQueue(ctx, db, run.Session, protocol.ConversationQueuePauseStopped); err != nil {
			return run, err
		}
	}
	return run, putAgentRun(ctx, db, run)
}

// interruptAgentTurn stops a turn the way a user stop does. Stopping a running
// turn also holds the session's command evidence; once the stop commits, the
// engine has the Node stop the session's non-service commands.
func interruptAgentTurn(ctx context.Context, db *sql.Conn, turn protocol.TurnID, now time.Time) (protocol.ConversationRun, error) {
	run, err := stopAgentTurn(ctx, db, turn, now)
	if err != nil || run.Status != protocol.ConversationStatusStopping {
		return run, err
	}
	return run, HoldStoppedCommands(ctx, db, run.Session)
}

func sendQueuedNow(ctx context.Context, db *sql.Conn, item protocol.ConversationRun, active []protocol.ConversationRun, now time.Time) ([]protocol.ConversationRun, error) {
	queue, err := ReadQueue(ctx, db, item.Session)
	if err != nil {
		return nil, err
	}
	if len(queue.Items) > 0 {
		if err := reorderQueue(ctx, db, item.Session, item.Turn, &queue.Items[0].Turn); err != nil {
			return nil, err
		}
	}
	changed := make([]protocol.ConversationRun, 0, len(active)+1)
	for _, run := range active {
		stopped, err := interruptAgentTurn(ctx, db, run.Turn, now)
		if err != nil {
			return nil, err
		}
		changed = append(changed, stopped)
	}
	run, err := startQueued(ctx, db, item.Turn)
	if err != nil {
		return nil, err
	}
	return append(changed, run), nil
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
		return new(queueItemNotPending)
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

// sameCapture reports whether two turns froze the same execution settings:
// model, mode and permissions, roles, plugins and hooks. Guidance may join
// only a turn it would have run identically to.
func sameCapture(ctx context.Context, db *sql.Conn, a, b protocol.TurnID) (bool, error) {
	var frozen [2][]byte
	for i, turn := range []protocol.TurnID{a, b} {
		capture, err := readTurnCapture(ctx, db, turn)
		if err != nil {
			return false, err
		}
		var hooks []byte
		if err = db.QueryRowContext(ctx, "SELECT hooks FROM agent_runs WHERE turn=?1", turn.String()).Scan(&hooks); err != nil {
			return false, err
		}
		if frozen[i], err = encode(struct {
			Config  any
			Roles   any
			Plugins any
			Hooks   string
		}{capture.Config, capture.Roles, capture.Plugins, string(hooks)}); err != nil {
			return false, err
		}
	}
	return bytes.Equal(frozen[0], frozen[1]), nil
}

// tagGuidance addresses a queued input to the running turn under the running
// turn's frozen settings, placed after the guidance already waiting for it and
// ahead of ordinary items.
func tagGuidance(ctx context.Context, db *sql.Conn, turn, recipient protocol.TurnID) error {
	_, err := addressGuidance(ctx, db, turn, recipient, nil)
	return err
}

// sendAt is set only for an explicit send from the queue. When the recipient is
// still running, the queued input adopts the recipient's frozen settings and
// joins it directly; when the recipient has already completed, it falls back to
// starting in an idle unpaused queue.
func addressGuidance(ctx context.Context, db *sql.Conn, turn, recipient protocol.TurnID, sendAt *time.Time) ([]protocol.ConversationRun, error) {
	source, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, err
	}
	target, err := ReadAgentRun(ctx, db, recipient)
	if err != nil {
		return nil, err
	}
	if turn == recipient {
		return nil, new(guidanceRecipientIsSource)
	}
	if source.Kind != protocol.ConversationRunKindTask || source.Notice || source.Continuation != nil || target.Session != source.Session || target.Worktree != source.Worktree {
		return nil, new(guidanceScopeMismatch)
	}
	if target.Kind != protocol.ConversationRunKindTask {
		return nil, new(guidanceRecipientNotTask)
	}
	if target.Status == protocol.ConversationStatusStopping {
		return nil, new(guidanceRecipientStopping)
	}
	if target.Status != protocol.ConversationStatusRunning {
		if sendAt != nil && target.Status == protocol.ConversationStatusCompleted && target.Joined == nil {
			return nil, sendGuidanceToIdleQueue(ctx, db, source)
		}
		return nil, new(guidanceRecipientNotRunning)
	}
	compatible, err := sameCapture(ctx, db, turn, recipient)
	if err != nil {
		return nil, err
	}
	if !compatible {
		// Guiding a running turn always joins it under the running turn's
		// frozen settings rather than splitting into a separate queued turn or
		// stopping the active reply.
		targetSettings, err := readTurnSettings(ctx, db, recipient)
		if err != nil {
			return nil, err
		}
		queued, err := readQueuedMessage(ctx, db, turn)
		if err != nil {
			return nil, err
		}
		session, err := ReadSession(ctx, db, source.Session)
		if err != nil {
			return nil, err
		}
		frozen := session
		frozen.Config, frozen.Roles = targetSettings.turn.Config, targetSettings.turn.Roles
		if err = validateAgentReferences(ctx, db, frozen, queued.Message, targetSettings.turn.Plugins); err != nil {
			return nil, err
		}
		if err = saveTurnSettings(ctx, db, turn, targetSettings); err != nil {
			return nil, err
		}
		if source, err = ReadAgentRun(ctx, db, turn); err != nil {
			return nil, err
		}
	}
	source.Guidance = &recipient
	if err = putAgentRun(ctx, db, source); err != nil {
		return nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET recipient=?2,revision=revision+1 WHERE turn=?1", turn.String(), recipient.String()); err != nil {
		return nil, err
	}
	if _, err = startQueued(ctx, db, turn); err != nil {
		return nil, err
	}
	queue, err := ReadQueue(ctx, db, source.Session)
	if err != nil {
		return nil, err
	}
	others := []protocol.TurnID{}
	last := -1
	for _, item := range queue.Items {
		if item.Turn == turn {
			continue
		}
		if item.Guidance != nil && *item.Guidance == recipient {
			last = len(others)
		}
		others = append(others, item.Turn)
	}
	var before *protocol.TurnID
	if last+1 < len(others) {
		before = &others[last+1]
	}
	return nil, reorderQueue(ctx, db, source.Session, turn, before)
}

func sendGuidanceToIdleQueue(ctx context.Context, db *sql.Conn, source protocol.ConversationRun) error {
	var active bool
	err := db.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn
		WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('running','stopping'))`, source.Session.String()).Scan(&active)
	if err != nil {
		return err
	}
	if active {
		return new(guidanceRecipientNotRunning)
	}
	queue, err := ReadQueue(ctx, db, source.Session)
	if err != nil {
		return err
	}
	if queue.Paused || len(queue.Items) == 0 {
		return new(guidanceRecipientNotRunning)
	}
	if err = reorderQueue(ctx, db, source.Session, source.Turn, &queue.Items[0].Turn); err != nil {
		return err
	}
	_, err = startQueued(ctx, db, source.Turn)
	return err
}

// addressSuccessors batches guidance that outlived its recipient. When the
// first such input starts its own turn, the inputs sent to the same recipient
// that directly follow it are addressed to that turn, so they join it
// together before its first model step instead of each being answered alone.
func addressSuccessors(ctx context.Context, db *sql.Conn, primary protocol.ConversationRun) error {
	if primary.Kind != protocol.ConversationRunKindTask || primary.Guidance == nil || primary.Notice || primary.Continuation != nil {
		return nil
	}
	queue, err := ReadQueue(ctx, db, primary.Session)
	if err != nil {
		return err
	}
	for _, item := range queue.Items {
		if item.Turn == primary.Turn {
			continue
		}
		run, err := ReadAgentRun(ctx, db, item.Turn)
		if err != nil {
			return err
		}
		if item.Guidance != nil || !item.Ready || item.Notice || run.Kind != protocol.ConversationRunKindTask || run.Status != protocol.ConversationStatusQueued || run.Continuation != nil || run.Guidance == nil || *run.Guidance != *primary.Guidance {
			return nil
		}
		compatible, err := sameCapture(ctx, db, item.Turn, primary.Turn)
		if err != nil || !compatible {
			return err
		}
		if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET recipient=?2,revision=revision+1 WHERE turn=?1", item.Turn.String(), primary.Turn.String()); err != nil {
			return err
		}
	}
	return nil
}

// releaseGuidance returns guidance its finished recipient never joined to the
// ordinary queue. It keeps its place and readiness, so it starts by the queue
// rules: at once after a turn that left the queue running, or on Send now or
// Resume after a stop.
// Released guidance becomes its own turn, so it runs with the session's current
// settings rather than those of the turn it never joined.
func releaseGuidance(ctx context.Context, db *sql.Conn, recipient protocol.ConversationRun) error {
	released, err := pendingTurns(ctx, db, "SELECT p.turn FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE p.recipient=?1 ORDER BY p.position,t.rowid", recipient.Turn.String())
	if err != nil || len(released) == 0 {
		return err
	}
	for _, turn := range released {
		if err = recaptureDetachedGuidance(ctx, db, turn); err != nil {
			return err
		}
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE recipient=?1", recipient.Turn.String()); err != nil {
		return err
	}
	return bumpQueue(ctx, db, recipient.Session)
}

// recaptureDetachedGuidance replaces the recipient's settings when a direct
// guidance input becomes its own turn. An ordinary queued message keeps the
// settings it captured on submission even if it was later offered as guidance.
// Settings that cannot be captured now, such as a removed model, keep the
// recipient's capture so the input is not lost; it settles by the claim rules
// for that configuration. Call before clearing the pending recipient.
func recaptureDetachedGuidance(ctx context.Context, db *sql.Conn, turn protocol.TurnID) error {
	var inherited bool
	err := db.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM agent_pending p
		JOIN turns t ON t.id=p.turn JOIN requests r ON r.id=t.request AND r.caller=t.caller
		WHERE p.turn=?1 AND p.recipient IS NOT NULL AND
		CASE WHEN json_valid(r.body) THEN json_extract(r.body,'$.command.kind')='guide_turn' ELSE 0 END)`, turn.String()).Scan(&inherited)
	if err != nil || !inherited {
		return err
	}
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil || run.Status != protocol.ConversationStatusQueued {
		return err
	}
	session, err := ReadSession(ctx, db, run.Session)
	if err != nil || session.Delegation != nil {
		return err
	}
	origin, err := OriginalTurnUser(ctx, db, turn)
	if err != nil {
		return err
	}
	user, err := ResolveUser(ctx, db, origin.ID)
	if err != nil {
		return err
	}
	node, err := storageNode(ctx, db)
	if err != nil {
		return err
	}
	session.Worktree = run.Worktree
	settings, err := captureTurnSettings(WithCapturedUser(WithUser(ctx, user), origin.ID), db, node, session, run.Kind)
	if uncapturableSettings(err) {
		return nil
	}
	if err != nil {
		return err
	}
	return saveTurnSettings(ctx, db, turn, settings)
}

// uncapturableSettings reports a configuration the session cannot run with
// now. A revision conflict means the configuration (such as a plugin
// assistant's version) changed during capture; failing here would fail the
// recipient's settlement, so the recipient's capture stands as for any other
// unusable configuration. Storage and availability faults propagate.
func uncapturableSettings(err error) bool {
	failure, ok := errors.AsType[*protocol.Fault](err)
	if !ok {
		return false
	}
	switch failure.Code {
	case protocol.ErrorCodeNotConfigured, protocol.ErrorCodeInvalidRequest, protocol.ErrorCodePermissionDenied, protocol.ErrorCodeExpired, protocol.ErrorCodeRevisionConflict:
		return true
	}
	return false
}

func pendingTurns(ctx context.Context, db *sql.Conn, query string, args ...any) ([]protocol.TurnID, error) {
	rows, err := db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	ids := []protocol.TurnID{}
	for rows.Next() {
		var text string
		if err = rows.Scan(&text); err != nil {
			rows.Close()
			return nil, err
		}
		id, e := protocol.ParseTurnID(text)
		if e != nil {
			rows.Close()
			return nil, e
		}
		ids = append(ids, id)
	}
	return ids, errors.Join(rows.Err(), rows.Close())
}
func TakeGuidance(ctx context.Context, db *sql.Conn, recipient protocol.TurnID, selected ...protocol.TurnID) ([]AgentGuidance, []protocol.Event, error) {
	target, err := ReadAgentRun(ctx, db, recipient)
	if err != nil {
		return nil, nil, err
	}
	if target.Status != protocol.ConversationStatusRunning {
		return nil, nil, new(guidanceRecipientNotRunning)
	}
	// The selected input takes priority within the same bounded batch, and
	// each group follows queue order. Unconsumed input keeps its position.
	priority, err := encode(selected)
	if err != nil {
		return nil, nil, err
	}
	ids, err := pendingTurns(ctx, db, `SELECT p.turn FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE p.recipient=?1
		ORDER BY EXISTS(SELECT 1 FROM json_each(?3) WHERE value=p.turn) DESC, p.position, t.rowid LIMIT ?2`, recipient.String(), backgroundBatch, string(priority))
	if err != nil {
		return nil, nil, err
	}
	out := []AgentGuidance{}
	size := 0
	for _, id := range ids {
		message, e := queuedInvocationInput(ctx, db, id)
		if e != nil {
			return nil, nil, e
		}
		body, e := encode(message.Message)
		if e != nil {
			return nil, nil, e
		}
		// A single message larger than the budget still joins alone.
		if size += len(body); size > backgroundBatchBytes && len(out) > 0 {
			break
		}
		run, e := ReadAgentRun(ctx, db, id)
		if e != nil {
			return nil, nil, e
		}
		run.Joined = &recipient
		if run.Guidance == nil {
			run.Guidance = &recipient
		}
		run.Status = protocol.ConversationStatusCompleted
		run.StartedMs = target.StartedMs
		run.FinishedMs = target.StartedMs
		if e = putAgentRun(ctx, db, run); e != nil {
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
	if len(out) == 0 {
		return out, nil, nil
	}
	if err = bumpQueue(ctx, db, target.Session); err != nil {
		return nil, nil, err
	}
	events, err := activityEvent(ctx, db, target.Session)
	return out, events, err
}
