package dispatch

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchSettlement(test *testing.T) {
	test.Parallel()
	test.Run("a failed end write keeps completed effects without replay", func(test *testing.T) {
		synctest.Test(test, func(test *testing.T) {
			clock := clockwork.NewFakeClock()
			ctx, cancel := context.WithCancel(test.Context())
			defer cancel()
			first, second := protocol.NewJobID(), protocol.NewJobID()
			callbackFailure := errors.New("fixture callback failure")
			results := map[protocol.JobID]error{first: nil, second: callbackFailure}
			var effects, writes, reports atomic.Int32
			var mu sync.Mutex
			settled := map[protocol.JobID]bool{}
			var pending protocol.JobID
			claimed := false
			runner := Runner{
				Clock: clock,
				Tick: func(context.Context, bool) (Batch, error) {
					if claimed {
						return Batch{}, nil
					}
					claimed = true
					return Batch{Work: []Work{{Job: first}, {Job: second}}}, nil
				},
				Execute: func(_ context.Context, work Work) error {
					effects.Add(1)
					return results[work.Job]
				},
				Finished: func(_ context.Context, job protocol.JobID, result error) error {
					if result != results[job] {
						test.Error("settlement changed the completed callback result")
					}
					attempt := writes.Add(1)
					if attempt == 1 {
						pending = job
					}
					if attempt <= 3 && job != pending {
						test.Error("another completion replaced the unsettled owner")
					}
					if attempt <= 2 {
						return errors.New("fixture storage failure")
					}
					mu.Lock()
					settled[job] = true
					mu.Unlock()
					return nil
				},
				Report: func(error) { reports.Add(1) },
			}
			done := make(chan struct{})
			go func() { defer close(done); runner.Serve(ctx) }()
			synctest.Wait()
			if effects.Load() != 2 || writes.Load() != 1 || reports.Load() != 1 {
				test.Errorf("failed settlement was discarded: effects %d, writes %d, reports %d", effects.Load(), writes.Load(), reports.Load())
			}
			clock.Advance(time.Second - time.Nanosecond)
			synctest.Wait()
			if writes.Load() != 1 {
				test.Error("settlement bypassed the tick's existing retry wait")
			}
			clock.Advance(time.Nanosecond)
			synctest.Wait()
			if effects.Load() != 2 || writes.Load() != 2 || reports.Load() != 2 {
				test.Errorf("the failed owner was not retained for another retry: effects %d, writes %d, reports %d", effects.Load(), writes.Load(), reports.Load())
			}
			clock.Advance(time.Second)
			synctest.Wait()
			mu.Lock()
			complete := settled[first] && settled[second]
			mu.Unlock()
			if !complete || effects.Load() != 2 || writes.Load() != 4 {
				test.Errorf("completed callbacks did not settle exactly once after storage recovered: complete %v, effects %d, writes %d", complete, effects.Load(), writes.Load())
			}
			cancel()
			<-done
		})
	})
	test.Run("shutdown releases owners waiting for settlement", func(test *testing.T) {
		synctest.Test(test, func(test *testing.T) {
			ctx, cancel := context.WithCancel(test.Context())
			defer cancel()
			var effects, writes atomic.Int32
			claimed := false
			runner := Runner{
				Clock: clockwork.NewFakeClock(),
				Tick: func(context.Context, bool) (Batch, error) {
					if claimed {
						return Batch{}, nil
					}
					claimed = true
					return Batch{Work: []Work{{Job: protocol.NewJobID()}, {Job: protocol.NewJobID()}, {Job: protocol.NewJobID()}}}, nil
				},
				Execute: func(context.Context, Work) error { effects.Add(1); return nil },
				Finished: func(context.Context, protocol.JobID, error) error {
					writes.Add(1)
					return errors.New("fixture storage failure")
				},
			}
			done := make(chan struct{})
			go func() { defer close(done); runner.Serve(ctx) }()
			synctest.Wait()
			if effects.Load() != 3 || writes.Load() != 1 {
				test.Errorf("fixture did not hold its completed owners: effects %d, writes %d", effects.Load(), writes.Load())
			}
			cancel()
			synctest.Wait()
			<-done
			if effects.Load() != 3 || writes.Load() != 1 {
				test.Fatal("shutdown replayed effects or wrote another completion")
			}
		})
	})
}

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

