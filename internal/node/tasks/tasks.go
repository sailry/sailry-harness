package tasks

import (
	"context"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const Capacity = 64

type Supervisor struct {
	ctx     context.Context
	cancel  context.CancelFunc
	mu      sync.Mutex
	stopped bool
	permits chan struct{}
	work    sync.WaitGroup
}

func New(ctx context.Context) *Supervisor {
	lifetime, cancel := context.WithCancel(context.WithoutCancel(ctx))
	return &Supervisor{ctx: lifetime, cancel: cancel, permits: make(chan struct{}, Capacity)}
}

func (owner *Supervisor) Submit(work func(context.Context)) error {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if owner.stopped {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "task supervisor is stopped"}
	}
	select {
	case owner.permits <- struct{}{}:
	default:
		return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "task supervisor is busy"}
	}
	owner.work.Add(1)
	go func() {
		defer owner.work.Done()
		defer func() { <-owner.permits }()
		work(owner.ctx)
	}()
	return nil
}

func (owner *Supervisor) Close(ctx context.Context) error {
	owner.mu.Lock()
	owner.stopped = true
	owner.mu.Unlock()
	done := make(chan struct{})
	go func() { owner.work.Wait(); close(done) }()
	timer := time.NewTimer(5 * time.Second)
	defer timer.Stop()
	select {
	case <-done:
		owner.cancel()
		return nil
	case <-timer.C:
	case <-ctx.Done():
	}
	owner.cancel()
	select {
	case <-done:
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "task shutdown exceeded its grace period"}
	case <-ctx.Done():
		return ctx.Err()
	}
}
