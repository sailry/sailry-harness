package codeindex

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"testing"

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
