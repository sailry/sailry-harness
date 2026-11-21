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
	var owner *execution
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
			owner = x
		}
	}
	r.mu.Unlock()
	if owner != nil && owner.interruptTools != nil {
		owner.interruptTools(errQueuedSteer)
	}
}

func (r *Runtime) beginToolStep(x *execution) (context.Context, func()) {
	step, cancel := context.WithCancelCause(x.ctx)
	unlock := r.LockPublication()
	x.interruptTools = cancel
	if len(x.queuedSteer) > 0 {
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
