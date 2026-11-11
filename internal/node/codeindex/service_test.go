package codeindex

import (
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// idle is a watch that never reports a change. closed, when set, closes with
// the subscription.
type idle struct {
	initial bool
	closed  chan struct{}
}

func (w *idle) Next(ctx context.Context) (protocol.Update, error) {
	if !w.initial {
		w.initial = true
		return protocol.Update{}, nil
	}
	<-ctx.Done()
	return protocol.Update{}, ctx.Err()
}

func (w *idle) Close() error {
	if w.closed != nil {
		close(w.closed)
	}
	return nil
}

type fixture struct {
	t       *testing.T
	root    string
	profile string
	id      protocol.WorktreeID
	service *Service
}

func newFixture(t *testing.T) *fixture {
	directory, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	f := &fixture{t: t, root: filepath.Join(directory, "repository"), profile: filepath.Join(directory, "profile"), id: protocol.WorktreeID{12}}
	for _, path := range []string{f.root, f.profile} {
		if err := os.Mkdir(path, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	f.service = New(f.profile, func(context.Context, string, protocol.WorktreeID) (link.Subscription, error) { return &idle{}, nil })
	t.Cleanup(f.service.Close)
	return f
}

func (f *fixture) write(files ...string) {
	for index := 0; index < len(files); index += 2 {
		path := filepath.Join(f.root, files[index])
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			f.t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(files[index+1]), 0o600); err != nil {
			f.t.Fatal(err)
		}
	}
}

func (f *fixture) chain() {
	f.write(
		"chain/a.rs", "pub fn a() {}\n",
		"chain/b.rs", "pub fn b() {\n    a();\n}\n",
		"chain/c.rs", "pub fn c() {\n    b();\n}\n",
		"chain/d.rs", "pub fn d() {\n    c();\n}\n",
		"chain/e.rs", "pub fn e() {\n    d();\n}\n",
	)
}

func graphQuery(view protocol.CodeIndexGraphView, name string, paths ...string) protocol.CodeIndexGraph {
	query := protocol.DefaultCodeIndexGraph()
	query.View, query.Paths, query.Depth, query.Limit = view, append([]string{}, paths...), 3, 50
	if name != "" {
		query.Name = &name
	}
	return query
}

func (f *fixture) graph(query protocol.CodeIndexGraph) protocol.CodeIndexGraphGraphPage {
	f.t.Helper()
	page, err := f.service.Graph(context.Background(), f.id, f.root, query)
	if err != nil {
		f.t.Fatalf("graph %+v: %v", query, err)
	}
	return page
}

func (f *fixture) search(text string) protocol.CodeIndexSearchPage {
	f.t.Helper()
	query := protocol.DefaultCodeIndexSearch()
	query.Query = text
	page, err := f.service.Search(context.Background(), f.id, f.root, query)
	if err != nil {
		f.t.Fatalf("search %q: %v", text, err)
	}
	return page
}

func (f *fixture) database() string {
	return filepath.Join(f.profile, "cache", "code-index", f.id.String(), "index.sqlite")
}

// origin is the place and resolution of one reference.
type caller struct {
	Path, Scope, Confidence string
	Ambiguous               bool
}

func sites(references []protocol.CodeIndexGraphReference) []caller {
	found := []caller{}
	for _, reference := range references {
		item := caller{Path: reference.Path, Ambiguous: reference.Ambiguous}
		if reference.Scope != nil {
			item.Scope = reference.Scope.Name
		}
		if reference.Confidence != nil {
			item.Confidence = string(*reference.Confidence)
		}
		found = append(found, item)
	}
	return found
}

type hop struct {
	Path  string
	Depth uint8
}

func reached(page protocol.CodeIndexGraphGraphPage) []hop {
	found := []hop{}
	for _, item := range page.Impact {
		found = append(found, hop{item.Path, item.Depth})
	}
	return found
}

func names2(symbols []Symbol) string {
	found := []string{}
	for _, symbol := range symbols {
		found = append(found, symbol.Name)
	}
	return strings.Join(found, ",")
}

func equal(t *testing.T, label string, got, want any) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Errorf("%s:\n got %+v\nwant %+v", label, got, want)
	}
}

