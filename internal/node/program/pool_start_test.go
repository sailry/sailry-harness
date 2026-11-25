package program

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestPoolInvalidationDuringStart(t *testing.T) {
	for _, operation := range []string{"invalidate", "close", "another owner"} {
		t.Run(operation, func(t *testing.T) {
			pool := NewPool(1, time.Minute)
			defer pool.Close()
			spec := fixtureSpec(t, "serve")
			starting, resume := make(chan struct{}), make(chan struct{})
			type result struct {
				entry   *Entry
				err     error
				process *Process
			}
			finished := make(chan result, 1)
			go func() {
				var process *Process
				entry, err := pool.Lease(t.Context(), "owner", 1, func(*Entry) int { return 1 }, func() (*Process, any, error) {
					close(starting)
					<-resume
					var err error
					process, err = Start(spec)
					return process, nil, err
				})
				finished <- result{entry, err, process}
			}()
			<-starting
			switch operation {
			case "invalidate":
				pool.Invalidate("owner")
			case "close":
				pool.Close()
			case "another owner":
				pool.Invalidate("another")
			}
			close(resume)
			got := <-finished
			if got.process == nil {
				t.Fatalf("fixture failed to start: %v", got.err)
			}
			defer got.process.Kill()
			if operation == "another owner" {
				if got.err != nil || got.entry == nil {
					t.Fatalf("unrelated invalidation rejected the lease: %v", got.err)
				}
				pool.Release(got.entry, false)
			} else if got.err == nil || got.entry != nil {
				t.Fatalf("invalidated launch escaped the pool: %v, %v", got.entry, got.err)
			}
			select {
			case <-got.process.Done():
			case <-time.After(10 * time.Second):
				t.Fatal("invalidated process was not stopped")
			}
			if operation == "invalidate" {
				entry, err := pool.Lease(t.Context(), "owner", 1, func(*Entry) int { return 1 }, func() (*Process, any, error) {
					process, err := Start(spec)
					return process, nil, err
				})
				if err != nil {
					t.Fatalf("replacement could not use the released slot: %v", err)
				}
				pool.Release(entry, false)
			}
		})
	}
}

func TestPoolLeaseAfterCancellation(t *testing.T) {
	pool := NewPool(1, time.Minute)
	defer pool.Close()
	spec := fixtureSpec(t, "serve")
	start := func() (*Process, any, error) {
		process, err := Start(spec)
		return process, nil, err
	}
	idle, err := pool.Lease(t.Context(), "owner", 1, func(*Entry) int { return 0 }, start)
	if err != nil {
		t.Fatal(err)
	}
	pool.Release(idle, true)
	// The idle program is not compatible, so a lease at the limit would
	// replace it.
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	started := false
	entry, err := pool.Lease(ctx, "owner", 1, func(*Entry) int { return 0 }, func() (*Process, any, error) {
		started = true
		return start()
	})
	if !errors.Is(err, context.Canceled) || entry != nil || started {
		t.Fatalf("cancelled lease = %v, %v, started %v", entry, err, started)
	}
	select {
	case <-idle.Process.Done():
		t.Fatal("a cancelled lease stopped the idle program")
	default:
	}
}
