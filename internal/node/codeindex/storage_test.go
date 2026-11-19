package codeindex

import (
	"context"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPersistentCodeExclusionsKeepOnlyMetadata(t *testing.T) {
	f := newFixture(t)
	f.write("code.go", "package fixture\nfunc Kept() {}\n",
		"settings.json", `{"hidden":true}`,
		"settings.yml", "hidden: true\n",
		"Cargo.lock", "version = 3\n",
		"public/bundle.min.js", "function hidden() {}\n",
		"locale/en.ts", "export function hidden() {}\n",
		"vendor/hidden.go", "package hidden\nfunc Hidden() {}\n")
	f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, stamp core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
		if path != "code.go" {
			t.Errorf("excluded file text was read: %s", path)
		}
		return files.InspectText(ctx, path, limit, stamp, inspect)
	}
	equal(t, "kept symbols", names2(f.search("").Symbols), "Kept")
	w, err := f.service.worktree(f.id, f.root)
	if err != nil {
		t.Fatal(err)
	}
	_, files, err := w.index.snapshot(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{"settings.json", "settings.yml", "Cargo.lock", "public/bundle.min.js", "locale/en.ts", "vendor/hidden.go"} {
		if metadata := files[path]; metadata.state.Status != StatusUnsupported || metadata.hash != nil {
			t.Errorf("excluded file %s has metadata %+v", path, metadata)
		}
	}
}

func TestUnmarkedMinifiedCodeKeepsOnlyMetadata(t *testing.T) {
	parser := &Parser{}
	const file = "public/script.js"
	source := "/*! bundled script */\n" + strings.Repeat("hidden();", 2000)
	parsed, err := parser.ParseIndexed(t.Context(), file, source)
	if err != nil || parsed.Status != StatusUnsupported || parsed.Language != "javascript" || parsed.Hash != Hash(source) || len(parsed.Symbols) > 0 || len(parsed.References) > 0 {
		t.Fatalf("minified bundle parsed: status=%s language=%q symbols=%d references=%d error=%v", parsed.Status, parsed.Language, len(parsed.Symbols), len(parsed.References), err)
	}
}

func TestTransientCodeIndexQueriesAndIsolation(t *testing.T) {
	ctx := t.Context()
	first, err := newTransientIndex(ctx)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = first.close() })
	parser := &Parser{}
	paths := []string{"a.go", "b.go"}
	sources := []string{"package example\nfunc StartWork() {}\n", "package example\nfunc RunWork() { StartWork() }\n"}
	parsed := make([]Parsed, 0, len(paths))
	for index, path := range paths {
		file, err := parser.ParseIndexed(ctx, path, sources[index])
		if err != nil {
			t.Fatal(err)
		}
		parsed = append(parsed, file)
	}
	if _, _, err := first.replace(ctx, parsed, Fingerprint); err != nil {
		t.Fatal(err)
	}
	search := protocol.DefaultCodeIndexSearch()
	search.Query = "StartWork"
	page, err := first.search(ctx, &search)
	if err != nil {
		t.Fatal(err)
	}
	equal(t, "full-text match", names2(page.Symbols), "StartWork")
	if len(page.Symbols) != 1 || page.Symbols[0].FileRevision != Hash(sources[0]) || page.Symbols[0].Path != paths[0] {
		t.Fatalf("stored declaration metadata %+v", page.Symbols)
	}
	graph := graphQuery("callers", "StartWork", paths[0])
	result, err := first.graph(ctx, &graph)
	if err != nil {
		t.Fatal(err)
	}
	equal(t, "callers", sites(result.Callers), []caller{{paths[1], "RunWork", "directory", false}})
	second, err := newTransientIndex(ctx)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = second.close() })
	isolated, err := second.search(ctx, &search)
	if err != nil || len(isolated.Symbols) != 0 || isolated.IndexRevision != 0 {
		t.Fatalf("query-local index inherited other rows: %+v, %v", isolated, err)
	}
}
