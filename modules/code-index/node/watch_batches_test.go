//go:build !sailry_no_code_index

package codeindex

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Each request acknowledges that the preceding successful wake was consumed.
// This lets the test control the same callback-before-wake order as the bridge.
type pacedWatch struct {
	requests chan chan error
	closed   chan struct{}
}

func (p *pacedWatch) Next(ctx context.Context) (protocol.Update, error) {
	reply := make(chan error, 1)
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case p.requests <- reply:
	}
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case err := <-reply:
		return protocol.Update{}, err
	}
}

func (p *pacedWatch) Close() error { close(p.closed); return nil }

func TestIndexWatchCoalescedInvalidations(t *testing.T) {
	f := newFixture(t)
	p := &pacedWatch{requests: make(chan chan error), closed: make(chan struct{})}
	f.service.watch = func(context.Context, string, protocol.WorktreeID) (link.Subscription, error) { return p, nil }
	w, err := f.service.worktree(f.id, f.root)
	if err != nil {
		t.Fatal(err)
	}
	w.observed(f.service, f.id)
	reply := <-p.requests
	initial := w.dirty.generation()
	f.service.Observed(f.id, []string{filepath.Join(f.root, "first.rs")})
	checkWake := func(full bool, previous uint64) uint64 {
		t.Helper()
		generation := w.dirty.generation()
		if generation <= previous {
			t.Fatal("batch did not invalidate the directory walk")
		}
		reply <- nil
		reply = <-p.requests
		w.dirty.mutex.Lock()
		defer w.dirty.mutex.Unlock()
		if w.dirty.full != full || w.dirty.revision != generation {
			t.Fatalf("wake changed recorded invalidation: full=%t revision=%d, want %t/%d", w.dirty.full, w.dirty.revision, full, generation)
		}
		return generation
	}
	generation := checkWake(false, initial)
	for _, name := range []string{"old.rs", "new.rs"} {
		f.service.Observed(f.id, []string{filepath.Join(f.root, name)})
	}
	generation = checkWake(false, generation)
	_, changed := w.dirty.snapshot()
	for _, path := range []string{"first.rs", "old.rs", "new.rs"} {
		if changed[path] == 0 {
			t.Fatalf("coalesced wake lost %s: %v", path, changed)
		}
	}
	f.service.Observed(f.id, []string{filepath.Join(f.root, "sub", ".gitignore")})
	generation = checkWake(false, generation)
	f.service.Observed(f.id, nil)
	f.service.Observed(f.id, []string{filepath.Join(f.root, "last.rs")})
	checkWake(true, generation)
	// A successful full scan acknowledges the broad invalidation. The Darwin
	// dropped-event marker (the root itself) must independently restore it.
	if page := f.search("absentsymbol"); page.Partial || w.dirty.full {
		t.Fatalf("full invalidation not acknowledged after scan: %+v", page)
	}
	generation = w.dirty.generation()
	f.service.Observed(f.id, []string{filepath.Join(f.root, "last.rs"), f.root})
	checkWake(true, generation)
	reply <- errors.New("lost watch")
	<-p.closed
	if !w.dirty.watchFailed {
		t.Fatal("subscription failure did not invalidate the watch")
	}
}
