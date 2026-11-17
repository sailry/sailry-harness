package updater

import (
	"errors"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"
)

func TestUpdaterStartupOperationsSettle(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		service := New(Options{Version: "1.0.0", Directory: t.TempDir()})
		defer service.Close()
		var receipts, checks atomic.Int32
		service.installed = func() (string, error) {
			receipts.Add(1)
			return "", nil
		}
		service.release = func(channel Channel, _ string) (Config, error) {
			checks.Add(1)
			if channel != Stable {
				t.Errorf("initial channel = %v, want Stable", channel)
			}
			return Config{}, failure("updates_source_unready", "fixture source unavailable")
		}
		service.Startup()
		settled := make(chan struct{})
		go func() {
			service.work.Wait()
			close(settled)
		}()
		synctest.Wait()
		select {
		case <-settled:
		default:
			t.Fatal("startup receipt work is blocked by the periodic watcher")
		}
		service.Startup()
		synctest.Wait()
		if receipts.Load() != 1 || checks.Load() != 0 {
			t.Fatalf("startup repeated work: receipts=%d checks=%d", receipts.Load(), checks.Load())
		}
		time.Sleep(watchDelay)
		synctest.Wait()
		if state, failure := service.Snapshot(); checks.Load() != 1 || state.Kind != Idle || failure != nil || len(service.Notices()) != 0 {
			t.Fatalf("background check = %d, %+v, %v", checks.Load(), state, failure)
		}
		service.Close()
		time.Sleep(watchEvery)
		if checks.Load() != 1 {
			t.Fatal("closed watcher started another check")
		}
	})
}

func TestUpdaterCloseRejectsOperations(t *testing.T) {
	for _, test := range []struct {
		name string
		kind Kind
		run  func(*Service)
	}{
		{"check", Idle, (*Service).Check},
		{"quiet check", Available, func(s *Service) { s.check("", true) }},
		{"required version", Ready, func(s *Service) { s.CheckVersion("1.1.0") }},
		{"startup", Idle, (*Service).Startup},
		{"download", Available, (*Service).Download},
		{"install", Ready, (*Service).Install},
		{"restart", Stopped, (*Service).Restart},
		{"channel change", Available, func(s *Service) {
			if s.ChannelChanged() {
				t.Error("closed service requested a channel check")
			}
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			var calls atomic.Int32
			service := New(Options{
				Version: "1.0.0", Directory: t.TempDir(),
				Channel: func() Channel { calls.Add(1); return Stable },
				Stop:    func() error { calls.Add(1); return errors.New("fixture stop") },
			})
			defer service.Close()
			service.state = State{Kind: test.kind}
			service.config = &Config{}
			service.selection = &Selection{}
			service.restart = &restart{executable: "fixture"}
			service.installed = func() (string, error) {
				calls.Add(1)
				return "", failure("updates_bundle_required", "fixture")
			}
			service.release = func(Channel, string) (Config, error) {
				calls.Add(1)
				return Config{}, failure("updates_source_unready", "fixture")
			}
			service.Close()
			before, _ := service.Snapshot()
			test.run(service)
			service.work.Wait()
			after, failure := service.Snapshot()
			if after != before || failure != nil || calls.Load() != 0 || len(service.Notices()) != 0 {
				t.Fatalf("closed service accepted %s: state=%+v failure=%v calls=%d", test.name, after, failure, calls.Load())
			}
		})
	}
}

func TestUpdaterWatchShutdownRace(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var checks atomic.Int32
		service := New(Options{Version: "1.0.0", Directory: t.TempDir()})
		defer service.Close()
		service.installed = func() (string, error) { return "", nil }
		service.release = func(Channel, string) (Config, error) {
			checks.Add(1)
			return Config{}, failure("updates_source_unready", "fixture")
		}
		service.Startup()
		synctest.Wait()
		time.Sleep(watchDelay - time.Nanosecond)
		closed := make(chan struct{})
		go func() {
			time.Sleep(time.Nanosecond)
			service.Close()
			close(closed)
		}()
		time.Sleep(time.Nanosecond)
		synctest.Wait()
		<-closed
		completed := checks.Load()
		service.check("", true)
		time.Sleep(watchEvery)
		synctest.Wait()
		if checks.Load() != completed {
			t.Fatalf("shutdown admitted a late request: before=%d after=%d", completed, checks.Load())
		}
	})
}
