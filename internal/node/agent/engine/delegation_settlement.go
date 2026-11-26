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
			if run.Status == protocol.ConversationStatusCancelled && x.inv.Session.Delegation == nil {
				r.stopTurnCommands(run.Session, x)
			}
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

// handoffChildGuidance gives already queued guidance the same owner and tool
// boundary. A controller-retried child has no in-memory delegation owner.
func (r *Runtime) handoffChildGuidance(x *execution, child *delegatedChild, successor protocol.ConversationRun) {
	next := &delegatedChild{session: successor.Session, turn: successor.Turn, sequence: successor.Sequence, released: true, commandsInherited: true}
	if child == nil {
		origin := *x.inv.Session.Delegation
		next.root, next.parent, next.detached = origin.Session, origin.Turn, true
		next.ctx, next.cancel = context.WithCancel(r.ctx)
		next.tools, next.connections = x.parentTools, x.inv.ConnectionTargets
	} else {
		next.root, next.parent, next.overlap, next.detached = child.root, child.parent, child.overlap, child.detached
		next.ctx, next.cancel = child.ctx, child.cancel
		next.tools, next.connections = child.tools, child.connections
	}
	r.mu.Lock()
	if child != nil {
		child.successor = next
		delete(r.children, child.turn)
	}
	if r.children == nil {
		r.children = make(map[protocol.TurnID]*delegatedChild)
	}
	r.children[next.turn] = next
	r.mu.Unlock()
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
			err = &observedTurnPanic{&protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "turn failed unexpectedly; calls were not replayed"}}
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

const (
	// Runtime-managed default step budgets for delegated children when no role
	// MaxTurns is set, preventing unbounded serial loops without requiring users
	// to tune turn caps per role.
	defaultChildMaxTurns         uint32 = 20
	defaultReadOnlyChildMaxTurns uint32 = 12
)

func effectiveChildMaxTurns(x *execution) (uint32, bool) {
	if x.childRole != nil && x.childRole.MaxTurns != nil {
		return *x.childRole.MaxTurns, true
	}
	if x.inv.Session.Delegation == nil || *x.inv.Session.Delegation == nil {
		return 0, false
	}
	if x.inv.Turn.Config.Mode == protocol.WorkModePlan || (x.childRole != nil && x.childRole.ReadOnly) {
		return defaultReadOnlyChildMaxTurns, true
	}
	return defaultChildMaxTurns, true
}

func childTurnBudgetExhausted(x *execution, state store.AgentState) bool {
	limit, bounded := effectiveChildMaxTurns(x)
	return bounded && usedChildTurns(x, state) >= limit
}

func (r *Runtime) recordChildTurnLimit(x *execution) error {
	// The runtime ended the turn, so it says so as a note clients localize,
	// not as an assistant reply in the child's model history.
	value := &canonical.Note{Note: canonical.OperationNote{Kind: "turn_limit"}}
	record := canonical.NewRecord(x.inv.Run.Turn, "operation", &canonical.Operation{Kind: "note", Data: value}, r.clock.Now())
	record.Provenance.Author = "system"
	record.Metadata.Runtime = &canonical.Runtime{UserVisible: true, Notes: &canonical.OperationNotes{TurnLimit: true}}
	return r.save(x.ctx, x.inv.Session.ID, record)
}

func childBudgetContext(x *execution, state store.AgentState) string {
	limit, bounded := effectiveChildMaxTurns(x)
	if !bounded || limit == 0 {
		return ""
	}
	used := usedChildTurns(x, state)
	if uint64(used)*2 < uint64(limit) {
		return ""
	}
	if uint64(used)+2 >= uint64(limit) {
		return fmt.Sprintf("<turn-budget>%d/%d used — wrap up and return your final concise report now</turn-budget>", used, limit)
	}
	return fmt.Sprintf("<turn-budget>%d/%d used</turn-budget>", used, limit)
}
