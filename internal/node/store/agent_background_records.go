package store

import (
	"context"
	"database/sql"
	"time"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ConsumeCommandBackground(ctx context.Context, db *sql.Conn, parent protocol.TurnID, snapshot protocol.ProcessSnapshot, now time.Time) ([]protocol.Event, error) {
	run, err := activeAgentRun(ctx, db, parent)
	if err != nil {
		return nil, err
	}
	if run.Session != snapshot.Info.Session {
		return nil, fault(protocol.ErrorCodePermissionDenied, "command belongs to another session")
	}
	if snapshot.Info.Status.Kind == "running" || snapshot.Info.Status.Kind == "stopping" {
		return nil, nil
	}
	if _, err = FinishCommand(ctx, db, process.Finished{Snapshot: snapshot}, now); err != nil {
		return nil, err
	}
	record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+snapshot.Info.ID.String())
	if err != nil {
		return nil, err
	}
	changed := false
	if record.Task.State != protocol.ConversationBackgroundStateConsumed && record.Task.State != protocol.ConversationBackgroundStateDismissed {
		record.Task.State, record.Task.Delivery = protocol.ConversationBackgroundStateConsumed, nil
		if err = saveBackgroundRecord(ctx, db, record); err != nil {
			return nil, err
		}
		changed = true
	}
	stall, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "stall:"+snapshot.Info.ID.String())
	if err != nil {
		return nil, err
	}
	if stall != nil && stall.Task.State == protocol.ConversationBackgroundStatePending && stall.Task.Delivery == nil {
		stall.Task.State = protocol.ConversationBackgroundStateDismissed
		if err = saveBackgroundRecord(ctx, db, stall); err != nil {
			return nil, err
		}
		changed = true
	}
	if changed {
		return activityEvent(ctx, db, run.Session)
	}
	return nil, nil
}

func RegisterChildBackground(ctx context.Context, db *sql.Conn, parent, child protocol.ConversationRun, title string, now time.Time) ([]protocol.Event, error) {
	id := "agent:" + child.Turn.String()
	record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", id)
	if err != nil {
		return nil, err
	}
	if record != nil {
		return nil, nil
	}
	started := now.UnixMilli()
	if child.StartedMs != nil {
		started = *child.StartedMs
	}
	value := BackgroundRecord{Task: protocol.ConversationBackgroundTask{ID: id, Session: parent.Session, Turn: parent.Turn, Kind: protocol.ConversationBackgroundKind{Kind: "agent", Data: protocol.ConversationBackgroundKindAgentPayload{Session: child.Session, Turn: child.Turn}}, Title: processPreview(title, 200), State: protocol.ConversationBackgroundStateRunning, StartedMs: started}}
	if err = saveBackgroundRecord(ctx, db, &value); err != nil {
		return nil, err
	}
	return activityEvent(ctx, db, parent.Session)
}

