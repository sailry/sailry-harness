package store

import (
	"context"
	"database/sql"
	"errors"
	"log/slog"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const backgroundFollowUp = "(kind='agent' OR json_extract(body,'$.follow_up')=1)"

// Older child records did not capture follow-up. The exact recorded core call
// can prove they were background; ambiguous records stay available to wait_agent.
const backgroundChild = `(json_extract(body,'$.follow_up')=1 OR EXISTS(
	SELECT 1 FROM agent_delegations d JOIN agent_events e ON e.session=agent_background.session AND e.id=d.entry AND e.turn=d.parent_turn
	JOIN json_each(e.body,'$.content') p
	WHERE d.turn=substr(agent_background.id,7) AND d.parent_turn=agent_background.turn
	AND json_extract(p.value,'$.index')=d.part AND json_extract(p.value,'$.type')='tool_request'
	AND json_extract(p.value,'$.name')='spawn_agent' AND json_type(p.value,'$.arguments.background')='true'))`

const backgroundReady = "state='pending' AND (kind='command' AND json_extract(body,'$.follow_up')=1 OR kind='agent' AND " + backgroundChild + ") AND json_extract(body,'$.task.suppressed')=0 AND json_extract(body,'$.task.recovered')=0 AND json_extract(body,'$.task.outcome') IN ('completed','failed','timed_out','unknown','stalled')"
const backgroundDeliverable = backgroundReady + " AND delivery IS NULL"
const backgroundEligible = "session=?1 AND " + backgroundDeliverable
const backgroundInputEligible = "session=?1 AND state='pending' AND delivery IS NULL AND (kind='command' OR kind='agent' AND " + backgroundChild + " AND (json_extract(body,'$.task.outcome')!='failed' OR json_extract(body,'$.task.recovered')=0))"
const backgroundBatch = 16
const backgroundBatchBytes = 32 * 1024

// automaticLimit bounds how many automatic turns, notice turns and package
// continuations alike, a session starts in a row without user input. A notice
// turn initially reserves up to 16 results, so the bound covers independent
// results that finish apart as well as a chain where each automatic turn starts
// work whose result starts the next; eight leaves room for a fan-out to report while
// keeping an unattended session finite. A continuing package may declare its
// own limit, up to automaticLimitMax, for the chain it owns.
const automaticLimit = 8
const automaticLimitMax = 100

// refusedDeliveries remembers the reason last logged for each session so a
// delivery that stays refused is reported once, not on every claim pass.
var refusedDeliveries sync.Map

func reportDelivery(session protocol.SessionID, reason string) {
	if reason == "" {
		refusedDeliveries.Delete(session)
		return
	}
	if last, ok := refusedDeliveries.Swap(session, reason); ok && last == reason {
		return
	}
	logging.Emit(slog.LevelWarn, slog.String("event", "node.background"), slog.String("operation", "deliver"), slog.String("status", "refused"), slog.String("session", session.String()), slog.String("reason", reason))
}

// AutomaticChain is a session's run of automatic turns since its last user
// input and the limit that run is held to.
type AutomaticChain struct {
	Count int
	Limit int
}

// ReadAutomaticChain counts the notice turns and package continuations that
// started since the session's last user input. Automatic turns that never
// started neither count nor reset; joined guidance is user input, and so is
// the request behind a continuation the user asked for, which is the first
// turn of its chain. The limit is the one the chain's latest continuation
// declared, or the default.
func ReadAutomaticChain(ctx context.Context, db *sql.Conn, session protocol.SessionID) (AutomaticChain, error) {
	chain := AutomaticChain{Limit: automaticLimit}
	runs, err := readBodies[protocol.ConversationRun](ctx, db, "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 ORDER BY t.rowid DESC LIMIT 256", session.String())
	if err != nil {
		return chain, err
	}
	declared := false
	for _, run := range runs {
		if run.Kind != protocol.ConversationRunKindTask || run.StartedMs == nil {
			continue
		}
		if !run.Notice && run.Continuation == nil {
			break
		}
		chain.Count++
		if !declared && run.Continuation != nil {
			declared = true
			if run.Continuation.Limit != nil {
				chain.Limit = int(*run.Continuation.Limit)
			}
		}
		if run.Continuation != nil && run.Continuation.User {
			break
		}
	}
	return chain, nil
}

// holdBackground leaves a session's deliverable results for the user's next
// message once its automatic chain reached the limit.
func holdBackground(ctx context.Context, db *sql.Conn, session protocol.SessionID, limit int) ([]protocol.Event, error) {
	records, err := readBodies[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE "+backgroundEligible, session.String())
	if err != nil {
		return nil, err
	}
	for _, record := range records {
		record.Task.Suppressed, record.Task.Hold = true, new(protocol.ConversationBackgroundHoldLimit)
		if err = saveBackgroundRecord(ctx, db, &record); err != nil {
			return nil, err
		}
	}
	logging.Emit(slog.LevelWarn, slog.String("event", "node.background"), slog.String("operation", "deliver"), slog.String("status", "held"), slog.String("session", session.String()), slog.String("reason", "automatic turn limit reached; results wait for user input"), slog.Int("limit", limit), slog.Int("results", len(records)))
	return activityEvent(ctx, db, session)
}

func nodeIdentity(ctx context.Context, db *sql.Conn) (protocol.NodeID, error) {
	var node protocol.NodeID
	var identity []byte
	if err := db.QueryRowContext(ctx, "SELECT identity FROM node WHERE singleton=1").Scan(&identity); err != nil {
		return node, err
	}
	if len(identity) != 32 {
		return node, invalid("invalid Node identity")
	}
	copy(node[:], identity)
	return node, nil
}

// scheduleDeliveries admits a notice turn for every session that holds
// deliverable results and is free to run one. The claim pass calls it when
// nothing is ready, so a result waits only for its blocker, whichever
// transaction removed it.
func scheduleDeliveries(ctx context.Context, db *sql.Conn, now time.Time) (bool, []protocol.Event, error) {
	rows, err := db.QueryContext(ctx, "SELECT DISTINCT session FROM agent_background WHERE "+backgroundDeliverable)
	if err != nil {
		return false, nil, err
	}
	sessions := []protocol.SessionID{}
	for rows.Next() {
		var text string
		if err = rows.Scan(&text); err != nil {
			rows.Close()
			return false, nil, err
		}
		id, e := protocol.ParseSessionID(text)
		if e != nil {
			rows.Close()
			return false, nil, e
		}
		sessions = append(sessions, id)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil || len(sessions) == 0 {
		return false, nil, err
	}
	node, err := nodeIdentity(ctx, db)
	if err != nil {
		return false, nil, err
	}
	admitted := false
	events := []protocol.Event{}
	for _, session := range sessions {
		turn, changed, e := ScheduleBackground(ctx, db, node, session, now)
		if e != nil {
			return admitted, events, e
		}
		events = append(events, changed...)
		admitted = admitted || turn != nil
	}
	return admitted, events, nil
}

func backgroundResult(ctx context.Context, db *sql.Conn, record BackgroundRecord) (canonical.BackgroundResult, error) {
	var result canonical.BackgroundResult
	if record.Task.Outcome == nil {
		return result, invalid("pending completion has no outcome")
	}
	result.Notice = protocol.ConversationBackgroundNotice{ID: record.Task.ID, Kind: record.Task.Kind, Title: processPreview(record.Task.Title, 200), Outcome: *record.Task.Outcome}
	result.ElapsedMs, result.SilenceMs = record.ElapsedMs, record.SilenceMs
	result.ExitCode = record.Task.ExitCode
	if record.Task.Summary != nil {
		result.Notice.Summary = new(processPreview(*record.Task.Summary, 512))
	}
	// Cap both current and legacy output so the oldest result can fit on its
	// own even if every retained byte needs JSON escaping.
	responseLimit, streamLimit := 4096, 2048
	if record.Response != nil && (record.Stdout != nil || record.Stderr != nil) {
		responseLimit, streamLimit = 2048, 1024
	}
	if record.Response != nil {
		result.Response = new(processPreview(*record.Response, responseLimit))
	}
	if record.Stdout != nil {
		result.Stdout = new(processTail(*record.Stdout, streamLimit))
		result.StdoutOmittedBytes = omittedCommandBytes(record.StdoutBytes, *result.Stdout)
		result.StdoutTruncated = record.StdoutTruncated || result.StdoutOmittedBytes != nil || len(*result.Stdout) < len(*record.Stdout)
	}
	if record.Stderr != nil {
		result.Stderr = new(processTail(*record.Stderr, streamLimit))
		result.StderrOmittedBytes = omittedCommandBytes(record.StderrBytes, *result.Stderr)
		result.StderrTruncated = record.StderrTruncated || result.StderrOmittedBytes != nil || len(*result.Stderr) < len(*record.Stderr)
	}
	if record.Task.Kind.Kind == "agent" && *record.Task.Outcome == protocol.ConversationBackgroundOutcomeFailed {
		child, ok := processStatusData[protocol.ConversationBackgroundKindAgentPayload](record.Task.Kind.Data)
		if !ok {
			return result, invalid("invalid background agent identity")
		}
		failed, err := ReadAgentRun(ctx, db, child.Turn)
		if err != nil {
			return result, err
		}
		if failed.Session != child.Session || failed.Status != protocol.ConversationStatusFailed {
			return result, invalid("background failure does not match its child run")
		}
		if failed.Error != nil {
			result.Notice.Summary = new(processPreview(failed.Error.Message, 512))
			result.ErrorCode = &failed.Error.Code
		}
	}
	return result, nil
}

func omittedCommandBytes(total *uint64, tail string) *uint64 {
	if total == nil || *total <= uint64(len(tail)) {
		return nil
	}
	return new(*total - uint64(len(tail)))
}

func readBackgroundBatch(ctx context.Context, db *sql.Conn, session protocol.SessionID, reservation *protocol.TurnID, explicitInput bool) ([]BackgroundRecord, []canonical.BackgroundResult, error) {
	where := backgroundEligible
	args := []any{session.String(), backgroundBatch}
	if reservation != nil {
		where = "session=?1 AND " + backgroundReady + " AND delivery=?3"
		args = append(args, reservation.String())
	} else if explicitInput {
		where = backgroundInputEligible
	}
	records, err := readBodies[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE "+where+" ORDER BY sequence,id LIMIT ?2", args...)
	if err != nil {
		return nil, nil, err
	}
	results := make([]canonical.BackgroundResult, 0, len(records))
	size := len(`{"results":[]}`)
	for _, record := range records {
		result, err := backgroundResult(ctx, db, record)
		if err != nil {
			return nil, nil, err
		}
		body, err := encode(result)
		if err != nil {
			return nil, nil, err
		}
		size += len(body) + 1
		if size > backgroundBatchBytes {
			if len(results) == 0 {
				return nil, nil, invalid("completion evidence exceeds its delivery budget")
			}
			break
		}
		results = append(results, result)
	}
	return records[:len(results)], results, nil
}

func backgroundNotices(results []canonical.BackgroundResult) []protocol.ConversationBackgroundNotice {
	notices := make([]protocol.ConversationBackgroundNotice, len(results))
	for i, result := range results {
		notices[i] = result.Notice
	}
	return notices
}

// DeliverBackground appends one hidden callback and settles its evidence in
// the caller's write. An initial notice takes only the queued turn's reservation.
// It may produce queue events without an entry if the evidence was collected.
func DeliverBackground(ctx context.Context, db *sql.Conn, turn protocol.TurnID, initialNotice bool, now time.Time) (*protocol.ConversationEntry, []protocol.Event, error) {
	return deliverBackground(ctx, db, turn, initialNotice, false, now)
}

// DeliverInputBackground preserves waiting results, including held and
// non-follow-up commands, at an initial user or package input boundary only.
func DeliverInputBackground(ctx context.Context, db *sql.Conn, turn protocol.TurnID, now time.Time) (*protocol.ConversationEntry, []protocol.Event, error) {
	return deliverBackground(ctx, db, turn, false, true, now)
}

func deliverBackground(ctx context.Context, db *sql.Conn, turn protocol.TurnID, initialNotice, explicitInput bool, now time.Time) (*protocol.ConversationEntry, []protocol.Event, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, nil, err
	}
	if run.Status == protocol.ConversationStatusStopping {
		return nil, nil, context.Canceled
	}
	if run.Status != protocol.ConversationStatusRunning || run.Kind != protocol.ConversationRunKindTask || initialNotice && !run.Notice || explicitInput && run.Notice {
		return nil, nil, fault(protocol.ErrorCodeConflict, "turn cannot take background results at this boundary")
	}
	var reservation *protocol.TurnID
	if initialNotice {
		var queued bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending WHERE turn=?1)", turn.String()).Scan(&queued); err != nil || !queued {
			return nil, nil, err
		}
		reservation = &turn
	}
	records, results, err := readBackgroundBatch(ctx, db, run.Session, reservation, explicitInput)
	if err != nil {
		return nil, nil, err
	}
	if len(results) == 0 && !initialNotice {
		return nil, nil, nil
	}
	for _, record := range records {
		record.Task.State, record.Task.Delivery, record.Task.Hold = protocol.ConversationBackgroundStateDelivered, &turn, nil
		if err = saveBackgroundRecord(ctx, db, &record); err != nil {
			return nil, nil, err
		}
	}
	if initialNotice || explicitInput {
		run.Notices = backgroundNotices(results)
		if err = putAgentRun(ctx, db, run); err != nil {
			return nil, nil, err
		}
	}
	var released []protocol.Event
	if initialNotice {
		if _, err = db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", turn.String()); err != nil {
			return nil, nil, err
		}
		if err = bumpQueue(ctx, db, run.Session); err != nil {
			return nil, nil, err
		}
		// A reservation from older code may include now-ineligible evidence.
		released, err = ReleaseBackground(ctx, db, turn, nil, false)
		if err != nil {
			return nil, nil, err
		}
	}
	if len(results) == 0 {
		events, err := activityEvent(ctx, db, run.Session)
		return nil, append(released, events...), err
	}
	record := canonical.NewRecord(turn, "background", &canonical.Background{Initial: initialNotice, Results: results}, now)
	record.Provenance.Author = "system"
	record.Metadata.Runtime = &canonical.Runtime{AgentVisible: true}
	entry, events, err := AppendCanonical(ctx, db, run.Session, record)
	if err != nil {
		return nil, nil, err
	}
	return &entry, append(released, events...), nil
}

