package engine

import (
	"context"
	"database/sql"
	"fmt"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// executeOwned finishes the run even when its finishing code panics, so a
// defect in one run never ends the Node or strands the durable slot.
func (r *Runtime) executeOwned(x *execution) {
	defer func() {
		value := recover()
		if value == nil {
			return
		}
		logPanic(x, "run", "", value)
		x.cancel()
		r.drainOperations(x)
		r.cancelForeground(x)
		_ = r.closeTools(x)
		ctx := context.WithoutCancel(r.ctx)
		failure := &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "execution failed unexpectedly; calls were not replayed"}
		if x.inv.Session.Delegation != nil {
			failure.Message = "child " + failure.Message
		}
		unlock := r.LockPublication()
		defer unlock()
		var run protocol.ConversationRun
		err := r.db.Write(ctx, func(db *sql.Conn) error {
			var events []protocol.Event
			var err error
			run, events, err = store.FinishTurn(ctx, db, x.inv.Run.Turn, protocol.ConversationStatusFailed, failure, r.clock.Now())
			if err != nil {
				return err
			}
			_, err = store.RecordEvents(ctx, db, r.node, events)
			return err
		})
		if err == nil {
			r.publish()
			r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "run", Data: run})
			r.RefreshCommitted(run.Session)
			if x.inv.Session.Delegation != nil && *x.inv.Session.Delegation != nil {
				r.RefreshCommitted((*x.inv.Session.Delegation).Session)
			}
		}
	}()
	r.execute(x)
}

// recoveredTurn runs the turn body. Its panic becomes an ordinary failed turn:
// calls are settled, children cancelled and the run finished by execute.
func (r *Runtime) recoveredTurn(x *execution) (err error) {
	defer func() {
		if value := recover(); value != nil {
			logPanic(x, "turn", "", value)
			err = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "turn failed unexpectedly; calls were not replayed"}
		}
	}()
	return r.turn(x)
}

func usedChildTurns(x *execution, state store.AgentState) uint32 {
	var count uint32
	for _, record := range state.Records {
		if record.Turn != x.inv.Run.Turn || record.Metadata.Runtime != nil && !record.Metadata.Runtime.AgentVisible {
			continue
		}
		if message := record.Message(); message != nil && message.Role == canonical.Assistant {
			count++
		}
	}
	return count
}

func (r *Runtime) childTurnLimit(x *execution, state store.AgentState) (bool, error) {
	if x.childRole == nil || x.childRole.MaxTurns == nil || usedChildTurns(x, state) < *x.childRole.MaxTurns {
		return false, nil
	}
	record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "I've reached the maximum number of actions I can do without user input. Would you like me to continue?")), r.clock.Now())
	record.Provenance.Author = "assistant"
	return true, r.save(x.ctx, x.inv.Session.ID, record)
}

func childBudgetContext(x *execution, state store.AgentState) string {
	if x.childRole == nil || x.childRole.MaxTurns == nil {
		return ""
	}
	used, limit := usedChildTurns(x, state), *x.childRole.MaxTurns
	if limit == 0 || uint64(used)*2 < uint64(limit) {
		return ""
	}
	return fmt.Sprintf("<turn-budget>%d/%d used</turn-budget>", used, limit)
}
