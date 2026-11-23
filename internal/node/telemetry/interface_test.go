package telemetry

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
)

func TestNativeProbeAnnotatesEveryHangOutcome(t *testing.T) {
	for _, tc := range []struct {
		status, heartbeat string
		replied, measured bool
	}{
		{"replied", "pending", true, true},
		{"replied", "recovered", true, true},
		{"timeout", "pending", false, true},
		{"unavailable", "pending", false, false},
	} {
		t.Run(tc.status+"/"+tc.heartbeat, func(t *testing.T) {
			dir := t.TempDir()
			w := NewWatchdog(WatchdogOptions{Directory: dir, ProbeNativeWindow: func() (bool, bool) { return tc.replied, tc.measured }})
			w.waiting = tc.heartbeat == "pending"
			w.hang()
			raw, err := os.ReadFile(filepath.Join(dir, HangFile))
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.HasPrefix(raw, []byte("native_window."+tc.status+"\nheartbeat."+tc.heartbeat+"\ngoroutine ")) {
				t.Fatal("missing native dispatch outcome or goroutines")
			}
			assertReduced(t, raw)
		})
	}
}

func TestWatchdogPauseSuppressesModalHangs(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	dir, clock := t.TempDir(), clockwork.NewFakeClock()
	var blocked atomic.Bool
	blocked.Store(true)
	held := make(chan func(), 64)
	w := NewWatchdog(WatchdogOptions{
		Directory: dir,
		Clock:     clock,
		Ping: func(done func()) {
			if blocked.Load() {
				held <- done
				return
			}
			done()
		},
	})
	defer func() {
		blocked.Store(false)
		closeHeld := func() {
			for {
				select {
				case done := <-held:
					done()
				default:
					return
				}
			}
		}
		closeHeld()
		w.Close()
		closeHeld()
	}()
	tick := func(step time.Duration) {
		t.Helper()
		if err := clock.BlockUntilContext(ctx, 1); err != nil {
			t.Fatal("the watchdog stopped ticking")
		}
		clock.Advance(step)
		if err := clock.BlockUntilContext(ctx, 1); err != nil {
			t.Fatal("the watchdog stopped ticking")
		}
	}
	traces := func() int {
		if _, err := os.Stat(filepath.Join(dir, HangFile)); err != nil {
			return 0
		}
		return 1
	}

	w.Visible(true)
	release1 := w.Pause()
	release2 := w.Pause()
	w.Touch()

	for range 15 {
		tick(watchInterval)
	}
	if traces() != 0 {
		t.Fatal("paused watchdog reported a modal hang")
	}
	// Duplicate releases and partial unnesting must keep pause active.
	release1()
	release1()
	for range int(HangAfter/watchInterval) + 2 {
		tick(watchInterval)
	}
	if traces() != 0 {
		t.Fatal("duplicate release or nested pause allowed a hang report")
	}

	release2()
	for range int(HangAfter/watchInterval) - 1 {
		tick(watchInterval)
	}
	if traces() != 0 {
		t.Fatalf("reported before %v after final resume", HangAfter)
	}
	tick(watchInterval)
	if traces() != 1 {
		t.Fatalf("did not report hang %v after modal resume", HangAfter)
	}
}

func TestWatchdogExtendAllowsBoundedWarmupAndStillDetectsDeadlock(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	dir, clock := t.TempDir(), clockwork.NewFakeClock()
	var blocked atomic.Bool
	held := make(chan func(), 64)
	w := NewWatchdog(WatchdogOptions{
		Directory: dir,
		Clock:     clock,
		Ping: func(done func()) {
			if blocked.Load() {
				held <- done
				return
			}
			done()
		},
	})
	settle := func() {
		for {
			select {
			case done := <-held:
				done()
				continue
			default:
			}
			w.mu.Lock()
			waiting := w.waiting
			w.mu.Unlock()
			if !waiting {
				return
			}
			time.Sleep(50 * time.Microsecond)
		}
	}
	defer func() {
		blocked.Store(false)
		settle()
		w.Close()
	}()
	tick := func(step time.Duration) {
		t.Helper()
		if err := clock.BlockUntilContext(ctx, 1); err != nil {
			t.Fatal("the watchdog stopped ticking")
		}
		clock.Advance(step)
		if err := clock.BlockUntilContext(ctx, 1); err != nil {
			t.Fatal("the watchdog stopped ticking")
		}
		if !blocked.Load() {
			settle()
		}
	}
	traces := func() int {
		if _, err := os.Stat(filepath.Join(dir, HangFile)); err != nil {
			return 0
		}
		return 1
	}

	w.Visible(true)
	blocked.Store(true)
	end := w.Extend(15 * time.Second)
	w.Touch()
	for range 10 {
		tick(watchInterval)
	}
	if traces() != 0 {
		t.Fatal("grace period reported a warmup hang too early")
	}
	end()
	end()
	blocked.Store(false)
	settle()
	if traces() != 0 {
		t.Fatal("recovered warmup produced a hang trace")
	}

	// A genuine deadlock exceeding the bounded grace must still report.
	blocked.Store(true)
	_ = w.Extend(15 * time.Second)
	for range 15 {
		tick(watchInterval)
	}
	if traces() != 1 {
		t.Fatal("deadlock past grace window did not report a hang")
	}
}
