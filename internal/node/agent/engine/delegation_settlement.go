package engine

import (
	"context"
	"database/sql"
	"fmt"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// executeOwned handles an unexpected panic without ending the Node. If the
// Node is shutting down before settlement completes, recovery owns the run.
func (r *Runtime) executeOwned(x *execution) {
	defer func() {
		value := recover()
		if value == nil {
			return
		}
		logPanic(x, "run", "", value)
		x.cancel()
		r.drainOperations(x)
		settled := r.cancelForeground(x)
		_ = r.closeTools(x)
		if !settled {
			return
		}
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
				r.RefreshCommitted((*x.inv.Session.Delegation).Session, run.Turn)
			}
		}
	}()
	r.execute(x)
}

// stopChildCommands ends the background commands of a child whose run has
// ended. Their completion is never delivered to a delegated session and no
// record hands them to the parent, so they would otherwise outlive every
// reader. Call under the publication lock only when settlement has no successor,
// before an idle child can admit another turn and reuse the same command scope.
func (r *Runtime) stopChildCommands(x *execution, run protocol.ConversationRun) {
	if r.stopCommands == nil || r.ctx.Err() != nil || x.inv.Session.Delegation == nil || *x.inv.Session.Delegation == nil {
		return
	}
	if childActive(run.Status) || run.Status == protocol.ConversationStatusContinued {
		return
	}
	r.stopCommands(x.inv.Session.ID)
}

// recoveredTurn runs the turn body. Its panic becomes an ordinary failed turn:
// calls are settled, children cancelled and the run finished by execute. A
// stopped or failed turn cancels every child of its session at once; execute
// waits for them in cancelForeground before the run is finished.
func (r *Runtime) recoveredTurn(x *execution) (err error) {
	defer func() {
		if value := recover(); value != nil {
			logPanic(x, "turn", "", value)
			err = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "turn failed unexpectedly; calls were not replayed"}
		}
		if err != nil || x.ctx.Err() != nil {
			r.stopChildren(x)
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
	// The runtime ended the turn, so it says so as a note clients localize,
	// not as an assistant reply in the child's model history.
	value := &canonical.Note{Note: canonical.OperationNote{Kind: "turn_limit"}}
	record := canonical.NewRecord(x.inv.Run.Turn, "operation", &canonical.Operation{Kind: "note", Data: value}, r.clock.Now())
	record.Provenance.Author = "system"
	record.Metadata.Runtime = &canonical.Runtime{UserVisible: true, Notes: &canonical.OperationNotes{TurnLimit: true}}
	return true, r.save(x.ctx, x.inv.Session.ID, record)
}

// childTurnLimitSummary tells the parent that a child stopped at its role's
// action limit rather than finishing; its last reply may be partial.
const childTurnLimitSummary = "Stopped at its action limit before finishing; guide_agent can continue it."

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