// automaticRefusal says why a session admits no automatic turn now, or nil
// when it is free: not archived, not a child, worktree present, queue running
// and empty, no running turn and no pending approval or question.
func automaticRefusal(ctx context.Context, db *sql.Conn, id protocol.SessionID) (*protocol.ConversationContinuationRefusal, error) {
	session, err := ReadSession(ctx, db, id)
	if err != nil {
		return nil, err
	}
	if session.Delegation != nil {
		return new(protocol.ConversationContinuationRefusalChild), nil
	}
	if session.Archived {
		return new(protocol.ConversationContinuationRefusalArchived), nil
	}
	var present bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM worktrees w WHERE w.id=?1 AND NOT EXISTS(SELECT 1 FROM removed_worktrees r WHERE r.worktree=w.id))", session.Worktree.String()).Scan(&present); err != nil {
		return nil, err
	}
	if !present {
		return new(protocol.ConversationContinuationRefusalWorktree), nil
	}
	queue, err := ReadQueue(ctx, db, id)
	if err != nil {
		return nil, err
	}
	if queue.Paused {
		return new(protocol.ConversationContinuationRefusalQueuePaused), nil
	}
	if len(queue.Items) > 0 {
		return new(protocol.ConversationContinuationRefusalQueuedInput), nil
	}
	var running, waiting bool
	err = db.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('running','stopping')),EXISTS(SELECT 1 FROM agent_questions q JOIN turns t ON t.id=q.turn WHERE t.session=?1 AND json_extract(q.body,'$.state.kind')='pending') OR EXISTS(SELECT 1 FROM agent_approvals q JOIN turns t ON t.id=q.turn WHERE t.session=?1 AND json_extract(q.body,'$.state')='pending')`, id.String()).Scan(&running, &waiting)
	if err != nil {
		return nil, err
	}
	if running {
		return new(protocol.ConversationContinuationRefusalRunning), nil
	}
	if waiting {
		return new(protocol.ConversationContinuationRefusalInteraction), nil
	}
	return nil, nil
}

func ScheduleBackground(ctx context.Context, db *sql.Conn, node protocol.NodeID, session protocol.SessionID, now time.Time) (*protocol.QueuedTurn, []protocol.Event, error) {
	if _, err := db.ExecContext(ctx, "SAVEPOINT background_delivery"); err != nil {
		return nil, nil, err
	}
	turn, events, err := admitBackground(ctx, db, node, session, now)
	if err != nil {
		if _, rollback := db.ExecContext(ctx, "ROLLBACK TO background_delivery"); rollback != nil {
			return nil, nil, rollback
		}
		if _, release := db.ExecContext(ctx, "RELEASE background_delivery"); release != nil {
			return nil, nil, release
		}
		// The results stay deliverable: the next claim pass tries again, so a
		// corrected cause such as a re-enabled provider needs no other trigger.
		reportDelivery(session, err.Error())
		return nil, nil, nil
	}
	if turn != nil {
		reportDelivery(session, "")
	}
	_, err = db.ExecContext(ctx, "RELEASE background_delivery")
	return turn, events, err
}

func admitBackground(ctx context.Context, db *sql.Conn, node protocol.NodeID, id protocol.SessionID, now time.Time) (*protocol.QueuedTurn, []protocol.Event, error) {
	var eligible bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_background WHERE "+backgroundEligible+")", id.String()).Scan(&eligible); err != nil {
		return nil, nil, err
	}
	if !eligible {
		return nil, nil, nil
	}
	var sourceText string
	if err := db.QueryRowContext(ctx, "SELECT json_extract(body,'$.task.turn') FROM agent_background WHERE "+backgroundEligible+" ORDER BY sequence,id LIMIT 1", id.String()).Scan(&sourceText); err != nil {
		return nil, nil, err
	}
	source, err := protocol.ParseTurnID(sourceText)
	if err != nil {
		return nil, nil, err
	}
	origin, err := OriginalTurnUser(ctx, db, source)
	if err != nil {
		return nil, nil, err
	}
	user, err := ResolveUser(ctx, db, origin.ID)
	if err != nil {
		return nil, nil, err
	}
	ctx = WithCapturedUser(WithUser(ctx, user), origin.ID)
	refusal, err := automaticRefusal(ctx, db, id)
	if err != nil || refusal != nil {
		return nil, nil, err
	}
	chain, err := ReadAutomaticChain(ctx, db, id)
	if err != nil {
		return nil, nil, err
	}
	if chain.Count >= chain.Limit {
		events, err := holdBackground(ctx, db, id, chain.Limit)
		return nil, events, err
	}
	records, results, err := readBackgroundBatch(ctx, db, id, nil, false)
	if err != nil {
		return nil, nil, err
	}
	if len(records) == 0 {
		return nil, nil, nil
	}
	session, err := ReadSession(ctx, db, id)
	if err != nil {
		return nil, nil, err
	}
	request := protocol.NewRequest(node, protocol.Command{Kind: "deliver_background", Data: &protocol.CommandDeliverBackgroundPayload{Session: id}})
	body, err := encode(request)
	if err != nil {
		return nil, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'completed')", node[:], request.ID.String(), body); err != nil {
		return nil, nil, err
	}
	turn, err := admitAgentTurn(ctx, db, node, node, request, session, protocol.ConversationRunKindTask, true)
	if err != nil {
		return nil, nil, err
	}
	run, err := ReadAgentRun(ctx, db, turn.ID)
	if err != nil {
		return nil, nil, err
	}
	run.Notice = true
	run.Notices = backgroundNotices(results)
	if err = putAgentRun(ctx, db, run); err != nil {
		return nil, nil, err
	}
	input, err := encode(protocol.DefaultConversationInput())
	if err != nil {
		return nil, nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET input=?2 WHERE turn=?1", turn.ID.String(), input); err != nil {
		return nil, nil, err
	}
	for _, record := range records {
		record.Task.Delivery, record.Task.Hold = &turn.ID, nil
		if err = saveBackgroundRecord(ctx, db, &record); err != nil {
			return nil, nil, err
		}
	}
	output := protocol.Output{Kind: "queued_turn", Data: turn}
	result, err := encode(protocol.Result[protocol.Output, protocol.Fault]{Ok: &output})
	if err != nil {
		return nil, nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE requests SET result=?3 WHERE caller=?1 AND id=?2", node[:], request.ID.String(), string(result)); err != nil {
		return nil, nil, err
	}
	events, err := activityEvent(ctx, db, id)
	return &turn, events, err
}
