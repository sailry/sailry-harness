package engine

import (
	"context"
	"database/sql"
	"errors"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) deliverBackground(x *execution, initial bool) (bool, error) {
	return r.commitCallback(x, func(db *sql.Conn) (*protocol.ConversationEntry, []protocol.Event, error) {
		if initial && !x.inv.Run.Notice {
			return store.DeliverInputBackground(x.ctx, db, x.inv.Run.Turn, r.clock.Now())
		}
		return store.DeliverBackground(x.ctx, db, x.inv.Run.Turn, initial, r.clock.Now())
	})
}

func (r *Runtime) deliverForeground(x *execution, state store.ChildState) error {
	turn := x.inv.Run.Turn
	child := state.Run
	result := canonical.BackgroundResult{
		Notice: protocol.ConversationBackgroundNotice{
			ID:   "agent:" + child.Turn.String(),
			Kind: protocol.ConversationBackgroundKind{Kind: "agent", Data: protocol.ConversationBackgroundKindAgentPayload{Session: child.Session, Turn: child.Turn}},
		},
	}
	switch child.Status {
	case protocol.ConversationStatusCompleted:
		result.Notice.Outcome = protocol.ConversationBackgroundOutcomeCompleted
		if state.TurnLimit {
			result.Notice.Summary = new(store.ChildTurnLimitSummary)
		}
		if state.Response != nil {
			result.ChildResponse(*state.Response, uint64(len(*state.Response)), 16384)
		}
	case protocol.ConversationStatusCancelled:
		result.Notice.Outcome = protocol.ConversationBackgroundOutcomeCancelled
	case protocol.ConversationStatusInterrupted:
		result.Notice.Outcome = protocol.ConversationBackgroundOutcomeInterrupted
	default:
		result.Notice.Outcome = protocol.ConversationBackgroundOutcomeFailed
		if child.Error != nil {
			result.Notice.Summary = callbackPreview(&child.Error.Message, 512)
			result.ErrorCode = &child.Error.Code
		}
	}
	_, err := r.commitCallback(x, func(db *sql.Conn) (*protocol.ConversationEntry, []protocol.Event, error) {
		parent, err := store.ReadAgentRun(x.ctx, db, turn)
		if err != nil {
			return nil, nil, err
		}
		if parent.Status == protocol.ConversationStatusStopping {
			return nil, nil, context.Canceled
		}
		if parent.Status != protocol.ConversationStatusRunning {
			return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "turn cannot take a foreground child result"}
		}
		record := canonical.NewRecord(turn, "background", &canonical.Background{CompactSummary: true, Results: []canonical.BackgroundResult{result}}, r.clock.Now())
		record.Provenance.Author = "system"
		runtimeMeta := &canonical.Runtime{AgentVisible: true}
		if len(state.FileRevisions) > 0 {
			runtimeMeta.FileChanges = map[string]map[string]string{child.Worktree.String(): state.FileRevisions}
		}
		record.Metadata.Runtime = runtimeMeta
		entry, events, err := store.AppendCanonical(x.ctx, db, x.inv.Session.ID, record)
		if err != nil {
			return nil, nil, err
		}
		consumed, err := store.ConsumeChildBackground(x.ctx, db, turn, child.Session, &child.Turn)
		return &entry, append(events, consumed...), err
	})
	if err == nil {
		r.markCollectedChild(x, child.Session, &child.Turn)
	}
	return err
}

func callbackPreview(value *string, limit int) *string {
	if value == nil || len(*value) <= limit {
		return value
	}
	for !utf8.RuneStart((*value)[limit]) {
		limit--
	}
	return new((*value)[:limit] + "…")
}

func (r *Runtime) commitCallback(x *execution, commit func(*sql.Conn) (*protocol.ConversationEntry, []protocol.Event, error)) (bool, error) {
	if err := x.ctx.Err(); err != nil {
		return false, err
	}
	unlock := r.LockPublication()
	defer unlock()
	var entry *protocol.ConversationEntry
	var changed bool
	err := r.db.Write(x.ctx, func(db *sql.Conn) error {
		var events []protocol.Event
		var err error
		entry, events, err = commit(db)
		if err != nil {
			return err
		}
		changed = len(events) != 0
		_, err = store.RecordEvents(x.ctx, db, r.node, events)
		return err
	})
	if errors.Is(err, context.Canceled) {
		x.cancel()
	}
	if err != nil || entry == nil && !changed {
		return false, err
	}
	r.publish()
	if entry != nil {
		x.callbackPending = true
		r.feed.Publish(x.inv.Session.ID, protocol.ConversationChange{Kind: "hidden", Data: protocol.ConversationChangeHiddenPayload{Turn: entry.Turn, ID: entry.ID}})
	}
	r.RefreshCommitted(x.inv.Session.ID)
	return entry != nil, nil
}
