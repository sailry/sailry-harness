package node

import (
	"testing"
	"time"
)

func TestWorkspaceLocks(t *testing.T) {
	// within reports whether the lock was taken before the wait ended.
	within := func(wait time.Duration, lock func() func()) (func(), bool) {
		taken := make(chan func(), 1)
		go func() { taken <- lock() }()
		select {
		case release := <-taken:
			return release, true
		case <-time.After(wait):
			return func() { (<-taken)() }, false
		}
	}
	const blocked, free = 100 * time.Millisecond, 10 * time.Second
	t.Run("one_root_at_a_time_and_other_roots_together", func(t *testing.T) {
		var locks workspaceLocks
		first := locks.effect("/a")
		other, ok := within(free, func() func() { return locks.effect("/b") })
		if !ok {
			t.Fatal("an effect waited for another root")
		}
		other()
		same, ok := within(blocked, func() func() { return locks.effect("/a") })
		if ok {
			t.Fatal("two effects held one root")
		}
		first()
		same()
		if len(locks.held) != 0 {
			t.Fatalf("released roots are kept: %v", locks.held)
		}
	})
	t.Run("a_waiting_effect_does_not_delay_a_registration", func(t *testing.T) {
		var locks workspaceLocks
		// A transfer's local part holds the root without joining the effects.
		transfer := locks.roots("/a")
		waiting, ok := within(blocked, func() func() { return locks.effect("/a") })
		if ok {
			t.Fatal("an effect ran in a held root")
		}
		registered, ok := within(free, locks.registration)
		if !ok {
			t.Fatal("a registration waited behind an effect that had not started")
		}
		registered()
		other, ok := within(free, func() func() { return locks.effect("/b") })
		if !ok {
			t.Fatal("another root waited behind a pending effect")
		}
		other()
		transfer()
		waiting()
		if locks.registrations.Load() != 1 {
			t.Fatal("a recorded registration was not counted")
		}
	})
	t.Run("a_registration_excludes_running_effects", func(t *testing.T) {
		var locks workspaceLocks
		running := locks.effect("/a")
		registered, ok := within(blocked, locks.registration)
		if ok {
			t.Fatal("a registration ran beside an effect")
		}
		running()
		registered()
	})
	t.Run("effects_across_two_roots_do_not_deadlock", func(t *testing.T) {
		var locks workspaceLocks
		done := make(chan struct{}, 2)
		for _, order := range [][]string{{"/a", "/b"}, {"/b", "/a"}} {
			go func() {
				for range 200 {
					locks.effect(order...)()
				}
				done <- struct{}{}
			}()
		}
		for range 2 {
			select {
			case <-done:
			case <-time.After(free):
				t.Fatal("mirrored effects deadlocked")
			}
		}
		// The same root twice is one lock.
		locks.effect("/a", "/a")()
	})
	t.Run("a_worktree_change_holds_its_roots_and_project_commands_only", func(t *testing.T) {
		var locks workspaceLocks
		// Git creating or removing a worktree may run for a long time.
		change := locks.change("/a")
		other, ok := within(free, func() func() { return locks.effect("/b") })
		if !ok {
			t.Fatal("a worktree change stalled another root")
		}
		other()
		same, ok := within(blocked, func() func() { return locks.effect("/a") })
		if ok {
			t.Fatal("an effect ran in a root that was being changed")
		}
		project, ok := within(blocked, locks.registration)
		if ok {
			t.Fatal("a project command ran between Git's work and the row")
		}
		change()
		// The project command took its turn first; the effect that waited
		// then sees that its root may be gone.
		project()
		same()
		if locks.registrations.Load() != 2 {
			t.Fatalf("registrations = %d, want the change and the project command", locks.registrations.Load())
		}
	})
	t.Run("commands_without_a_root_run_together", func(t *testing.T) {
		var locks workspaceLocks
		// A web fetch holds no worktree; its cancellation must not wait for it.
		fetch := locks.effect("", "")
		cancel, ok := within(free, func() func() { return locks.effect("", "") })
		if !ok {
			t.Fatal("a command without a root waited for another")
		}
		cancel()
		mixed := locks.effect("", "/a")
		same, ok := within(blocked, func() func() { return locks.effect("/a") })
		if ok {
			t.Fatal("an empty root dropped the root beside it")
		}
		mixed()
		same()
		fetch()
		if len(locks.held) != 0 {
			t.Fatalf("released roots are kept: %v", locks.held)
		}
	})
}