func FinishChildBackground(ctx context.Context, db *sql.Conn, child protocol.ConversationRun, now time.Time) ([]protocol.Event, error) {
	switch child.Status {
	case protocol.ConversationStatusQueued, protocol.ConversationStatusRunning, protocol.ConversationStatusStopping, protocol.ConversationStatusContinued:
		return nil, nil
	}
	var guidance bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1 AND p.recipient IS NOT NULL)", child.Session.String()).Scan(&guidance); err != nil {
		return nil, err
	}
	if guidance {
		return nil, nil
	}
	records, err := readBodies[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE kind='agent' AND source=?1 AND state='running'", child.Session.String())
	if err != nil {
		return nil, err
	}
	owners := map[protocol.SessionID]bool{}
	for _, record := range records {
		record.Task.Kind = protocol.ConversationBackgroundKind{Kind: "agent", Data: protocol.ConversationBackgroundKindAgentPayload{Session: child.Session, Turn: child.Turn}}
		if child.Status == protocol.ConversationStatusCompleted {
			record.Response, err = childResponse(ctx, db, child.Turn)
			if err != nil {
				return nil, err
			}
			if record.Response != nil {
				v := processPreview(*record.Response, 4096)
				record.Response = &v
			}
		}
		outcome := protocol.ConversationBackgroundOutcomeInterrupted
		switch child.Status {
		case protocol.ConversationStatusCompleted:
			outcome = protocol.ConversationBackgroundOutcomeCompleted
		case protocol.ConversationStatusFailed:
			outcome = protocol.ConversationBackgroundOutcomeFailed
		case protocol.ConversationStatusCancelled:
			outcome = protocol.ConversationBackgroundOutcomeCancelled
		}
		summary := record.Response
		if summary == nil && child.Error != nil {
			summary = &child.Error.Message
		}
		if err = terminalCommandRecord(ctx, db, &record, outcome, summary, now); err != nil {
			return nil, err
		}
		if child.Status == protocol.ConversationStatusCancelled || child.Status == protocol.ConversationStatusInterrupted {
			// The owning turn already told the model about a foreground
			// child it cancelled, so the result is not delivered again.
			var reported bool
			if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_events e, json_each(e.body,'$.metadata.runtime.notes.cancelled_subagents') n WHERE e.turn=?1 AND n.value=?2)", record.Task.Turn.String(), child.Session.String()).Scan(&reported); err != nil {
				return nil, err
			}
			if reported {
				record.Task.State = protocol.ConversationBackgroundStateConsumed
				if err = saveBackgroundRecord(ctx, db, &record); err != nil {
					return nil, err
				}
			}
		}
		owners[record.Task.Session] = true
	}
	return backgroundOwnerEvents(ctx, db, owners)
}

func ConsumeChildBackground(ctx context.Context, db *sql.Conn, parent protocol.TurnID, child protocol.SessionID, exact *protocol.TurnID) ([]protocol.Event, error) {
	run, err := ownedChild(ctx, db, parent, child, exact)
	if err != nil {
		return nil, err
	}
	owner, err := ReadAgentRun(ctx, db, parent)
	if err != nil {
		return nil, err
	}
	records, err := readBodies[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE kind='agent' AND source=?1", child.String())
	if err != nil {
		return nil, err
	}
	changed := false
	for _, record := range records {
		kind, ok := processStatusData[protocol.ConversationBackgroundKindAgentPayload](record.Task.Kind.Data)
		if !ok || kind.Turn != run.Turn || record.Task.Session != owner.Session || record.Task.State == protocol.ConversationBackgroundStateRunning || record.Task.State == protocol.ConversationBackgroundStateConsumed || record.Task.State == protocol.ConversationBackgroundStateDismissed {
			continue
		}
		record.Task.State = protocol.ConversationBackgroundStateConsumed
		record.Task.Delivery = nil
		if err = saveBackgroundRecord(ctx, db, &record); err != nil {
			return nil, err
		}
		changed = true
	}
	if changed {
		return activityEvent(ctx, db, owner.Session)
	}
	return nil, nil
}

// ReleaseBackground returns the results reserved for a turn that did not take
// them. The hold is why the turn did not. With suppress they are suppressed:
// they start no notice turn and join the user's next message, and the hold
// tells clients why. Without it only the results that were already suppressed
// take the hold, so none is left held without a reason.
func ReleaseBackground(ctx context.Context, db *sql.Conn, turn protocol.TurnID, hold *protocol.ConversationBackgroundHold, suppress bool) ([]protocol.Event, error) {
	records, err := readBodies[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE state='pending' AND delivery=?1", turn.String())
	if err != nil {
		return nil, err
	}
	owners := map[protocol.SessionID]bool{}
	for _, record := range records {
		record.Task.Delivery = nil
		if hold != nil && (suppress || record.Task.Suppressed) {
			record.Task.Suppressed, record.Task.Hold = true, hold
		}
		if err = saveBackgroundRecord(ctx, db, &record); err != nil {
			return nil, err
		}
		owners[record.Task.Session] = true
	}
	return backgroundOwnerEvents(ctx, db, owners)
}

func backgroundOwnerEvents(ctx context.Context, db *sql.Conn, owners map[protocol.SessionID]bool) ([]protocol.Event, error) {
	events := []protocol.Event{}
	for owner := range owners {
		changed, err := activityEvent(ctx, db, owner)
		if err != nil {
			return nil, err
		}
		events = append(events, changed...)
	}
	return events, nil
}
