package codeindex

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLiveCodeQueryCoverage(t *testing.T) {
	f := newFixture(t)
	f.chain()
	f.write(".gitignore", "ignored/\n", "ignored/a.rs", "pub fn ignored() { a(); }\n")
	for i := range queryFiles + 3 {
		f.write(fmt.Sprintf("broad/%03d.rs", i), fmt.Sprintf("pub fn ordinary_%03d() {}\n", i))
	}
	ctx := context.Background()

	search := protocol.DefaultCodeIndexSearch()
	search.Path = new("broad")
	found, err := f.service.SearchOnDemand(ctx, f.id, f.root, search)
	if err != nil || !found.Partial || !found.Truncated || len(found.Symbols) != int(search.Limit) || found.IndexRevision != 0 {
		t.Fatalf("unbounded request returned %+v, %v", found, err)
	}
	if warm, err := f.service.SearchOnDemand(ctx, f.id, f.root, search); err != nil || !reflect.DeepEqual(warm, found) {
		t.Fatalf("warm scan changed the budgeted result: %+v, %v", warm, err)
	}
	search.Query = fmt.Sprintf("ordinary_%03d", queryFiles+2)
	search.Name = &search.Query
	found, err = f.service.SearchOnDemand(ctx, f.id, f.root, search)
	if err != nil || found.Partial || found.Truncated || len(found.Symbols) != 1 || found.Symbols[0].Name != search.Query {
		t.Fatalf("selective request returned %+v, %v", found, err)
	}
	search.Path = new("broad/removed.rs")
	found, err = f.service.SearchOnDemand(ctx, f.id, f.root, search)
	if err != nil || found.Partial || len(found.Symbols) != 0 {
		t.Fatalf("missing path returned %+v, %v", found, err)
	}

	callers := graphQuery("callers", "a", "chain/a.rs")
	related, err := f.service.GraphOnDemand(ctx, f.id, f.root, callers)
	if err != nil || related.Partial || len(related.Callers) != 1 || related.Callers[0].Path != "chain/b.rs" {
		t.Fatalf("original callers %+v, %v", related, err)
	}
	f.write("chain/b.rs", "pub fn b() {}\n", "chain/new.rs", "pub fn later() { a(); }\n")
	related, err = f.service.GraphOnDemand(ctx, f.id, f.root, callers)
	if err != nil || related.Partial || len(related.Callers) != 1 || related.Callers[0].Path != "chain/new.rs" {
		t.Fatalf("updated callers %+v, %v", related, err)
	}
	ranked, err := f.service.GraphOnDemand(ctx, f.id, f.root, graphQuery("map", "unused", "chain"))
	if err != nil || !ranked.Partial || ranked.Truncated || len(ranked.Map) != 6 {
		t.Fatalf("scoped map %+v, %v", ranked, err)
	}
	if _, err = os.Stat(f.service.directory(f.id)); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("live queries created a cache directory: %v", err)
	}
}

