package engine

import (
	"context"
	"errors"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var errQueuedSteer = errors.New("step redirected by queued guidance")

type foregroundToolKey struct{}

// QueueCommitted reconciles steering against the durable queue under publication.
func (r *Runtime) QueueCommitted(update protocol.ConversationQueueUpdate, selection *protocol.CommandGuideQueuedTurnPayload) {
	r.mu.Lock()
	var interruptTools, interruptStream context.CancelCauseFunc
	for _, x := range r.active {
		if x.inv.Session.ID != update.Session || x.ctx.Err() != nil {
			continue
		}
		addressed := func(id protocol.TurnID) bool {
			return slices.ContainsFunc(update.Queue.Items, func(item protocol.ConversationPending) bool {
				return item.Turn == id && item.Guidance != nil && *item.Guidance == x.inv.Run.Turn
			})
		}
		x.queuedSteer = slices.DeleteFunc(x.queuedSteer, func(id protocol.TurnID) bool { return !addressed(id) })
		if selection != nil && selection.Recipient == x.inv.Run.Turn && addressed(selection.Turn) {
			if !slices.Contains(x.queuedSteer, selection.Turn) {
				x.queuedSteer = append(x.queuedSteer, selection.Turn)
			}
			interruptTools, interruptStream = x.interruptTools, x.interruptStream
		}
	}
	r.mu.Unlock()
	if interruptTools != nil {
		interruptTools(errQueuedSteer)
	}
	if interruptStream != nil {
		interruptStream(errQueuedSteer)
	}
}

// GuideCommitted delivers direct composer guidance under publication so it
// immediately interrupts active foreground tools, approvals, questions, and
// in-flight model streaming to steer the running turn.
func (r *Runtime) GuideCommitted(turn protocol.QueuedTurn, recipient protocol.TurnID) {
	if recipient == (protocol.TurnID{}) {
		return
	}
	r.mu.Lock()
	var interruptTools, interruptStream context.CancelCauseFunc
	owner := r.active[recipient]
	if owner != nil && (owner.inv.Session.ID != turn.Session || owner.ctx.Err() != nil) {
		owner = nil
	}
	if owner != nil {
		owner.directSteer = true
		interruptTools, interruptStream = owner.interruptTools, owner.interruptStream
	}
	r.mu.Unlock()
	if interruptTools != nil {
		interruptTools(errQueuedSteer)
	}
	if interruptStream != nil {
		interruptStream(errQueuedSteer)
	}
}

func (r *Runtime) hasDirectSteer(x *execution) bool {
	if x.foregroundSteered {
		return false
	}
	unlock := r.LockPublication()
	defer unlock()
	return x.directSteer
}

func (r *Runtime) beginModelStep(x *execution) (context.Context, func()) {
	step, cancel := context.WithCancelCause(x.ctx)
	unlock := r.LockPublication()
	x.interruptStream = cancel
	if len(x.queuedSteer) > 0 || x.directSteer {
		cancel(errQueuedSteer)
	}
	unlock()
	return step, func() {
		unlock := r.LockPublication()
		x.interruptStream = nil
		unlock()
		cancel(nil)
	}
}

func (r *Runtime) beginToolStep(x *execution) (context.Context, func()) {
	step, cancel := context.WithCancelCause(x.ctx)
	unlock := r.LockPublication()
	x.interruptTools = cancel
	if len(x.queuedSteer) > 0 || x.directSteer {
		cancel(errQueuedSteer)
	}
	unlock()
	return step, func() {
		unlock := r.LockPublication()
		x.interruptTools = nil
		unlock()
		cancel(nil)
	}
}

func (r *Runtime) hasQueuedSteer(x *execution) bool {
	if x.foregroundSteered {
		return false
	}
	unlock := r.LockPublication()
	defer unlock()
	return len(x.queuedSteer) > 0
}

// ForegroundToolContext lets a command observe steering only during foreground
// execution. Its preparation and durable receipts use the operation owner.
func ForegroundToolContext(ctx, operation context.Context) context.Context {
	if step, ok := ctx.Value(foregroundToolKey{}).(context.Context); ok {
		return step
	}
	return operation
}

func Steered(ctx context.Context) bool {
	return context.Cause(ctx) == errQueuedSteer
}
