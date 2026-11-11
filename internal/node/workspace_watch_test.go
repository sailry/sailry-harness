package node

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceWatch(t *testing.T) {
	t.Parallel()
	setup := func(t *testing.T) (*workspaceWatches, string) {
		root, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		watches := newWorkspaceWatches(workspacegit.New(workspacegit.Options{}))
		watches.after = func(time.Duration) <-chan time.Time { return time.After(time.Millisecond) }
		t.Cleanup(func() { _ = watches.close() })
		return watches, root
	}
	next := func(t *testing.T, events link.Subscription) (protocol.Update, error) {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		return events.Next(ctx)
	}
	roots := func(watches *workspaceWatches) int {
		watches.mu.Lock()
		defer watches.mu.Unlock()
		return len(watches.roots)
	}

	t.Run("subscriptions share one watch that ends with the last of them", func(t *testing.T) {
		watches, root := setup(t)
		var mutex sync.Mutex
		var named []string
		first, err := watches.observe(context.Background(), root, protocol.NodeID{}, protocol.WorktreeID{1}, false, func(paths []string) {
			mutex.Lock()
			named = append(named, paths...)
			mutex.Unlock()
		})
		if err != nil {
			t.Fatal(err)
		}
		second, err := watches.subscribe(context.Background(), root, protocol.NodeID{}, protocol.WorktreeID{1}, false)
		if err != nil {
			t.Fatal(err)
		}
		if roots(watches) != 1 {
			t.Fatalf("roots %d", roots(watches))
		}
		watches.mu.Lock()
		active := watches.roots[root]
		watches.mu.Unlock()
		for _, events := range []link.Subscription{first, second} {
			if update, err := next(t, events); err != nil || update.Kind != "files_changed" {
				t.Fatalf("initial %+v %v", update, err)
			}
		}
		// A file in a directory created after the watch opened is reported
		// with its path.
		changed := filepath.Join(root, "later", "file.go")
		if err := os.Mkdir(filepath.Dir(changed), 0o700); err != nil {
			t.Fatal(err)
		}
		if _, err := next(t, first); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(changed, []byte("package later\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		deadline := time.Now().Add(10 * time.Second)
		for {
			if _, err := next(t, first); err != nil {
				t.Fatal(err)
			}
			mutex.Lock()
			found := slices.Contains(named, changed)
			mutex.Unlock()
			if found {
				break
			}
			if time.Now().After(deadline) {
				t.Fatalf("paths %v", named)
			}
		}
		if _, err := next(t, second); err != nil {
			t.Fatal(err)
		}

		if err := first.Close(); err != nil {
			t.Fatal(err)
		}
		select {
		case <-active.done:
			t.Fatal("watch ended while a subscription remained")
		default:
		}
		if err := second.Close(); err != nil {
			t.Fatal(err)
		}
		select {
		case <-active.done:
		default:
			t.Fatal("watch outlived its subscriptions")
		}
		if roots(watches) != 0 {
			t.Fatalf("roots %d", roots(watches))
		}
		if _, open := <-active.native.Events; open {
			t.Fatal("native watch stayed open")
		}
	})

	t.Run("concurrent subscribers of one root open it once", func(t *testing.T) {
		watches, root := setup(t)
		var group sync.WaitGroup
		opened := make([]link.Subscription, 16)
		for index := range opened {
			group.Go(func() {
				events, err := watches.subscribe(context.Background(), root, protocol.NodeID{}, protocol.WorktreeID{1}, index%2 == 0)
				if err != nil {
					t.Error(err)
					return
				}
				opened[index] = events
			})
		}
		group.Wait()
		if t.Failed() {
			return
		}
		if roots(watches) != 1 {
			t.Fatalf("roots %d", roots(watches))
		}
		for _, events := range opened {
			_ = events.Close()
		}
		if roots(watches) != 0 {
			t.Fatalf("roots %d", roots(watches))
		}
	})

	t.Run("a removed root fails its subscriptions and frees its slot", func(t *testing.T) {
		watches, root := setup(t)
		events, err := watches.subscribe(context.Background(), root, protocol.NodeID{}, protocol.WorktreeID{1}, false)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := next(t, events); err != nil {
			t.Fatal(err)
		}
		watches.remove(root)
		if _, err := next(t, events); err == nil {
			t.Fatal("subscription survived its root")
		}
		_ = events.Close()
		if roots(watches) != 0 {
			t.Fatalf("roots %d", roots(watches))
		}
		// A missing root is refused without keeping a slot.
		if _, err := watches.subscribe(context.Background(), filepath.Join(root, "missing"), protocol.NodeID{}, protocol.WorktreeID{2}, false); err == nil {
			t.Fatal("missing root was watched")
		}
		if roots(watches) != 0 {
			t.Fatalf("roots %d", roots(watches))
		}
	})
}
