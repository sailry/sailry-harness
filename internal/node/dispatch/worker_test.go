package dispatch

import (
	"context"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchWakeup(test *testing.T) {
	test.Parallel()
	clock := clockwork.NewFakeClockAt(time.UnixMilli(0))
	ctx, cancel := context.WithCancel(test.Context())
	defer cancel()
	wake := make(chan struct{}, 1)
	done := make(chan struct{})
	executed := make(chan Work, 1)
	finished := make(chan struct{}, 1)
	var reads atomic.Int32
	next := int64(1500)
	work := Work{Job: protocol.NewJobID(), Completion: protocol.DispatchCompletionCommand}
	runner := Runner{
		Clock: clock, Wake: wake,
		Tick: func(_ context.Context, recovering bool) (Batch, error) {
			count := reads.Add(1)
			if recovering != (count == 1) {
				test.Error("startup recovery flag was not consumed exactly once")
			}
			if clock.Now().UnixMilli() < next {
				return Batch{NextMs: &next}, nil
			}
			if count == 2 {
				return Batch{Work: []Work{work}}, nil
			}
			return Batch{}, nil
		},
		Execute: func(_ context.Context, actual Work) error {
			executed <- actual
			return nil
		},
		Finished: func(_ context.Context, job protocol.JobID, result error) error {
			if job != work.Job || result != nil {
				test.Error("callback completion changed its immutable owner")
			}
			finished <- struct{}{}
			return nil
		},
	}
	go func() {
		defer close(done)
		runner.Serve(ctx)
	}()
	if err := clock.BlockUntilContext(test.Context(), 1); err != nil {
		test.Fatal(err)
	}
	clock.Advance(5500 * time.Millisecond)
	select {
	case actual := <-executed:
		if actual.Job != work.Job {
			test.Fatal("timer delivered another job")
		}
	case <-test.Context().Done():
		test.Fatal(test.Context().Err())
	}
	select {
	case <-finished:
	case <-test.Context().Done():
		test.Fatal(test.Context().Err())
	}
	cancel()
	<-done
}
