package node

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/files"
)

func TestWorkspaceLocks(t *testing.T) {
	t.Parallel()
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
	t.Run("prepared_files_and_moves_share_the_root_lock_domain", func(t *testing.T) {
		var locks workspaceLocks
		root := t.TempDir()
		// A mixed patch also owns a path in another worktree.
		other := t.TempDir()
		to := filepath.Join(other, "outside")
		patch := &files.Prepared{Changes: []files.PreparedChange{{Path: "one", Destination: &to}, {Path: "two"}}}
		hold := locks.fileEffect(root, patch)
		sibling, ok := within(free, func() func() {
			return locks.fileEffect(root, &files.Prepared{Changes: []files.PreparedChange{{Path: "one-more"}}})
		})
		if !ok {
			t.Fatal("an independent file waited behind the patch")
		}
		sibling()
		child, ok := within(free, func() func() {
			return locks.fileEffect(root, &files.Prepared{Changes: []files.PreparedChange{{Path: "two/child"}}})
		})
		if !ok {
			t.Fatal("a file waited for an unrelated parent path")
		}
		child()
		var blockedReleases []func()
		for _, name := range []string{filepath.Join(root, "one"), to, other, root} {
			release, ok := within(blocked, func() func() { return locks.effect(name) })
			if ok {
				t.Fatalf("%s ran during an overlapping patch", name)
			}
			blockedReleases = append(blockedReleases, release)
		}
		hold()
		for _, release := range blockedReleases {
			release()
		}
		if len(locks.held) != 0 {
			t.Fatalf("released reservations retained: %d", len(locks.held))
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

func TestWorkspaceFileKeys(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	// Ordering also works for names that do not exist until the patch creates them.
	one := workspaceFileKey(root, "Folder/caf\u00e9")
	if one != workspaceFileKey("", filepath.Join(root, "Folder", "caf\u00e9")) {
		t.Fatal("relative and absolute names split the file queue")
	}
	names := []string{"folder/caf\u00e9", "Folder/Caf\u00e9", "Folder/cafe\u0301", "Folder/other"}
	before := make([]string, len(names))
	for i, name := range names {
		before[i] = workspaceFileKey(root, name)
	}
	if err := os.Mkdir(filepath.Join(root, "Folder"), 0o700); err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(root, "Folder", "caf\u00e9")
	if err := os.WriteFile(file, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	written, err := os.Stat(file)
	if err != nil {
		t.Fatal(err)
	}
	if workspaceFileKey("", file) != one {
		t.Fatal("creating the file changed its queue")
	}
	for i, name := range names {
		found, err := os.Stat(filepath.Join(root, name))
		if err != nil && !os.IsNotExist(err) {
			t.Fatal(err)
		}
		alias := err == nil && os.SameFile(written, found)
		if (before[i] == one) != alias || workspaceFileKey(root, name) != before[i] {
			t.Errorf("%q did not follow the volume's lookup rules before and after creation (alias=%v)", name, alias)
		}
	}
	if runtime.GOOS != "windows" {
		alias := filepath.Join(t.TempDir(), "alias")
		if err := os.Symlink(root, alias); err != nil {
			t.Fatal(err)
		}
		if one != workspaceFileKey(alias, "Folder/caf\u00e9") {
			t.Fatal("parent alias split the file lock")
		}
	}
}
