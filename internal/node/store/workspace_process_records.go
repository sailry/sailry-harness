package store

import (
	"context"
	"database/sql"
	"errors"
	"math"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// BackgroundRecord retains completion evidence independently of process handles.
// B6 consumes its follow-up flag through the same authoritative writer.
type BackgroundRecord struct {
	Task      protocol.ConversationBackgroundTask `json:"task"`
	FollowUp  bool                                `json:"follow_up"`
	Stdout    *string                             `json:"stdout"`
	Stderr    *string                             `json:"stderr"`
	Response  *string                             `json:"response"`
	ElapsedMs *uint64                             `json:"elapsed_ms"`
	Sequence  uint64                              `json:"sequence"`
}

func (record *BackgroundRecord) UnmarshalJSON(data []byte) error {
	type plain BackgroundRecord
	var value plain
	if err := protocol.DecodeObject(data, &value, []string{"task", "sequence"}, true); err != nil {
		return err
	}
	*record = BackgroundRecord(value)
	return nil
}

func RegisterCommand(ctx context.Context, db *sql.Conn, request protocol.Request, now time.Time) ([]protocol.Event, error) {
	input, ok := processCommandPayload(request.Command)
	if !ok {
		return nil, invalid("command launch required")
	}
	if !input.Background {
		return nil, nil
	}
	return registerProcessRecord(ctx, db, request.ID, input, input.FollowUp, now)
}

func RegisterPromotedCommand(ctx context.Context, db *sql.Conn, request protocol.Request, now time.Time) ([]protocol.Event, error) {
	input, ok := processCommandPayload(request.Command)
	if !ok {
		return nil, invalid("command launch required")
	}
	return registerProcessRecord(ctx, db, request.ID, input, true, now)
}

func processCommandPayload(command protocol.Command) (protocol.CommandRunCommandPayload, bool) {
	if command.Kind != "run_command" {
		return protocol.CommandRunCommandPayload{}, false
	}
	switch value := command.Data.(type) {
	case protocol.CommandRunCommandPayload:
		return value, true
	case *protocol.CommandRunCommandPayload:
		if value != nil {
			return *value, true
		}
	}
	return protocol.CommandRunCommandPayload{}, false
}

func registerProcessRecord(ctx context.Context, db *sql.Conn, id protocol.RequestID, input protocol.CommandRunCommandPayload, followUp bool, now time.Time) ([]protocol.Event, error) {
	run, err := ReadAgentRun(ctx, db, input.Turn)
	if err != nil {
		return nil, err
	}
	identity := "command:" + id.String()
	current, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", identity)
	if err != nil {
		return nil, err
	}
	if current != nil {
		if current.Task.Session != run.Session || current.Task.Turn != input.Turn {
			return nil, fault(protocol.ErrorCodeConflict, "command identity belongs to another execution")
		}
		return nil, nil
	}
	title := input.Command
	if input.Description != nil {
		title = *input.Description
	}
	record := BackgroundRecord{Task: protocol.ConversationBackgroundTask{ID: identity, Session: run.Session, Turn: input.Turn, Kind: protocol.ConversationBackgroundKind{Kind: "command", Data: protocol.ConversationBackgroundKindCommandPayload{ID: id}}, Title: processPreview(title, 200), State: protocol.ConversationBackgroundStateRunning, StartedMs: now.UnixMilli()}, FollowUp: followUp}
	if err := saveBackgroundRecord(ctx, db, &record); err != nil {
		return nil, err
	}
	return activityEvent(ctx, db, run.Session)
}

func FinishCommand(ctx context.Context, db *sql.Conn, finished process.Finished, now time.Time) ([]protocol.Event, error) {
	snapshot := finished.Snapshot
	record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+snapshot.Info.ID.String())
	if err != nil {
		return nil, err
	}
	if record == nil {
		return nil, errors.New("background command was not registered before execution")
	}
	if record.Task.Session != snapshot.Info.Session || record.Task.Turn != snapshot.Info.Turn {
		return nil, errors.New("background command ownership changed")
	}
	if record.Task.State != protocol.ConversationBackgroundStateRunning {
		return nil, nil
	}
	outcome := protocol.ConversationBackgroundOutcomeFailed
	var summary *string
	switch snapshot.Info.Status.Kind {
	case "running", "stopping":
		return nil, nil
	case "failed":
		if message, ok := processStatusData[string](snapshot.Info.Status.Data); ok {
			summary = &message
		}
	case "finished":
		result, ok := processStatusData[protocol.ProcessOutcome](snapshot.Info.Status.Data)
		if !ok {
			return nil, errors.New("invalid command completion status")
		}
		switch result.Kind {
		case "exited":
			code, ok := processStatusData[int32](result.Data)
			if !ok {
				return nil, errors.New("invalid command exit status")
			}
			if code == 0 {
				outcome = protocol.ConversationBackgroundOutcomeCompleted
			}
		case "timed_out":
			outcome = protocol.ConversationBackgroundOutcomeTimedOut
		case "cancelled":
			outcome = protocol.ConversationBackgroundOutcomeCancelled
			if finished.Explicit {
				outcome = protocol.ConversationBackgroundOutcomeStopped
			}
		case "unknown":
			outcome = protocol.ConversationBackgroundOutcomeUnknown
		}
	default:
		return nil, errors.New("invalid command status")
	}
	stdout, stderr := processTail(snapshot.Stdout.Text, 2048), processTail(snapshot.Stderr.Text, 2048)
	record.Stdout, record.Stderr, record.ElapsedMs = &stdout, &stderr, finished.ElapsedMs
	for _, text := range []string{snapshot.Stderr.Text, snapshot.Stdout.Text} {
		if text != "" {
			value := processTail(text, 512)
			summary = &value
			break
		}
	}
	if err := terminalCommandRecord(ctx, db, record, outcome, summary, now); err != nil {
		return nil, err
	}
	return activityEvent(ctx, db, record.Task.Session)
}