// sleepingClock is a wall clock that can run ahead of its timers, as the
// system's does across sleep: Slept moves Now without firing any timer.
type sleepingClock struct {
	*clockwork.FakeClock
	mu    sync.Mutex
	slept time.Duration
}

func (clock *sleepingClock) Now() time.Time {
	clock.mu.Lock()
	defer clock.mu.Unlock()
	return clock.FakeClock.Now().Add(clock.slept)
}

func (clock *sleepingClock) Slept(duration time.Duration) {
	clock.mu.Lock()
	clock.slept += duration
	clock.mu.Unlock()
}

func TestDispatchWallClock(test *testing.T) {
	test.Parallel()
	for _, scenario := range []struct {
		name string
		// due is the occurrence's distance at arming; slept is the wall time
		// that passes without the timer; awake is timer time after the wake.
		due, slept, awake time.Duration
		ran               bool
	}{
		{"fire_missed_during_sleep_runs_within_the_recheck_after_wake", 10 * time.Hour, 11 * time.Hour, Recheck, true},
		{"fire_still_ahead_after_sleep_waits_for_its_time", 10 * time.Hour, 5 * time.Hour, Recheck, false},
		{"wall_clock_set_forward_past_the_due_time_runs_it", time.Hour, 2 * time.Hour, Recheck, true},
		{"wall_clock_set_back_does_not_run_it_early", time.Minute, -time.Hour, 2 * time.Minute, false},
	} {
		test.Run(scenario.name, func(test *testing.T) {
			test.Parallel()
			clock := &sleepingClock{FakeClock: clockwork.NewFakeClockAt(time.UnixMilli(0))}
			ctx, cancel := context.WithCancel(test.Context())
			defer cancel()
			// Real time bounds only a failing run; a passing one never waits.
			waiting, release := context.WithTimeout(test.Context(), 10*time.Second)
			defer release()
			next := scenario.due.Milliseconds()
			ticks := make(chan int64, 64)
			executed := make(chan struct{}, 1)
			var claimed atomic.Bool
			runner := Runner{
				Clock: clock, Wake: make(chan struct{}),
				Tick: func(context.Context, bool) (Batch, error) {
					now := clock.Now().UnixMilli()
					ticks <- now
					if now < next {
						return Batch{NextMs: &next}, nil
					}
					if claimed.Swap(true) {
						return Batch{}, nil
					}
					return Batch{Work: []Work{{Job: protocol.NewJobID(), Completion: protocol.DispatchCompletionCommand}}}, nil
				},
				Execute:  func(context.Context, Work) error { executed <- struct{}{}; return nil },
				Finished: func(context.Context, protocol.JobID, error) error { return nil },
			}
			done := make(chan struct{})
			go func() {
				defer close(done)
				runner.Serve(ctx)
			}()
			<-ticks
			if err := clock.BlockUntilContext(waiting, 1); err != nil {
				test.Fatal(err)
			}
			clock.Slept(scenario.slept)
			// Advance in steps of the recheck: each step ends one armed wait.
			for elapsed := time.Duration(0); elapsed < scenario.awake; elapsed += Recheck {
				clock.Advance(Recheck)
				select {
				case <-ticks:
				case <-waiting.Done():
					test.Fatal("the runner did not read the wall clock again within the recheck")
				}
				if !scenario.ran || elapsed+Recheck < scenario.awake {
					if err := clock.BlockUntilContext(waiting, 1); err != nil {
						test.Fatal(err)
					}
				}
			}
			if scenario.ran {
				select {
				case <-executed:
				case <-waiting.Done():
					test.Fatal("an occurrence due after the wake did not run")
				}
			}
			cancel()
			<-done
			select {
			case <-executed:
				if !scenario.ran {
					test.Fatal("an occurrence ran before its wall-clock time")
				}
			default:
			}
		})
	}
}