func TestLiveGraphRevalidatesCachedSelection(t *testing.T) {
	f := newFixture(t)
	f.chain()
	const side = "other/side.rs"
	const before, after = "pub fn side() { b(); }\n", "pub fn side() { a(); }\n"
	f.write(side, before)
	reads, checks, unavailable := map[string]int{}, map[string]int{}, map[string]int{}
	f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, previous core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
		checks[path]++
		stamp, err := files.InspectText(ctx, path, limit, previous, func(source *string) error {
			reads[path]++
			return inspect(source)
		})
		if stamp == (core.FileStamp{}) {
			unavailable[path]++
		}
		return stamp, err
	}
	query := graphQuery("callers", "a", "chain/a.rs")
	run := func(expected ...string) protocol.CodeIndexGraphGraphPage {
		t.Helper()
		page, err := f.service.GraphOnDemand(t.Context(), f.id, f.root, query)
		if err != nil || page.Partial || page.Truncated || page.IndexRevision != 0 || len(page.Callers) != len(expected) {
			t.Fatalf("callers = %v, want %v; partial=%t truncated=%t err=%v", sites(page.Callers), expected, page.Partial, page.Truncated, err)
		}
		for i, path := range expected {
			if page.Callers[i].Path != path {
				t.Fatalf("callers = %v, want %v", sites(page.Callers), expected)
			}
		}
		return page
	}
	cold := run("chain/b.rs")
	if len(checks) != 6 || !reflect.DeepEqual(reads, checks) {
		t.Fatalf("cold query did not inspect all cross-file candidates: checks=%v reads=%v", checks, reads)
	}
	clear(reads)
	clear(checks)
	clear(unavailable)
	if warm := run("chain/b.rs"); !reflect.DeepEqual(cold, warm) {
		t.Fatalf("warm result differs: cold=%+v warm=%+v", cold, warm)
	}
	if len(checks) != 6 || !reflect.DeepEqual(reads, unavailable) {
		t.Fatalf("warm checks or fallback reads: checks=%v reads=%v unavailable=%v", checks, reads, unavailable)
	}
	clear(reads)
	clear(unavailable)
	limit := query.Limit
	query.Limit = 1
	if paged := run("chain/b.rs"); !reflect.DeepEqual(cold, paged) || !reflect.DeepEqual(reads, unavailable) {
		t.Fatalf("another page limit discarded shared selection proofs: result=%+v reads=%v unavailable=%v", paged, reads, unavailable)
	}
	query.Limit = limit

	// Neither discovery nor reuse depends on a working watch.
	w, err := f.service.worktree(f.id, f.root)
	if err != nil {
		t.Fatal(err)
	}
	w.dirty.fail()
	path := filepath.Join(f.root, side)
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	f.write("other/replacement", after)
	replacement := filepath.Join(f.root, "other/replacement")
	if err := os.Chtimes(replacement, info.ModTime(), info.ModTime()); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(replacement, path); err != nil {
		t.Fatal(err)
	}
	clear(reads)
	run("chain/b.rs", side)
	if reads[side] != 1 {
		t.Fatalf("same-length same-mtime replacement was not read: %v", reads)
	}
	if err := os.Rename(path, filepath.Join(f.root, "other/moved.rs")); err != nil {
		t.Fatal(err)
	}
	run("chain/b.rs", "other/moved.rs")
	f.write(".gitignore", "other/\n", "added.rs", after)
	clear(checks)
	run("chain/b.rs", "added.rs")
	if checks["other/moved.rs"] != 0 {
		t.Fatalf("ignored file still inspected: %v", checks)
	}
	if err := os.Remove(filepath.Join(f.root, "added.rs")); err != nil {
		t.Fatal(err)
	}
	run("chain/b.rs")
}

func TestLiveGraphReclassifiesEditedShebang(t *testing.T) {
	f := newFixture(t)
	const body = "def bridge():\n    start_work()\n"
	const script = "#!/usr/bin/env python3\n" + body
	f.write("main.py", "def start_work():\n    pass\n", "runner", script, "ignored.codeindex-unknown", script)
	query := graphQuery("callers", "start_work", "main.py")
	page, err := f.service.GraphOnDemand(t.Context(), f.id, f.root, query)
	if err != nil || page.Partial || page.Truncated || len(page.Callers) != 1 {
		t.Fatalf("initial callers: %+v, %v", page, err)
	}
	caller := page.Callers[0]
	if caller.Path != "runner" || caller.Scope == nil || caller.Scope.Name != "bridge" || caller.Line != 2 || caller.Column != 4 {
		t.Fatalf("unexpected extensionless caller: %+v", caller)
	}
	f.write("runner", body)
	page, err = f.service.GraphOnDemand(t.Context(), f.id, f.root, query)
	if err != nil || page.Partial || page.Truncated || len(page.Targets) != 1 || len(page.Callers) != 0 {
		t.Fatalf("callers after removing the shebang: %+v, %v", page, err)
	}
}

func TestLiveReuseKeepsWorktreesSeparate(t *testing.T) {
	f := newFixture(t)
	f.write("same.rs", "pub fn original() {}\n", ".sailry/worktrees/hidden/source.rs", "pub fn hidden() {}\n")
	var reused bool
	f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, previous core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
		reused = reused || previous != (core.FileStamp{})
		return files.InspectText(ctx, path, limit, previous, inspect)
	}
	out, err := f.service.OutlineOnDemand(t.Context(), f.id, f.root, "same.rs")
	if err != nil || len(out.Symbols) != 1 || out.FileRevision == nil || out.Symbols[0].Signature == nil {
		t.Fatalf("initial outline: %+v %v", out, err)
	}
	// Outline callers must not be able to change the cached derivation.
	*out.FileRevision, *out.Symbols[0].Signature, out.Symbols[0].Name = "invalid", "invalid", "invalid"
	otherID := protocol.WorktreeID{13}
	other, err := f.service.OutlineOnDemand(t.Context(), otherID, f.root, "same.rs")
	if reused || err != nil || len(other.Symbols) != 1 || other.Symbols[0].Name != "original" {
		t.Fatalf("second worktree reused the first proof: reused=%t outline=%+v err=%v", reused, other, err)
	}
	again, err := f.service.OutlineOnDemand(t.Context(), f.id, f.root, "same.rs")
	if err != nil || !reflect.DeepEqual(again, other) {
		t.Fatalf("first worktree cached a mutated outline: got=%+v want=%+v err=%v", again, other, err)
	}
	childRoot := filepath.Join(f.root, ".sailry", "worktrees", "hidden")
	child, err := f.service.OutlineOnDemand(t.Context(), protocol.WorktreeID{14}, childRoot, "source.rs")
	if err != nil || names2(child.Symbols) != "hidden" {
		t.Fatalf("registered child root could not query itself: %+v, %v", child, err)
	}
	if _, err := f.service.OutlineOnDemand(t.Context(), f.id, f.root, ".sailry/worktrees/hidden/source.rs"); code(err) != protocol.ErrorCodePermissionDenied {
		t.Fatalf("outline entered a managed worktree: %v", err)
	}
	otherRoot, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(otherRoot, "same.rs"), []byte("pub fn changed() {}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := f.service.OutlineOnDemand(t.Context(), f.id, otherRoot, "same.rs"); code(err) != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("known worktree admitted a different root: %v", err)
	}
	if err := f.service.Remove(f.id); err != nil {
		t.Fatal(err)
	}
	reused = false
	changed, err := f.service.OutlineOnDemand(t.Context(), f.id, otherRoot, "same.rs")
	if reused || err != nil || len(changed.Symbols) != 1 || changed.Symbols[0].Name != "changed" {
		t.Fatalf("reregistered worktree reused an old proof: reused=%t outline=%+v err=%v", reused, changed, err)
	}
}

