//go:build !sailry_no_code_index

package codeindex

import (
	"context"
	"maps"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Count delivered source bytes, not candidate opens. A miss isolates scan I/O;
// the extra body on a hit belongs to result validation, even on a warm scan.
func TestScanContentReads(t *testing.T) {
	t.Parallel()
	for _, fallback := range []bool{false, true} {
		name := "native stamps"
		if fallback {
			name = "unavailable stamps"
		}
		t.Run(name, func(t *testing.T) {
			f := newFixture(t)
			all := make(map[string]int)
			for _, path := range []string{"one", "two", "three"} {
				source := "pub fn " + path + "() {}\n"
				f.write(path+".rs", source)
				all[path+".rs"] = len(source)
			}
			var mu sync.Mutex
			read, unsupported := map[string]int{}, map[string]int{}
			f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, previous core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
				if fallback {
					previous = core.FileStamp{}
				}
				stamp, err := files.InspectText(ctx, path, limit, previous, func(source *string) error {
					mu.Lock()
					read[path] += len(*source)
					mu.Unlock()
					return inspect(source)
				})
				if fallback {
					stamp = core.FileStamp{}
				}
				if stamp == (core.FileStamp{}) {
					mu.Lock()
					unsupported[path] = all[path]
					mu.Unlock()
				}
				return stamp, err
			}
			expect := func(stage string, want map[string]int) {
				t.Helper()
				equal(t, stage, read, want)
				clear(read)
			}
			cold := f.search("absentsymbol")
			expect("cold scan", all)
			hotReads := maps.Clone(unsupported)
			hot := f.search("absentsymbol")
			expect("hot scan", hotReads)
			if cold.Partial || hot.Partial || hot.IndexRevision != cold.IndexRevision {
				t.Fatalf("cold/hot revision or completeness changed: %+v / %+v", cold, hot)
			}
			hit := f.search("two")
			if names2(hit.Symbols) != "two" || hit.Partial || hit.IndexRevision != cold.IndexRevision {
				t.Fatalf("invalid hit: %+v", hit)
			}
			withHit := maps.Clone(hotReads)
			withHit["two.rs"] += all["two.rs"]
			expect("hot scan plus result validation", withHit)
			f.service.Observed(f.id, []string{filepath.Join(f.root, "one.rs")})
			changed := f.graph(graphQuery("map", ""))
			if changed.Partial || changed.IndexRevision != cold.IndexRevision {
				t.Fatalf("notification without a content change advanced revision: %+v", changed)
			}
			withDirty := maps.Clone(hotReads)
			withDirty["one.rs"] = all["one.rs"]
			expect("known change", withDirty)
			f.service.Changed(f.id, nil)
			f.search("absentsymbol")
			expect("root invalidation", all)
			f.service.releaseIdle(time.Now().Add(idleRelease + time.Second))
			if reopened := f.search("absentsymbol"); reopened.IndexRevision != cold.IndexRevision {
				t.Fatalf("reopen advanced revision: %+v", reopened)
			}
			expect("reopened worktree", all)
			w, _ := f.service.worktree(f.id, f.root)
			w.dirty.fail()
			for range 2 {
				if failed := f.search("absentsymbol"); !failed.Partial || failed.IndexRevision != cold.IndexRevision {
					t.Fatalf("unavailable watch: %+v", failed)
				}
				expect("failed watch", all)
			}
		})
	}
}

