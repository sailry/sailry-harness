package engine

import (
	"context"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// BeginOperation and drainOperations share the execution lock, sealing admission
// before terminal history is committed and waiting for already admitted effects.
func (r *Runtime) BeginOperation(turn protocol.TurnID) (context.Context, func(), bool) {
	r.mu.Lock()
	owner := r.active[turn]
	if owner == nil || owner.finishing || owner.ctx.Err() != nil {
		r.mu.Unlock()
		return nil, nil, false
	}
	if owner.operations == 0 {
		owner.operationsDone = make(chan struct{})
	}
	owner.operations++
	r.mu.Unlock()
	var once sync.Once
	return owner.ctx, func() {
		once.Do(func() {
			r.mu.Lock()
			owner.operations--
			if owner.operations == 0 {
				close(owner.operationsDone)
			}
			r.mu.Unlock()
		})
	}, true
}
func (r *Runtime) drainOperations(owner *execution) {
	r.mu.Lock()
	owner.finishing = true
	count, done := owner.operations, owner.operationsDone
	r.mu.Unlock()
	if count != 0 {
		<-done
	}
}