// TestGraphBounds transcribes the Rust graph assertions: resolution tiers,
// the impact depth cap, paging, map ranking and rejected unbounded queries.
func TestGraphBounds(t *testing.T) {
	t.Run("callers resolve by preference", func(t *testing.T) {
		f := newFixture(t)
		f.write(
			"src/store.rs", "pub fn open() {}\npub fn helper() {}\n",
			"src/app.rs", "pub fn start() {\n    open();\n    helper();\n}\n\nfn open() {}\n",
			"src/cli.rs", "use crate::store::open;\n\npub fn main() {\n    open();\n}\n",
			"tools/open.rs", "pub fn open() {}\n",
			"src/util.rs", "pub fn shared() {}\n",
			"src/web.rs", "pub fn serve() {\n    shared();\n}\n",
			"other/shared.rs", "pub fn shared() {}\n",
			"docs/demo.rs", "fn demo() {\n    shared();\n}\n",
		)
		store := f.graph(graphQuery("callers", "open", "src/store.rs"))
		equal(t, "store targets", len(store.Targets), 1)
		equal(t, "store callers", sites(store.Callers), []caller{{"src/cli.rs", "main", "imported", false}})
		local := f.graph(graphQuery("callers", "open", "src/app.rs"))
		equal(t, "local callers", sites(local.Callers), []caller{{"src/app.rs", "start", "local", false}})
		util := f.graph(graphQuery("callers", "shared", "src/util.rs"))
		equal(t, "util callers", sites(util.Callers), []caller{{"src/web.rs", "serve", "directory", false}, {"docs/demo.rs", "demo", "workspace", true}})
		if len(util.Callers) == 2 {
			demo := util.Callers[1]
			equal(t, "demo", []uint64{demo.Candidates, uint64(demo.Line), uint64(demo.Column), uint64(demo.Count)}, []uint64{2, 1, 4, 1})
		}
		other := f.graph(graphQuery("callers", "shared", "other/shared.rs"))
		equal(t, "other callers", sites(other.Callers), []caller{{"docs/demo.rs", "demo", "workspace", true}})
		callees := f.graph(graphQuery("callees", "start"))
		resolved := [][3]string{}
		for _, reference := range callees.Callees {
			item := [3]string{reference.Name}
			if reference.Confidence != nil {
				item[1] = string(*reference.Confidence)
			}
			if len(reference.Targets) != 0 {
				item[2] = reference.Targets[0].Path
			}
			resolved = append(resolved, item)
		}
		equal(t, "callees", resolved, [][3]string{{"open", "local", "src/app.rs"}, {"helper", "directory", "src/store.rs"}})
	})
	t.Run("impact depth is capped and paged", func(t *testing.T) {
		f := newFixture(t)
		f.chain()
		full := f.graph(graphQuery("impact", "a"))
		equal(t, "full", reached(full), []hop{{"chain/b.rs", 1}, {"chain/c.rs", 2}, {"chain/d.rs", 3}})
		for _, item := range full.Impact {
			if item.Confidence != "directory" || item.Ambiguous || len(item.Symbols) != 1 || item.Symbols[0].Depth != item.Depth {
				t.Errorf("impacted %+v", item)
			}
		}
		near := graphQuery("impact", "a")
		near.Depth = 1
		equal(t, "near", reached(f.graph(near)), []hop{{"chain/b.rs", 1}})
		equal(t, "changed", reached(f.graph(graphQuery("impact", "", "chain/a.rs"))), reached(full))
		paged := graphQuery("impact", "a")
		paged.Limit, paged.Offset = 1, 1
		page := f.graph(paged)
		equal(t, "paged", reached(page), []hop{{"chain/c.rs", 2}})
		next := uint64(2)
		equal(t, "paged total", page.Total, uint64(3))
		equal(t, "paged next", page.NextOffset, &next)
		equal(t, "paged truncated", page.Truncated, true)
	})
	t.Run("map ranks deterministically", func(t *testing.T) {
		f := newFixture(t)
		f.chain()
		first := f.graph(graphQuery("map", ""))
		second := f.graph(graphQuery("map", ""))
		equal(t, "repeat", first.Map, second.Map)
		equal(t, "total", first.Total, uint64(5))
		if len(first.Map) != 5 {
			t.Fatalf("map %+v", first.Map)
		}
		equal(t, "top", []any{first.Map[0].Path, first.Map[0].Score, first.Map[0].Symbols[0].Name, first.Map[0].Symbols[0].References}, []any{"chain/a.rs", uint32(1000), "a", uint32(1)})
		equal(t, "focused", f.graph(graphQuery("map", "", "chain/e.rs")).Map[0].Path, "chain/e.rs")
		bounded := graphQuery("map", "")
		bounded.Limit = 2
		page := f.graph(bounded)
		next := uint64(2)
		equal(t, "bounded", page.Map, first.Map[:2])
		equal(t, "bounded next", page.NextOffset, &next)
	})
	t.Run("context combines one definition", func(t *testing.T) {
		f := newFixture(t)
		f.chain()
		query := graphQuery("context", "", "chain/c.rs")
		line := uint32(1)
		query.Line = &line
		context := f.graph(query)
		if len(context.Targets) != 1 || context.Targets[0].Name != "c" || len(context.Callers) == 0 || len(context.Callees) == 0 {
			t.Fatalf("context %+v", context)
		}
		equal(t, "caller", context.Callers[0].Path, "chain/d.rs")
		equal(t, "callee", context.Callees[0].Name, "b")
		equal(t, "impact", reached(context), []hop{{"chain/d.rs", 1}, {"chain/e.rs", 2}})
		equal(t, "total", context.Total, uint64(2))
	})
	t.Run("unbounded queries are rejected", func(t *testing.T) {
		f := newFixture(t)
		deep := graphQuery("impact", "a")
		deep.Depth = 4
		wide := graphQuery("map", "")
		wide.Limit = maxGraphItems + 1
		lined := graphQuery("callers", "", "a.rs", "b.rs")
		line := uint32(1)
		lined.Line = &line
		for _, invalid := range []protocol.CodeIndexGraph{graphQuery("callers", ""), graphQuery("impact", ""), deep, wide, lined, graphQuery("map", "", "../outside")} {
			_, err := f.service.Graph(context.Background(), f.id, f.root, invalid)
			if code(err) != protocol.ErrorCodeInvalidRequest {
				t.Errorf("%+v: %v", invalid, err)
			}
		}
	})
	t.Run("reads stop at their bounds", func(t *testing.T) {
		f := newFixture(t)
		for index := range maxTargets + 5 {
			f.write("many/"+string(rune('a'+index))+".rs", "pub fn dup() {}\n")
		}
		f.write("use.rs", "fn user() {\n    dup();\n    dup();\n    other();\n}\n")
		page := f.graph(graphQuery("callers", "dup"))
		equal(t, "targets", len(page.Targets), maxTargets)
		equal(t, "limited", []bool{page.Partial, page.Truncated}, []bool{true, true})
		if len(page.Callers) != 1 || page.Callers[0].Count != 2 || page.Callers[0].Candidates != maxTargets+5 || len(page.Callers[0].Targets) != maxSamples {
			t.Errorf("callers %+v", page.Callers)
		}
		worktree, err := f.service.worktree(f.id, f.root)
		if err != nil {
			t.Fatal(err)
		}
		err = worktree.index.handle.Read(context.Background(), func(db *sql.Conn) error {
			reader := &reader{ctx: context.Background(), db: db}
			found, err := reader.references([]string{"dup", "other"}, 2)
			if len(found) != 2 || !reader.limited {
				t.Errorf("references %+v limited %v", found, reader.limited)
			}
			reader.limited = false
			inside, err2 := reader.inside("use.rs", 0, 1000, 1)
			if len(inside) != 1 || !reader.limited {
				t.Errorf("inside %+v limited %v", inside, reader.limited)
			}
			reader.limited = false
			within, err3 := reader.within([]string{"many"}, 3)
			if len(within) != 3 || !reader.limited {
				t.Errorf("within %+v limited %v", within, reader.limited)
			}
			return errors.Join(err, err2, err3)
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}

// TestIndexRebuildable proves the index is a cache: records follow file
// changes and deletions, rows written under other extraction rules are parsed
// again, and a deleted database is rebuilt to the same answers.
func TestIndexRebuildable(t *testing.T) {
	t.Run("follows updates and deletions", func(t *testing.T) {
		f := newFixture(t)
		f.chain()
		before := f.graph(graphQuery("callers", "b"))
		equal(t, "before", sites(before.Callers), []caller{{"chain/c.rs", "c", "directory", false}})
		f.write("chain/c.rs", "pub fn c() {}\n")
		changed := "chain/c.rs"
		f.service.Changed(f.id, &changed)
		equal(t, "updated", len(f.graph(graphQuery("callers", "b")).Callers), 0)
		if err := os.Remove(filepath.Join(f.root, "chain/a.rs")); err != nil {
			t.Fatal(err)
		}
		f.service.Changed(f.id, nil)
		removed := f.graph(graphQuery("callers", "a"))
		for attempts := 0; (removed.Partial || removed.Truncated) && attempts < 3; attempts++ {
			removed = f.graph(graphQuery("callers", "a"))
		}
		equal(t, "removed", []any{removed.Partial, removed.Truncated, len(removed.Targets)}, []any{false, false, 0})
		callees := f.graph(graphQuery("callees", "b")).Callees
		if len(callees) != 1 || callees[0].Confidence != nil || len(callees[0].Targets) != 0 {
			t.Errorf("callees %+v", callees)
		}
	})
	t.Run("watch batches name the changed files", func(t *testing.T) {
		f := newFixture(t)
		f.write("main.rs", "pub fn main() {}\n")
		f.search("")
		worktree, _ := f.service.worktree(f.id, f.root)
		f.service.Observed(f.id, []string{
			filepath.Join(f.root, "main.rs"), filepath.Join(f.root, "src", "lib.rs"),
			// Never indexed: the root, Git's files, other formats and other trees.
			f.root, filepath.Join(f.root, ".git", "index"), filepath.Join(f.root, "vendor", ".git", "HEAD"),
			filepath.Join(f.root, "notes.unknownext"), filepath.Join(filepath.Dir(f.root), "outside.rs"),
		})
		generation, changed := worktree.dirty.snapshot()
		equal(t, "changed", changed, map[string]uint64{"main.rs": generation - 1, "src/lib.rs": generation})
		// The remembered paths only order a reconcile and stay bounded.
		for index := range maxChanged + 8 {
			f.service.Observed(f.id, []string{filepath.Join(f.root, "generated", strconv.Itoa(index)+".rs")})
		}
		_, changed = worktree.dirty.snapshot()
		equal(t, "bounded", len(changed), maxChanged)
		f.write("main.rs", "pub fn renamed() {}\n")
		f.service.Observed(f.id, []string{filepath.Join(f.root, "main.rs")})
		equal(t, "reindexed", names2(f.search("").Symbols), "renamed")
	})
	t.Run("an idle worktree releases its watch and reopens on the next query", func(t *testing.T) {
		f := newFixture(t)
		closed := make(chan struct{})
		f.service.watch = func(context.Context, string, protocol.WorktreeID) (link.Subscription, error) {
			return &idle{closed: closed}, nil
		}
		f.write("main.rs", "pub fn main() {}\n")
		first := f.search("")
		f.service.releaseIdle(time.Now().Add(idleRelease - time.Minute))
		select {
		case <-closed:
			t.Fatal("a used worktree lost its watch")
		default:
		}
		f.service.releaseIdle(time.Now().Add(idleRelease + time.Second))
		select {
		case <-closed:
		case <-time.After(10 * time.Second):
			t.Fatal("an idle worktree kept its watch")
		}
		f.service.mutex.Lock()
		held := len(f.service.worktrees)
		f.service.mutex.Unlock()
		equal(t, "held", held, 0)
		f.service.watch = func(context.Context, string, protocol.WorktreeID) (link.Subscription, error) { return &idle{}, nil }
		again := f.search("")
		equal(t, "reopened", []any{names2(again.Symbols), again.IndexRevision}, []any{"main", first.IndexRevision})
	})
	t.Run("replaces symbols, text index and metadata together", func(t *testing.T) {
		f := newFixture(t)
		f.write("main.rs", "pub fn retired() {}\n")
		first := f.search("")
		equal(t, "unchanged revision", f.search("").IndexRevision, first.IndexRevision)
		// A same-size change keeps the size; only the full hash proves it.
		f.write("main.rs", "pub fn current() {}\n")
		current := f.search("")
		equal(t, "current", names2(current.Symbols), "current")
		if current.IndexRevision <= first.IndexRevision || current.Symbols[0].FileRevision == first.Symbols[0].FileRevision {
			t.Errorf("revisions %d %d", first.IndexRevision, current.IndexRevision)
		}
		equal(t, "retired", len(f.search("retired").Symbols), 0)
		equal(t, "words", names2(f.search("current").Symbols), "current")
		outline, err := f.service.Outline(context.Background(), f.id, f.root, "main.rs")
		if err != nil {
			t.Fatal(err)
		}
		if outline.FileRevision == nil || len(outline.Symbols) != 1 || outline.Symbols[0].FileRevision != *outline.FileRevision || *outline.FileRevision != Hash("pub fn current() {}\n") {
			t.Errorf("outline %+v", outline)
		}
		equal(t, "outline revision", outline.IndexRevision, current.IndexRevision)
	})
	t.Run("outline parses only the requested file", func(t *testing.T) {
		f := newFixture(t)
		f.write("first.rs", "pub fn first() {}\n", "second.rs", "pub fn second() {}\n", "notes.unknownext", "text\n")
		outline, err := f.service.Outline(context.Background(), f.id, f.root, "first.rs")
		if err != nil {
			t.Fatal(err)
		}
		equal(t, "outline", names2(outline.Symbols), "first")
		worktree, _ := f.service.worktree(f.id, f.root)
		_, files, err := worktree.index.snapshot(context.Background())
		if err != nil || len(files) != 1 {
			t.Fatalf("files %v %v", files, err)
		}
		equal(t, "search", names2(f.search("").Symbols), "first,second")
		unsupported, err := f.service.Outline(context.Background(), f.id, f.root, "notes.unknownext")
		if err != nil {
			t.Fatal(err)
		}
		equal(t, "unsupported", []any{unsupported.Status, len(unsupported.Symbols), unsupported.Language}, []any{StatusUnsupported, 0, (*string)(nil)})
	})
	t.Run("oversized status is reused until the size changes", func(t *testing.T) {
		f := newFixture(t)
		f.write("main.rs", strings.Repeat("x", MaxFileBytes+1))
		first := f.search("")
		equal(t, "issues", first.Issues, []protocol.CodeIndexIssue{{Path: "main.rs", Status: StatusTooLarge}})
		f.service.Changed(f.id, nil)
		equal(t, "checked", f.search("").IndexRevision, first.IndexRevision)
		f.write("main.rs", "pub fn now_readable() {}\n")
		changed := "main.rs"
		f.service.Changed(f.id, &changed)
		parsed := f.search("")
		equal(t, "parsed", []any{parsed.IndexRevision, names2(parsed.Symbols)}, []any{first.IndexRevision + 1, "now_readable"})
	})
	t.Run("other extraction rules are parsed again", func(t *testing.T) {
		f := newFixture(t)
		f.write("main.rs", "pub fn original() {}\n")
		first := f.search("")
		worktree, _ := f.service.worktree(f.id, f.root)
		// Another engine's row: its fingerprint and a declaration this one
		// does not extract.
		foreign, err := f.service.parser.Parse(context.Background(), "main.rs", "pub fn foreign() {}\n")
		if err != nil {
			t.Fatal(err)
		}
		foreign.Hash = Hash("pub fn original() {}\n")
		if _, _, err := worktree.index.replace(context.Background(), []Parsed{foreign}, "different-extraction-rules"); err != nil {
			t.Fatal(err)
		}
		rebuilt := f.search("")
		equal(t, "rebuilt", names2(rebuilt.Symbols), "original")
		if rebuilt.IndexRevision <= first.IndexRevision+1 {
			t.Errorf("revision %d after %d", rebuilt.IndexRevision, first.IndexRevision)
		}
		_, files, err := worktree.index.snapshot(context.Background())
		if err != nil || files["main.rs"].fingerprint != Fingerprint {
			t.Errorf("fingerprint %+v %v", files["main.rs"], err)
		}
	})
	t.Run("a removed cache is rebuilt", func(t *testing.T) {
		f := newFixture(t)
		f.chain()
		first := f.graph(graphQuery("impact", "a"))
		if _, err := os.Stat(f.database()); err != nil {
			t.Fatal(err)
		}
		for range 2 {
			if err := f.service.Remove(f.id); err != nil {
				t.Fatal(err)
			}
		}
		if _, err := os.Stat(filepath.Dir(f.database())); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("cache remains: %v", err)
		}
		if _, err := os.Stat(filepath.Join(f.root, "chain/a.rs")); err != nil {
			t.Fatal(err)
		}
		rebuilt := f.graph(graphQuery("impact", "a"))
		equal(t, "impact", rebuilt.Impact, first.Impact)
		equal(t, "revision", rebuilt.IndexRevision, first.IndexRevision)
		f.service.Configure(false)
		if _, err := f.service.Search(context.Background(), f.id, f.root, protocol.DefaultCodeIndexSearch()); code(err) != protocol.ErrorCodeNotConfigured {
			t.Errorf("disabled search: %v", err)
		}
		if err := f.service.Remove(f.id); err != nil {
			t.Fatal(err)
		}
		if _, err := os.Stat(filepath.Dir(f.database())); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("cache remains: %v", err)
		}
	})
	t.Run("cancelled queries commit nothing", func(t *testing.T) {
		f := newFixture(t)
		f.write("main.rs", "pub fn preserved() {}\n")
		first := f.search("")
		f.write("main.rs", "pub fn discarded() {}\n")
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		if _, err := f.service.Search(ctx, f.id, f.root, protocol.DefaultCodeIndexSearch()); code(err) != protocol.ErrorCodeCancelled {
			t.Errorf("cancelled search: %v", err)
		}
		worktree, _ := f.service.worktree(f.id, f.root)
		revision, err := worktree.index.revision(context.Background())
		equal(t, "revision", []any{revision, err}, []any{first.IndexRevision, error(nil)})
	})
}