func TestQuietScanDiscovery(t *testing.T) {
	t.Parallel()
	f := newFixture(t)
	const first = "pub fn before() {}\n"
	const second = "pub fn second() {}\n"
	f.write("one.rs", first, "hidden/two.rs", second, ".gitignore", "hidden/\n")
	baseline := f.search("before")
	if names2(baseline.Symbols) != "before" || baseline.Partial {
		t.Fatalf("baseline: %+v", baseline)
	}
	path := filepath.Join(f.root, "one.rs")
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	f.write("one.rs", second)
	if err := os.Chtimes(path, info.ModTime(), info.ModTime()); err != nil {
		t.Fatal(err)
	}
	changed := f.search("second")
	if changed.Partial || names2(changed.Symbols) != "second" || changed.Symbols[0].FileRevision != Hash(second) || changed.IndexRevision != baseline.IndexRevision+1 {
		t.Fatalf("equal-length write with restored mtime: %+v", changed)
	}
	f.write("replacement", first)
	if err := os.Chtimes(filepath.Join(f.root, "replacement"), info.ModTime(), info.ModTime()); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(filepath.Join(f.root, "replacement"), path); err != nil {
		t.Fatal(err)
	}
	if replaced := f.search("before"); names2(replaced.Symbols) != "before" || replaced.IndexRevision != changed.IndexRevision+1 || replaced.Symbols[0].FileRevision != Hash(first) {
		t.Fatalf("same-name replacement: %+v", replaced)
	}
	f.write("new.rs", "pub fn added() {}\n", ".gitignore", "one.rs\n")
	if err := os.Rename(filepath.Join(f.root, "hidden", "two.rs"), filepath.Join(f.root, "moved.rs")); err != nil {
		t.Fatal(err)
	}
	page := f.search("")
	if page.Partial || page.Truncated || len(page.Symbols) != 2 {
		t.Fatalf("silent add/ignore/rename: %+v", page)
	}
	files := map[string]string{}
	for _, symbol := range page.Symbols {
		files[symbol.Path] = symbol.Name
	}
	equal(t, "discover new matches and prune ignored files", files, map[string]string{"moved.rs": "second", "new.rs": "added"})
	if err := os.Remove(filepath.Join(f.root, "new.rs")); err != nil {
		t.Fatal(err)
	}
	if gone := f.search("added"); gone.Partial || len(gone.Symbols) != 0 || gone.IndexRevision <= page.IndexRevision {
		t.Fatalf("silent delete: %+v", gone)
	}
}

func TestChangeWhileCheckingDoesNotCommitStaleParse(t *testing.T) {
	t.Parallel()
	f := newFixture(t)
	f.write("one.rs", "pub fn before() {}\n")
	initial := f.search("before")
	f.write("one.rs", "pub fn middle() {}\n")
	path := filepath.Join(f.root, "one.rs")
	change := func() error {
		const latest = "pub fn latest() { /* edited */ }\n"
		if runtime.GOOS == "windows" {
			// An open destination cannot be replaced by os.Rename on Windows.
			// Write a different size to detect the change without relying on mtime.
			return os.WriteFile(path, []byte(latest), 0o600)
		}
		replacement := filepath.Join(filepath.Dir(f.root), "next.rs")
		if err := os.WriteFile(replacement, []byte(latest), 0o600); err != nil {
			return err
		}
		return os.Rename(replacement, path)
	}
	started, resume := make(chan struct{}), make(chan struct{})
	var once sync.Once
	f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, stamp core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
		return files.InspectText(ctx, path, limit, stamp, func(source *string) error {
			err := inspect(source)
			once.Do(func() {
				close(started)
				select {
				case <-resume:
				case <-ctx.Done():
				}
			})
			return err
		})
	}
	query := protocol.DefaultCodeIndexSearch()
	query.Query = "middle"
	type response struct {
		page protocol.CodeIndexSearchPage
		err  error
	}
	done := make(chan response, 1)
	go func() {
		page, err := f.service.Search(t.Context(), f.id, f.root, query)
		done <- response{page, err}
	}()
	select {
	case <-started:
	case result := <-done:
		t.Fatalf("query ended before inspecting the changed file: %+v", result)
	}
	graph := f.graph(graphQuery("map", ""))
	err := change()
	close(resume)
	if err != nil {
		t.Fatal(err)
	}
	result := <-done
	if result.err != nil || !result.page.Partial || len(result.page.Symbols) != 0 || result.page.IndexRevision != initial.IndexRevision || !graph.Partial || graph.IndexRevision != initial.IndexRevision {
		t.Fatalf("stale or non-partial view: search=%+v graph=%+v", result, graph)
	}
	latest := f.search("latest")
	if latest.Partial || names2(latest.Symbols) != "latest" || latest.IndexRevision != initial.IndexRevision+1 {
		t.Fatalf("index did not recover after the change: %+v", latest)
	}
}
