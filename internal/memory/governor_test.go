package memory

import (
	"slices"
	"sync/atomic"
	"testing"
	"time"
)

func TestGovernorRegisterUnregisterAndReentrantTrim(t *testing.T) {
	t.Parallel()
	g := NewGovernor()

	var calls atomic.Int32
	var unregisterFirst func()
	unregisterFirst = g.Register("first", func(level Level) {
		if level != Idle {
			t.Errorf("level = %v, want Idle", level)
		}
		calls.Add(1)
		unregisterFirst()
		// Reentrant idempotency check while Trim runs.
		unregisterFirst()
	})
	unregisterSecond := g.Register("second", func(level Level) {
		calls.Add(10)
	})

	g.Trim(Idle)
	if calls.Load() != 11 {
		t.Fatalf("first Trim calls = %d, want 11", calls.Load())
	}
	if snap := g.Snapshot(); snap.Registered != 1 {
		t.Fatalf("registered = %d, want 1", snap.Registered)
	}

	g.Trim(Idle)
	if calls.Load() != 21 {
		t.Fatalf("second Trim calls = %d, want 21", calls.Load())
	}
	unregisterSecond()
	unregisterSecond()
	if snap := g.Snapshot(); snap.Registered != 0 {
		t.Fatalf("registered after unregister = %d, want 0", snap.Registered)
	}
}

func TestGovernorStartStopRefcounting(t *testing.T) {
	t.Parallel()
	g := NewGovernor()
	g.interval = 5 * time.Millisecond

	var idleCount atomic.Int32
	unreg := g.Register("counter", func(level Level) {
		if level == Idle {
			idleCount.Add(1)
		}
	})
	defer unreg()

	stop1 := g.Start()
	stop2 := g.Start()

	deadline := time.Now().Add(time.Second)
	for idleCount.Load() == 0 && time.Now().Before(deadline) {
		time.Sleep(5 * time.Millisecond)
	}
	if idleCount.Load() == 0 {
		t.Fatal("background sweep did not fire Idle trim")
	}

	stop1()
	stop1()
	g.mu.Lock()
	refsAfterFirst := g.refs
	running := g.doneCh != nil
	g.mu.Unlock()
	if refsAfterFirst != 1 || !running {
		t.Fatalf("after first stop: refs=%d running=%v", refsAfterFirst, running)
	}

	stop2()
	stop2()
	g.mu.Lock()
	refsAfterSecond := g.refs
	stillRunning := g.doneCh != nil
	g.mu.Unlock()
	if refsAfterSecond != 0 || stillRunning {
		t.Fatalf("after last stop: refs=%d running=%v", refsAfterSecond, stillRunning)
	}
}

func TestGovernorPressureAndCooldown(t *testing.T) {
	t.Parallel()
	g := NewGovernor()
	base := time.Date(2026, 11, 26, 12, 0, 0, 0, time.UTC)

	var levels []Level
	unreg := g.Register("pressure", func(level Level) {
		levels = append(levels, level)
	})
	defer unreg()

	for _, sample := range []struct {
		name       string
		heap, live uint64 // MiB
		elapsed    time.Duration
		pressure   bool
	}{
		{"garbage before collection", 900, 300, 0, false},
		{"retained heap crosses threshold", 520, 420, time.Second, true},
		{"cooldown", 650, 590, 2 * time.Second, false},
		{"sustained growth", 620, 590, time.Minute, true},
		{"ordinary allocations", 1100, 590, 2 * time.Minute, false},
		{"unchanged after collection", 591, 590, 3 * time.Minute, false},
		{"released memory", 440, 400, 4 * time.Minute, false},
		{"renewed growth", 560, 530, 5 * time.Minute, true},
	} {
		if got := g.checkPressure(sample.heap<<20, sample.live<<20, base.Add(sample.elapsed)); got != sample.pressure {
			t.Fatalf("%s: pressure = %v, want %v", sample.name, got, sample.pressure)
		}
	}
	if !slices.Equal(levels, []Level{Pressure, Pressure, Pressure}) {
		t.Fatalf("unexpected callbacks: %v", levels)
	}
	snap := g.Snapshot()
	if snap.Registered != 1 || snap.LastLevel != Pressure || snap.LastHeapBytes != 560<<20 || snap.LastLiveBytes != 530<<20 {
		t.Fatalf("unexpected snapshot: %+v", snap)
	}
}
