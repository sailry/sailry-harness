package codeindex

import (
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/odvcencio/gotreesitter"
	"github.com/odvcencio/gotreesitter/grammars"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type liveGraphSource struct{ path, text string }

func liveGraphAllocFixture(t testing.TB) (*Service, string, []liveGraphSource) {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	const calls = 96
	files := []liveGraphSource{
		{"target.go", "package fixture\nfunc GoTarget() {}\n"},
		{"caller.go", "package fixture\nfunc GoCaller() {\n" + strings.Repeat("\tGoTarget()\n", calls) + "}\n"},
		{"target.rs", "pub fn rust_target() {}\n"},
		{"caller.rs", "pub fn rust_caller() {\n" + strings.Repeat("    rust_target();\n", calls) + "}\n"},
		{"target.ts", "export function tsTarget() {}\n"},
		{"caller.ts", "export function tsCaller() {\n" + strings.Repeat("    tsTarget();\n", calls) + "}\n"},
		{"example.kt", "fun kotlinExample() {}\n"},
	}
	for _, file := range files {
		if err := os.WriteFile(filepath.Join(root, file.path), []byte(file.text), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	service := New(t.TempDir(), nil)
	t.Cleanup(service.Close)
	return service, root, files
}

func liveGraphAllocPage(t testing.TB, service *Service, root string) protocol.CodeIndexGraphGraphPage {
	t.Helper()
	query := graphQuery(protocol.CodeIndexGraphViewMap, "")
	page, err := service.GraphOnDemand(t.Context(), protocol.WorktreeID{1}, root, query)
	if err != nil || page.Partial || page.Truncated || len(page.Issues) != 0 || page.IndexRevision != 0 {
		t.Fatalf("mixed graph: %+v, %v", page, err)
	}
	for _, target := range []struct{ path, name string }{{"target.go", "GoTarget"}, {"target.rs", "rust_target"}, {"target.ts", "tsTarget"}} {
		if !slices.ContainsFunc(page.Map, func(entry protocol.CodeIndexGraphEntry) bool {
			return entry.Path == target.path && slices.ContainsFunc(entry.Symbols, func(symbol protocol.CodeIndexGraphRanked) bool {
				return symbol.Name == target.name && symbol.References > 0
			})
		}) {
			t.Fatalf("missing ranked target %s in %s", target.name, target.path)
		}
	}
	return page
}

func BenchmarkGraphOnDemand(b *testing.B) {
	for _, mode := range []string{"rebuild", "warm"} {
		b.Run(mode, func(b *testing.B) {
			service, root, _ := liveGraphAllocFixture(b)
			liveGraphAllocPage(b, service, root)
			b.ReportAllocs()
			for b.Loop() {
				if mode == "rebuild" {
					service.live.clear()
				}
				liveGraphAllocPage(b, service, root)
			}
		})
	}
}

// Run each mode in its own test process to start with a cold grammar cache.
func TestLiveGraphHeap(t *testing.T) {
	if os.Getenv("SAILRY_CODEINDEX_PERF") != "1" {
		t.Skip("opt-in code index measurement")
	}
	for _, mode := range []string{"cold", "primed"} {
		t.Run(mode, func(t *testing.T) {
			service, root, files := liveGraphAllocFixture(t)
			mark := func(phase string) {
				runtime.GC()
				var m runtime.MemStats
				runtime.ReadMemStats(&m)
				loaded, _ := grammars.EmbeddedLanguageCacheStats()
				t.Logf("CODEINDEX_LIVE_HEAP phase=%s heap_alloc=%d heap_objects=%d total_alloc=%d grammars=%d",
					phase, m.HeapAlloc, m.HeapObjects, m.TotalAlloc, loaded)
			}
			mark("baseline")
			if mode == "primed" {
				languages := map[string]*gotreesitter.Language{}
				for _, file := range files {
					entry := grammars.DetectLanguage(file.path)
					if entry == nil {
						t.Fatalf("missing grammar for %s", file.path)
					}
					languages[entry.Name] = entry.Language()
				}
				mark("grammar_decode")
				for _, file := range files {
					tree, err := parse(t.Context(), grammars.DetectLanguage(file.path), []byte(file.text))
					if err != nil || tree == nil {
						t.Fatalf("grammar-only parse of %s: %v", file.path, err)
					}
					tree.Release()
				}
				gotreesitter.DrainArenaPools()
				mark("grammar_parse")
				for _, name := range []string{"go", "rust", "typescript", "kotlin"} {
					t.Logf("CODEINDEX_LIVE_HEAP language=%s table_estimate=%d", name, languages[name].Size())
				}
			}
			page := liveGraphAllocPage(t, service, root)
			mark("graph_first")
			if warm := liveGraphAllocPage(t, service, root); !reflect.DeepEqual(warm, page) {
				t.Fatal("warm graph changed the results")
			}
			mark("graph_warm")
			for range 3 {
				service.live.clear()
				if rebuilt := liveGraphAllocPage(t, service, root); !reflect.DeepEqual(rebuilt, page) {
					t.Fatal("rebuilt graph changed the results")
				}
			}
			mark("graph_rebuilt")
			service.live.clear()
			mark("live_cache_cleared")
			runtime.KeepAlive(service)
			runtime.KeepAlive(files)
		})
	}
}
