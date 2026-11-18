//go:build darwin || linux

package codeindex

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	perfHotQueries = 6
	perfDenseCalls = 96
)

type perfTree struct {
	files, fileBytes, symbols, refs int
	first, hash                     string
}

func perfSource(shape string, n int) string {
	var s strings.Builder
	fmt.Fprintf(&s, "pub fn perfnode%03d() {\n", n)
	calls := 0
	if shape == "dense" {
		calls = perfDenseCalls
	} else if n == 1 {
		calls = 1
	}
	for range calls {
		s.WriteString("    perfnode000();\n")
	}
	s.WriteString("}\n")
	if n == 0 {
		s.WriteString("pub fn retired() {}\n")
	}
	size := 4096
	if shape == "large" {
		size = 256 << 10
		for i := range 32 {
			fmt.Fprintf(&s, "pub fn block%03d() {\n    /*%s*/\n}\n", i, strings.Repeat("x", 8100))
		}
	}
	s.WriteString("//")
	s.WriteString(strings.Repeat("x", size-s.Len()-1))
	s.WriteByte('\n')
	return s.String()
}

func perfSeed(f *fixture, shape string) perfTree {
	tree := perfTree{files: 48, refs: 1}
	if shape == "dense" || shape == "large" {
		tree.files = 16
	}
	tree.symbols = tree.files + 1
	if shape == "dense" {
		tree.refs = tree.files * perfDenseCalls
	} else if shape == "large" {
		tree.symbols += tree.files * 32
	}
	for i := range tree.files {
		source := perfSource(shape, i)
		f.write(fmt.Sprintf("src/f%03d.rs", i), source)
		if i == 0 {
			tree.first, tree.fileBytes, tree.hash = source, len(source), Hash(source)
		}
	}
	return tree
}

// Each page is an externally observable response, not an internal commit.
type perfPage struct {
	MS       float64 `json:"ms"`
	Hits     int     `json:"hits"`
	Visited  uint64  `json:"reported_visited"`
	Revision uint64  `json:"index_revision"`
	Partial  bool    `json:"partial"`
	Cut      bool    `json:"truncated"`
}

func perfMS(d time.Duration) float64 { return float64(d) / float64(time.Millisecond) }

func perfSearch(f *fixture, name string) protocol.CodeIndexSearchPage {
	q := protocol.DefaultCodeIndexSearch()
	q.Query, q.Name = name, &name
	p, err := f.service.Search(context.Background(), f.id, f.root, q)
	if err != nil {
		f.t.Fatal(err)
	}
	return p
}

func perfSearchPage(p protocol.CodeIndexSearchPage, since time.Time) perfPage {
	return perfPage{perfMS(time.Since(since)), len(p.Symbols), p.Visited, p.IndexRevision, p.Partial, p.Truncated}
}

func perfMatch(t *testing.T, p protocol.CodeIndexSearchPage, name, hash string) {
	t.Helper()
	if len(p.Symbols) != 1 || p.Symbols[0].Name != name || p.Symbols[0].Path != "src/f000.rs" || p.Symbols[0].FileRevision != hash || len(p.Issues) != 0 || p.Partial || p.Truncated {
		t.Fatalf("incomplete or incorrect %q result: %+v", name, p)
	}
}

func perfCold(f *fixture, tree perfTree, start time.Time, pages *[]perfPage) protocol.CodeIndexSearchPage {
	for range 8 {
		p := perfSearch(f, "retired")
		*pages = append(*pages, perfSearchPage(p, start))
		if len(p.Issues) != 0 {
			f.t.Fatalf("cold page has indexing issues: %+v", p.Issues)
		}
		if !p.Partial && !p.Truncated {
			perfMatch(f.t, p, "retired", tree.hash)
			return p
		}
	}
	f.t.Fatalf("no complete page: %+v", *pages)
	return protocol.CodeIndexSearchPage{}
}

func perfCheckRows(f *fixture, tree perfTree) {
	f.t.Helper()
	w, err := f.service.worktree(f.id, f.root)
	if err != nil {
		f.t.Fatal(err)
	}
	var files, symbols, refs int
	err = w.index.handle.Read(context.Background(), func(db *sql.Conn) error {
		return db.QueryRowContext(context.Background(), "SELECT (SELECT count(*) FROM files), (SELECT count(*) FROM symbols), (SELECT count(*) FROM refs)").Scan(&files, &symbols, &refs)
	})
	if err != nil || files != tree.files || symbols != tree.symbols || refs != tree.refs {
		f.t.Fatalf("stored files/symbols/refs = %d/%d/%d, want %d/%d/%d: %v", files, symbols, refs, tree.files, tree.symbols, tree.refs, err)
	}
}

type perfSample struct {
	at            time.Time
	cpu           time.Duration
	bytes, allocs uint64
}

func perfMark(t *testing.T) perfSample {
	var m runtime.MemStats
	runtime.ReadMemStats(&m)
	var u syscall.Rusage
	if err := syscall.Getrusage(syscall.RUSAGE_SELF, &u); err != nil {
		t.Fatal(err)
	}
	return perfSample{time.Now(), time.Duration(u.Utime.Nano() + u.Stime.Nano()), m.TotalAlloc, m.Mallocs}
}

func perfLog(t *testing.T, before, after perfSample, fields map[string]any) {
	t.Helper()
	fields["scenario"] = strings.TrimPrefix(t.Name(), "TestCodeIndexPerf/")
	fields["wall_ms"] = perfMS(after.at.Sub(before.at))
	fields["process_cpu_s"] = (after.cpu - before.cpu).Seconds()
	fields["go_alloc_bytes"] = after.bytes - before.bytes
	fields["go_allocs"] = after.allocs - before.allocs
	encoded, err := json.Marshal(fields)
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("CODEINDEX_PERF %s", encoded)
}