func TestLiveResultReusesCompleteDerivation(t *testing.T) {
	f := newFixture(t)
	const original = "// α \npub fn alpha() {}\n"
	const changed = "// β\npub fn alpha() { }\n"
	f.write("source.rs", original)
	// Contents must be read and checked when the platform cannot supply a stamp.
	reads := 0
	f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, previous core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
		if previous != (core.FileStamp{}) {
			t.Fatal("unavailable stamp was offered as proof")
		}
		_, err := files.InspectText(ctx, path, limit, previous, func(source *string) error {
			reads++
			return inspect(source)
		})
		return core.FileStamp{}, err
	}
	query := protocol.DefaultCodeIndexSearch()
	query.Name = new("alpha")
	builds := 0
	var symbols *Symbol
	var first protocol.CodeIndexSearchPage
	for round := range 2 {
		ctx, scan, end, err := f.service.liveFiles(t.Context(), f.id, f.root, false)
		if err != nil {
			t.Fatal(err)
		}
		defer end()
		if err := scan.walk("", expression([]string{*query.Name}, false)); err != nil || len(scan.parsed) != 1 || len(scan.parsed[0].Symbols) != 1 {
			t.Fatalf("selection=%+v err=%v", scan.parsed, err)
		}
		page, err := indexedLivePage(ctx, scan, &query, func(i *index, ctx context.Context, query *protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error) {
			builds++
			return i.search(ctx, query)
		})
		if err != nil || len(page.Symbols) != 1 {
			t.Fatalf("search result: %+v, %v", page, err)
		}
		if round == 0 {
			symbols, first = &scan.parsed[0].Symbols[0], page
		} else if symbols != &scan.parsed[0].Symbols[0] || !reflect.DeepEqual(page, first) {
			t.Fatalf("unchanged bytes were not reused: got=%+v want=%+v", page, first)
		}
	}
	if builds != 1 || reads != 2 {
		t.Fatalf("got %d transient builds and %d full reads, want 1 and 2", builds, reads)
	}
	f.write("source.rs", changed)
	page, err := f.service.SearchOnDemand(t.Context(), f.id, f.root, query)
	if err != nil || len(page.Symbols) != 1 {
		t.Fatalf("full reads did not discover modified content: %+v, %v", page, err)
	}
	if symbol := page.Symbols[0]; symbol.FileRevision != Hash(changed) || symbol.FileRevision == first.Symbols[0].FileRevision ||
		symbol.Range.StartByte != uint32(strings.Index(changed, "pub fn")) || symbol.Range.EndByte != uint32(len(changed)-1) {
		t.Fatalf("changed source has stale revision or range: %+v", symbol)
	}
}

func TestOwnLiveParsePreservesSharedScopes(t *testing.T) {
	parsed := parseDenseReferences(t, new(Parser), denseGoSource())
	owned, size := ownLiveParse(&parsed)
	if owned == nil || size == 0 || !reflect.DeepEqual(owned, &parsed) {
		t.Fatalf("detached parse differs from the original: owned=%t size=%d", owned != nil, size)
	}
	original, detached := parsed.References[0].Scope, owned.References[0].Scope
	if detached == nil || detached == original {
		t.Fatalf("cached scope shares the original scope: %+v", detached)
	}
	for i, ref := range owned.References {
		if ref.Scope != detached {
			t.Fatalf("reference %d did not reuse the detached scope", i)
		}
	}
	original.Name = "changed"
	if detached.Name != "run" {
		t.Fatalf("mutation affected the cached scope: %+v", detached)
	}
}