func processStatusData[T any](value any) (T, bool) {
	switch value := value.(type) {
	case T:
		return value, true
	case *T:
		if value != nil {
			return *value, true
		}
	}
	var zero T
	return zero, false
}

func terminalCommandRecord(ctx context.Context, db *sql.Conn, record *BackgroundRecord, outcome protocol.ConversationBackgroundOutcome, summary *string, now time.Time) error {
	record.Task.State = protocol.ConversationBackgroundStatePending
	record.Task.Outcome = &outcome
	finished := now.UnixMilli()
	record.Task.FinishedMs = &finished
	if summary != nil {
		value := processPreview(*summary, 512)
		record.Task.Summary = &value
	} else {
		record.Task.Summary = nil
	}
	sequence, err := NextCounter(ctx, db, CounterBackground)
	if err != nil {
		return err
	}
	record.Sequence = sequence
	return saveBackgroundRecord(ctx, db, record)
}

func saveBackgroundRecord(ctx context.Context, db *sql.Conn, record *BackgroundRecord) error {
	if record.Task.Revision >= math.MaxInt64 || record.Sequence > math.MaxInt64 {
		return errors.New("background revision exhausted")
	}
	record.Task.Revision++
	var kind, source string
	switch record.Task.Kind.Kind {
	case "command":
		value, ok := processStatusData[protocol.ConversationBackgroundKindCommandPayload](record.Task.Kind.Data)
		if !ok {
			return errors.New("invalid background command identity")
		}
		kind, source = "command", value.ID.String()
	case "agent":
		value, ok := processStatusData[protocol.ConversationBackgroundKindAgentPayload](record.Task.Kind.Data)
		if !ok {
			return errors.New("invalid background agent identity")
		}
		kind, source = "agent", value.Session.String()
	default:
		return errors.New("invalid background kind")
	}
	body, err := encode(record)
	if err != nil {
		return err
	}
	var delivery any
	if record.Task.Delivery != nil {
		delivery = record.Task.Delivery.String()
	}
	_, err = db.ExecContext(ctx, "INSERT INTO agent_background(id,session,turn,kind,source,state,delivery,sequence,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET state=excluded.state,delivery=excluded.delivery,sequence=excluded.sequence,body=excluded.body", record.Task.ID, record.Task.Session.String(), record.Task.Turn.String(), kind, source, string(record.Task.State), delivery, record.Sequence, body)
	return err
}

func ReadBackgroundTasks(ctx context.Context, db *sql.Conn, session protocol.SessionID) ([]protocol.ConversationBackgroundTask, error) {
	records, err := readBodies[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE session=?1 AND state IN ('running','pending') UNION ALL SELECT body FROM (SELECT body FROM agent_background WHERE session=?1 AND state NOT IN ('running','pending') ORDER BY sequence DESC LIMIT 20)", session.String())
	if err != nil {
		return nil, err
	}
	tasks := make([]protocol.ConversationBackgroundTask, 0, len(records))
	for _, record := range records {
		tasks = append(tasks, record.Task)
	}
	return tasks, nil
}

func RecoverCommands(ctx context.Context, db *sql.Conn, now time.Time) ([]protocol.Event, error) {
	records, err := readBodies[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE state IN ('running','pending')")
	if err != nil {
		return nil, err
	}
	sessions := map[protocol.SessionID]bool{}
	for _, record := range records {
		if record.Task.State == protocol.ConversationBackgroundStateRunning {
			summary := "Execution was interrupted; its outcome was not confirmed or replayed"
			if err := terminalCommandRecord(ctx, db, &record, protocol.ConversationBackgroundOutcomeInterrupted, &summary, now); err != nil {
				return nil, err
			}
		}
		if record.Task.Delivery != nil {
			run, err := ReadAgentRun(ctx, db, *record.Task.Delivery)
			if err != nil {
				return nil, err
			}
			if run.Notice && run.Status == protocol.ConversationStatusQueued {
				if _, _, err := FinishTurn(ctx, db, run.Turn, protocol.ConversationStatusCancelled, nil, now); err != nil {
					return nil, err
				}
				if _, err := db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", run.Turn.String()); err != nil {
					return nil, err
				}
			}
		}
		record.Task.Delivery = nil
		record.Task.Recovered, record.Task.Suppressed = true, true
		if err := saveBackgroundRecord(ctx, db, &record); err != nil {
			return nil, err
		}
		sessions[record.Task.Session] = true
	}
	events := []protocol.Event{}
	for session := range sessions {
		changed, err := activityEvent(ctx, db, session)
		if err != nil {
			return nil, err
		}
		events = append(events, changed...)
	}
	return events, nil
}

func processPreview(text string, limit int) string {
	if len(text) <= limit {
		return text
	}
	end := limit
	for end < len(text) && !utf8.RuneStart(text[end]) {
		end++
	}
	return text[:end]
}
func processTail(text string, limit int) string {
	if len(text) <= limit {
		return text
	}
	start := len(text) - limit
	for start < len(text) && !utf8.RuneStart(text[start]) {
		start++
	}
	return strings.Clone(text[start:])
}