// Run one scenario per process for independent cold grammar state and peak measurements.
func TestCodeIndexPerf(t *testing.T) {
	if os.Getenv("SAILRY_CODEINDEX_PERF") != "1" {
		t.Skip("opt-in code index measurement")
	}
	for _, scenario := range []string{"cold", "cold_dense", "cold_large", "warm_only", "hot_miss", "hot_hit", "hot_graph", "rewrite"} {
		t.Run(scenario, func(t *testing.T) {
			shape := strings.TrimPrefix(scenario, "cold_")
			f := newFixture(t)
			tree := perfSeed(f, shape)
			pages := make([]perfPage, 0, 8)
			fields := map[string]any{"files": tree.files, "bytes_per_file": tree.fileBytes, "expected_symbols": tree.symbols, "expected_refs": tree.refs}
			var before, after perfSample
			if strings.HasPrefix(scenario, "cold") {
				before = perfMark(t)
				last := perfCold(f, tree, before.at, &pages)
				after = perfMark(t)
				fields["final_file_revision"] = last.Symbols[0].FileRevision
				fields["first_response_ms"], fields["complete_ms"] = pages[0].MS, pages[len(pages)-1].MS
				for _, p := range pages {
					if p.Hits != 0 {
						fields["first_hit_ms"] = p.MS
						break
					}
				}
			} else {
				warm := perfCold(f, tree, time.Now(), &pages)
				fields["warm_pages"], fields["warm_revision"] = len(pages), warm.IndexRevision
				pages = pages[:0]
				switch scenario {
				case "warm_only":
					before = perfMark(t)
					after = perfMark(t)
				case "rewrite":
					source := strings.Replace(tree.first, "retired", "current", 1)
					newHash := Hash(source)
					f.write("src/f000.rs", source)
					before = perfMark(t)
					old := perfSearch(f, "retired")
					pages = append(pages, perfSearchPage(old, before.at))
					current := perfSearch(f, "current")
					pages = append(pages, perfSearchPage(current, before.at))
					after = perfMark(t)
					if old.Partial || old.Truncated || len(old.Issues) != 0 || len(old.Symbols) != 0 || old.IndexRevision <= warm.IndexRevision || old.IndexRevision != current.IndexRevision {
						t.Fatalf("old symbol or inconsistent revisions: %+v, new revision %d", old, current.IndexRevision)
					}
					perfMatch(t, current, "current", newHash)
					fields["old_absent_ms"], fields["new_visible_ms"] = pages[0].MS, pages[1].MS
					fields["old_file_revision"], fields["new_file_revision"] = tree.hash, current.Symbols[0].FileRevision
				default:
					q := graphQuery("callers", "perfnode000", "src/f000.rs")
					before = perfMark(t)
					for range perfHotQueries {
						start := time.Now()
						if scenario == "hot_graph" {
							p := f.graph(q)
							pages = append(pages, perfPage{perfMS(time.Since(start)), len(p.Callers), p.Visited, p.IndexRevision, p.Partial, p.Truncated})
							if len(p.Targets) != 1 || len(p.Callers) != 1 || p.Callers[0].Path != "src/f001.rs" || p.Callers[0].Scope == nil || p.Callers[0].Scope.Name != "perfnode001" || p.Callers[0].Count != 1 || len(p.Issues) != 0 {
								t.Fatalf("unexpected graph: %+v", p)
							}
						} else {
							name := "retired"
							if scenario == "hot_miss" {
								name = "absentmarker"
							}
							p := perfSearch(f, name)
							pages = append(pages, perfSearchPage(p, start))
							if scenario == "hot_hit" {
								perfMatch(t, p, name, tree.hash)
							} else if len(p.Symbols) != 0 || len(p.Issues) != 0 {
								t.Fatalf("unexpected miss page: %+v", p)
							}
						}
						if p := pages[len(pages)-1]; p.Revision != warm.IndexRevision || p.Partial || p.Cut {
							t.Fatalf("unexpected hot state: %+v, warm revision %d", p, warm.IndexRevision)
						}
					}
					after = perfMark(t)
				}
			}
			perfCheckRows(f, tree)
			fields["pages"] = pages
			perfLog(t, before, after, fields)
		})
	}
	t.Run("parse_dense", func(t *testing.T) {
		source := perfSource("dense", 1)
		var parser Parser
		before := perfMark(t)
		p, err := parser.Parse(context.Background(), "src/f001.rs", source)
		after := perfMark(t)
		if err != nil || p.Status != StatusParsed || p.Partial || p.Truncated || len(p.Symbols) != 1 || len(p.References) != perfDenseCalls {
			t.Fatalf("parse: status=%s partial=%t truncated=%t symbols=%d refs=%d err=%v", p.Status, p.Partial, p.Truncated, len(p.Symbols), len(p.References), err)
		}
		start := strings.Index(source, "    perfnode000();") + 4
		for i, r := range p.References {
			if r.Name != "perfnode000" || int(r.Start) != start+i*len("    perfnode000();\n") || int(r.Line) != i+1 || r.Column != 4 || r.Scope == nil || r.Scope.Name != "perfnode001" || r.Scope.Kind != "function" || r.Scope.StartLine != 0 || r.Scope.EndLine != perfDenseCalls+1 {
				t.Fatalf("reference %d: %+v scope=%+v", i, r, r.Scope)
			}
		}
		perfLog(t, before, after, map[string]any{"files": 1, "bytes_per_file": len(source), "symbols": len(p.Symbols), "refs": len(p.References), "partial": p.Partial, "truncated": p.Truncated, "file_revision": p.Hash})
	})
}
