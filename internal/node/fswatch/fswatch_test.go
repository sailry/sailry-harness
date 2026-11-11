package fswatch

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

// descriptors counts the open descriptors of this process.
func descriptors(t *testing.T) int {
	t.Helper()
	entries, err := os.ReadDir("/dev/fd")
	if err != nil {
		t.Skip("descriptors cannot be listed on this system")
	}
	return len(entries)
}

// await reads batches until one names path.
func await(t *testing.T, watcher *Watcher, path string) Event {
	t.Helper()
	deadline := time.After(10 * time.Second)
	for {
		select {
		case batch, open := <-watcher.Events:
			if !open {
				t.Fatalf("events closed before %s", path)
			}
			for _, event := range batch {
				if event.Path == path {
					return event
				}
			}
		case err := <-watcher.Errors:
			t.Fatalf("watch failed: %v", err)
		case <-deadline:
			t.Fatalf("no event for %s", path)
		}
	}
}

// quiet fails when path is reported within the settle time.
func quiet(t *testing.T, watcher *Watcher, path string) {
	t.Helper()
	settled := time.After(time.Second)
	for {
		select {
		case batch := <-watcher.Events:
			for _, event := range batch {
				if event.Path == path {
					t.Fatalf("unexpected event for %s", path)
				}
			}
		case <-settled:
			return
		}
	}
}

func write(t *testing.T, path string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(time.Now().String()), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestWatcher(t *testing.T) {
	t.Run("tree", func(t *testing.T) {
		root := t.TempDir()
		nested := filepath.Join(root, "a", "b")
		if err := os.MkdirAll(nested, 0o700); err != nil {
			t.Fatal(err)
		}
		watcher, err := New()
		if err != nil {
			t.Fatal(err)
		}
		defer watcher.Close()
		if err := watcher.Tree(root); err != nil {
			t.Fatal(err)
		}
		existing := filepath.Join(nested, "existing.txt")
		write(t, existing)
		await(t, watcher, existing)

		// A directory created later joins the tree.
		later := filepath.Join(root, "later")
		if err := os.Mkdir(later, 0o700); err != nil {
			t.Fatal(err)
		}
		await(t, watcher, later)
		inside := filepath.Join(later, "inside.txt")
		write(t, inside)
		await(t, watcher, inside)

		if err := os.Remove(existing); err != nil {
			t.Fatal(err)
		}
		await(t, watcher, existing)
	})

	t.Run("directory", func(t *testing.T) {
		root := t.TempDir()
		watched, other := filepath.Join(root, "watched"), filepath.Join(root, "other")
		for _, dir := range []string{filepath.Join(watched, "below"), other} {
			if err := os.MkdirAll(dir, 0o700); err != nil {
				t.Fatal(err)
			}
		}
		watcher, err := New()
		if err != nil {
			t.Fatal(err)
		}
		defer watcher.Close()
		if err := watcher.Dir(root, watched); err != nil {
			t.Fatal(err)
		}
		if err := watcher.Dir(root, watched); err != nil {
			t.Fatal(err)
		}
		// Only the entries of the added directory are reported.
		outside, deeper := filepath.Join(other, "outside.txt"), filepath.Join(watched, "below", "deeper.txt")
		write(t, outside)
		write(t, deeper)
		direct := filepath.Join(watched, "direct.txt")
		write(t, direct)
		await(t, watcher, direct)
		write(t, outside)
		write(t, deeper)
		quiet(t, watcher, outside)

		watcher.Drop(watched)
		write(t, direct)
		quiet(t, watcher, direct)
	})

	// The reason this package exists: fsnotify's kqueue backend held one
	// descriptor per file of every watched directory.
	t.Run("descriptors do not grow with files", func(t *testing.T) {
		if runtime.GOOS == "windows" {
			t.Skip("handles are not listed on Windows")
		}
		root := t.TempDir()
		const directories, files = 20, 100
		for d := range directories {
			dir := filepath.Join(root, fmt.Sprintf("d%02d", d))
			if err := os.Mkdir(dir, 0o700); err != nil {
				t.Fatal(err)
			}
			for f := range files {
				write(t, filepath.Join(dir, fmt.Sprintf("f%03d", f)))
			}
		}
		before := descriptors(t)
		watcher, err := New()
		if err != nil {
			t.Fatal(err)
		}
		if err := watcher.Tree(root); err != nil {
			t.Fatal(err)
		}
		probe := filepath.Join(root, "d00", "probe")
		write(t, probe)
		await(t, watcher, probe)
		// A stream or an inotify instance costs a few descriptors, never
		// one per file or directory.
		if held := descriptors(t) - before; held > 16 {
			t.Fatalf("watching %d files held %d descriptors", directories*files, held)
		}
		if err := watcher.Close(); err != nil {
			t.Fatal(err)
		}
		if _, open := <-watcher.Events; open {
			// Drain what was pending at Close.
			for range watcher.Events {
			}
		}
		if err := watcher.Close(); err != nil {
			t.Fatal(err)
		}
		if after := descriptors(t); after > before+4 {
			t.Fatalf("close left %d descriptors, started with %d", after, before)
		}
	})

	t.Run("many roots", func(t *testing.T) {
		watcher, err := New()
		if err != nil {
			t.Fatal(err)
		}
		defer watcher.Close()
		// The workspace watcher allows 32 roots with up to two Git
		// directories each and knowledge 128 collections.
		var last string
		for range 256 {
			last = t.TempDir()
			if err := watcher.Tree(last); err != nil {
				t.Fatal(err)
			}
		}
		probe := filepath.Join(last, "probe")
		write(t, probe)
		await(t, watcher, probe)
	})
}
