package service

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/knowledge/source"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
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

	t.Run("a source that cannot be read is recorded and the pass continues", func(t *testing.T) {
		fixture := newHarness(t)
		root := t.TempDir()
		write(t, filepath.Join(root, "a.md"), "alpha notes\n")
		write(t, filepath.Join(root, "broken.png"), "not an image")
		write(t, filepath.Join(root, "c.md"), "gamma notes\n")
		write(t, filepath.Join(root, "locked", "inside.md"), "inside notes\n")
		fixture.selectEmbedding(false)
		// The reader fails the way the Node's does for an undecodable image.
		recognized, corrupt := 0, true
		fixture.service.options.OCR = &source.OCR{
			Signature: func() ([32]byte, error) { return [32]byte{1}, nil },
			Parts: func(_ context.Context, name string, _ []byte) ([]office.Part, error) {
				recognized++
				if corrupt {
					return nil, newFault(protocol.ErrorCodeUnavailable, "OCR failed: image: unknown format")
				}
				return []office.Part{{Name: name, Text: "recognized text", Position: protocol.KnowledgePosition{Kind: "page", Data: protocol.KnowledgePositionPagePayload{Page: 1}}}}, nil
			},
		}
		fixture.create("Docs", directory(root))
		progress := func() protocol.KnowledgeProgress {
			t.Helper()
			fixture.sweep()
			return fixture.collections()[0].Progress
		}
		broken := filepath.Join(root, "broken.png")
		first := progress()
		if first.Status != protocol.KnowledgeStatusReady || first.Sources != 3 || first.Completed != 3 || first.Failed != 1 ||
			!slices.Equal(first.Failures, []protocol.KnowledgeFailure{{Path: broken, Reason: "OCR failed: image: unknown format"}}) {
			t.Fatalf("first pass = %+v", first)
		}
		// The files after the failed one are indexed.
		if found := names(fixture.query("gamma", nil).Hits); !slices.Equal(found, []string{"c.md"}) {
			t.Fatalf("later file = %v", found)
		}
		if !fixture.service.retry {
			t.Fatal("no retry was scheduled for the failed source")
		}

		// It is read again on the next passes, at most three times while
		// its metadata is unchanged, and stays reported.
		for pass := 2; pass <= 5; pass++ {
			if again := progress(); again.Failed != 1 || again.Status != protocol.KnowledgeStatusReady {
				t.Fatalf("pass %d = %+v", pass, again)
			}
			if want := min(pass, attempts); recognized != want {
				t.Fatalf("pass %d read the source %d times, want %d", pass, recognized, want)
			}
			if want := pass < attempts; fixture.service.retry != want {
				t.Fatalf("pass %d retry = %v", pass, fixture.service.retry)
			}
		}

		// A directory that cannot be entered keeps what was published
		// from it and is reported until it can be read.
		locked := filepath.Join(root, "locked")
		if err := os.Chmod(locked, 0); err != nil {
			t.Fatal(err)
		}
		restored := false
		restore := func() {
			if !restored {
				restored = true
				_ = os.Chmod(locked, 0o755)
			}
		}
		t.Cleanup(restore)
		if _, err := os.ReadDir(locked); err == nil {
			t.Log("this user reads directories without permission; the directory case is not covered")
		} else {
			denied := progress()
			if denied.Status != protocol.KnowledgeStatusReady || denied.Failed != 2 || denied.Sources != 3 ||
				!slices.ContainsFunc(denied.Failures, func(failure protocol.KnowledgeFailure) bool {
					return failure.Path == locked && failure.Reason == "knowledge source cannot be read"
				}) {
				t.Fatalf("unreadable directory = %+v", denied)
			}
			if found := names(fixture.query("inside", nil).Hits); !slices.Equal(found, []string{"inside.md"}) {
				t.Fatalf("published source below the directory = %v", found)
			}
		}
		restore()

		// Invalidate, as a changed model set does, forgets the attempts.
		corrupt = false
		if stale := progress(); stale.Failed != 1 {
			t.Fatalf("exhausted source was read again: %+v", stale)
		}
		fixture.service.Invalidate()
		healed := progress()
		if healed.Failed != 0 || len(healed.Failures) != 0 || healed.Sources != 4 || fixture.service.retry {
			t.Fatalf("after the source became readable = %+v retry %v", healed, fixture.service.retry)
		}
		if found := names(fixture.query("recognized", nil).Hits); !slices.Equal(found, []string{"broken.png"}) {
			t.Fatalf("recovered source = %v", found)
		}

		// A collection that fails as a whole reports its reason and is
		// retried a bounded number of times.
		if err := os.RemoveAll(root); err != nil {
			t.Fatal(err)
		}
		for pass := 1; pass <= attempts+1; pass++ {
			gone := progress()
			if gone.Status != protocol.KnowledgeStatusPaused || len(gone.Failures) != 1 || gone.Failures[0].Path != "" ||
				!strings.Contains(gone.Failures[0].Reason, "does not exist") {
				t.Fatalf("missing root = %+v", gone)
			}
			if want := pass < attempts; fixture.service.retry != want {
				t.Fatalf("missing root pass %d retry = %v", pass, fixture.service.retry)
			}
		}
	})
}
