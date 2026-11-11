package service

import (
	"path/filepath"
	"testing"
	"time"
)

// TestWorker covers what a pass does around its sources: watching them and
// surviving the ones it cannot read.
func TestWorker(t *testing.T) {
	t.Run("watches request a pass for indexed entries only", func(t *testing.T) {
		fixture := newHarness(t)
		root := t.TempDir()
		write(t, filepath.Join(root, "notes", "first.md"), "first note\n")
		write(t, filepath.Join(root, "build", "output.md"), "generated\n")
		write(t, filepath.Join(root, ".gitignore"), "build/\n")
		fixture.selectEmbedding(false)
		fixture.create("Docs", directory(root))
		requested := make(chan struct{}, 1)
		watches := newWatches(func() {
			select {
			case requested <- struct{}{}:
			default:
			}
		})
		defer watches.close()
		// The first pass registers the scanned directories and asks for a
		// recheck; the second finds the topology settled.
		for range 2 {
			if err := fixture.service.sweep(t.Context(), watches); err != nil {
				t.Fatal(err)
			}
		}
		if watches.needsPoll() {
			t.Fatal("watching fell back to the periodic rescan")
		}
		settle := func() bool {
			select {
			case <-requested:
				return true
			case <-time.After(time.Second):
				return false
			}
		}
		// Changes made while the watches were registered may still arrive.
		for settle() {
		}
		write(t, filepath.Join(root, "build", "more.md"), "generated\n")
		if settle() {
			t.Fatal("an ignored directory requested a pass")
		}
		write(t, filepath.Join(root, "notes", "second.md"), "second note\n")
		select {
		case <-requested:
		case <-time.After(10 * time.Second):
			t.Fatal("an indexed directory did not request a pass")
		}
	})
}
