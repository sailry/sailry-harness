//go:build !sailry_no_code_index

package codeindex

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func cacheFixture(t *testing.T, dir string, modified time.Time) {
	t.Helper()
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"index.turso", "index.db-log"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("cache"), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.Chtimes(path, modified, modified); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.Chtimes(dir, modified, modified); err != nil {
		t.Fatal(err)
	}
}

func TestCodeIndexCachePruning(t *testing.T) {
	f := newFixture(t)
	// The startup sweep uses the wall clock. Test retention at a later time.
	now := time.Now().Add(3 * 24 * time.Hour)
	old := now.Add(-cacheMaxAge - time.Hour)
	recent := time.Now().Add(-time.Hour)
	stale := f.service.directory(protocol.WorktreeID{1})
	used := f.service.directory(protocol.WorktreeID{2})
	foreign := f.service.directory(protocol.WorktreeID{3})
	tiedFirst := f.service.directory(protocol.WorktreeID{4})
	tiedSecond := f.service.directory(protocol.WorktreeID{5})
	activeID := protocol.WorktreeID{6}
	active := f.service.directory(activeID)
	outside := filepath.Join(f.profile, "other-data")
	unknown := filepath.Join(filepath.Dir(active), "other-cache")
	interrupted := f.service.directory(protocol.WorktreeID{7}) + discardedSuffix
	for _, dir := range []string{stale, used, foreign, active, outside, unknown} {
		cacheFixture(t, dir, old)
	}
	for _, dir := range []string{tiedFirst, tiedSecond} {
		cacheFixture(t, dir, recent)
	}
	pending := filepath.Join(f.profile, "pending")
	cacheFixture(t, pending, recent)
	if err := os.Rename(pending, interrupted); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(foreign, "owned-by-something-else"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	// A cache used read-only must rank above the old database page times.
	touchCache(used)
	ctx, _, end, err := f.service.begin(context.Background(), activeID, f.root, true)
	if err != nil {
		t.Fatal(err)
	}
	defer end()
	f.service.prune(now, cacheMaxAge, 30)
	if err := ctx.Err(); err != nil {
		t.Fatal(err)
	}
	for _, dir := range []string{stale, tiedFirst, interrupted} {
		if _, err := os.Stat(dir); !errors.Is(err, os.ErrNotExist) {
			t.Errorf("old cache remains at %s: %v", dir, err)
		}
	}
	for _, dir := range []string{used, foreign, active, tiedSecond, outside, unknown} {
		if _, err := os.Stat(filepath.Join(dir, "index.turso")); err != nil {
			t.Errorf("cache should remain at %s: %v", dir, err)
		}
	}
}

func TestCodeIndexCachePruningLeavesSymlinks(t *testing.T) {
	f := newFixture(t)
	outside := filepath.Join(f.profile, "other-data")
	cacheFixture(t, outside, time.Now().Add(-2*cacheMaxAge))
	linked := f.service.directory(protocol.WorktreeID{1})
	if err := os.MkdirAll(filepath.Dir(linked), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, linked); err != nil {
		t.Skipf("directory symlink unavailable: %v", err)
	}
	f.service.prune(time.Now(), 0, 0)
	if info, err := os.Lstat(linked); err != nil || info.Mode()&os.ModeSymlink == 0 {
		t.Fatalf("symlink changed: %v %v", info, err)
	}
	if _, err := os.Stat(filepath.Join(outside, "index.turso")); err != nil {
		t.Fatal(err)
	}
}

func TestCodeIndexIdleProtectsAnAdmittedQuery(t *testing.T) {
	f := newFixture(t)
	ctx, w, end, err := f.service.begin(context.Background(), f.id, f.root, true)
	if err != nil {
		t.Fatal(err)
	}
	f.service.releaseIdle(time.Now().Add(2 * idleRelease))
	active := ctx.Err() == nil && w.stopped.Err() == nil
	end()
	if !active {
		t.Fatal("idle release cancelled an admitted query")
	}
	f.service.releaseIdle(time.Now().Add(2 * idleRelease))
	if w.stopped.Err() == nil {
		t.Fatal("completed query kept an idle worktree active")
	}
}

func TestCodeIndexLiveQueriesDoNotKeepThePersistentIndexOpen(t *testing.T) {
	f := newFixture(t)
	f.write("source.go", "package example\nfunc Target() {}\n")
	f.search("Target")
	w, err := f.service.worktree(f.id, f.root)
	if err != nil {
		t.Fatal(err)
	}
	f.service.mutex.Lock()
	w.used = time.Now().Add(-2 * idleRelease)
	f.service.mutex.Unlock()
	if _, err := f.service.OutlineOnDemand(t.Context(), f.id, f.root, "source.go"); err != nil {
		t.Fatal(err)
	}
	f.service.releaseIdle(time.Now())
	if w.stopped.Err() == nil {
		t.Fatal("an on-demand query kept the persistent index open")
	}
}
