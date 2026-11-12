package service

import (
	"context"
	"database/sql"
	"encoding/hex"
	"fmt"
	"math"
	"math/rand/v2"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// same compares rankings by key and by the bits of the score.
func same(left, right []knowledge.Ranked) bool {
	return slices.EqualFunc(left, right, func(a, b knowledge.Ranked) bool {
		return a.Key == b.Key && math.Float32bits(a.Score) == math.Float32bits(b.Score)
	})
}

// TestSemanticScan holds the scan to the results of scoring every stored
// vector with Vector.Cosine and sorting all of them, which is what a query
// did before vectors were kept in memory and scanned in parallel.
func TestSemanticScan(t *testing.T) {
	ctx := t.Context()
	random := rand.New(rand.NewPCG(7, 11))
	direction := func(dimension int) []float32 {
		values := make([]float32, dimension)
		for position := range values {
			values[position] = float32(random.NormFloat64())
		}
		return values
	}
	t.Run("parallel scan equals the serial scan on ties", func(t *testing.T) {
		const dimension, total = 24, 20_000
		// A few directions repeat through the block, so far more vectors than
		// the candidate limit share the best scores and only keys order them.
		// The query is one of them, so the tied vectors are the nearest.
		query := direction(dimension)
		repeated := make([][]byte, 5)
		for position := range repeated {
			values := query
			if position != 0 {
				values = direction(dimension)
			}
			vector, err := knowledge.Quantize(values)
			if err != nil {
				t.Fatal(err)
			}
			repeated[position] = vector.Encode()
		}
		held := &block{dimension: dimension}
		var want []knowledge.Ranked
		for position := range total {
			bytes := repeated[position%len(repeated)]
			if position%3 == 0 {
				vector, err := knowledge.Quantize(direction(dimension))
				if err != nil {
					t.Fatal(err)
				}
				bytes = vector.Encode()
			}
			// Keys are not in block order, so ties are not decided by position.
			key := fmt.Sprintf("%08x", uint32(position)*2654435761)
			if err := held.add(key, bytes); err != nil {
				t.Fatal(err)
			}
			vector, err := knowledge.DecodeVector(bytes, dimension)
			if err != nil {
				t.Fatal(err)
			}
			score, err := vector.Cosine(query)
			if err != nil {
				t.Fatal(err)
			}
			want = append(want, knowledge.Ranked{Key: key, Score: score})
		}
		slices.SortFunc(want, order)
		want = want[:candidates]
		if want[0].Score != want[candidates-1].Score {
			t.Fatal("the fixture has no ties across the candidate limit")
		}
		prepared, err := knowledge.NewDirection(query)
		if err != nil {
			t.Fatal(err)
		}
		whole := []span{{held, 0, total}}
		pieces := []span{{held, 12_345, total}, {held, 0, 7}, {held, 7, 12_345}}
		for _, workers := range []int{1, 2, 3, 8, 13} {
			for name, spans := range map[string][]span{"whole": whole, "pieces": pieces} {
				got, err := scan(ctx, prepared, spans, workers)
				if err != nil || !same(got, want) {
					t.Fatalf("%d workers over %s: %v\n got %v\nwant %v", workers, name, err, got[:min(len(got), 5)], want[:5])
				}
			}
		}
		// Fewer vectors than the limit all come back, in order.
		got, err := scan(ctx, prepared, []span{{held, 0, 40}}, 8)
		if err != nil || len(got) != 40 || !slices.IsSortedFunc(got, order) {
			t.Fatalf("short scan = %d candidates, %v", len(got), err)
		}
		cancelled, cancel := context.WithCancel(ctx)
		cancel()
		if _, err := scan(cancelled, prepared, whole, 8); code(err) != protocol.ErrorCodeCancelled {
			t.Fatalf("cancelled scan = %v", err)
		}
	})

	const dimension = 8
	opened, err := store.Open(ctx, filepath.Join(t.TempDir(), "profile"), protocol.NodeID{1})
	if err != nil {
		t.Fatal(err)
	}
	defer opened.Close()
	index := newIndex(opened.Knowledge)
	shared := make([][]float32, 4)
	for position := range shared {
		shared[position] = direction(dimension)
	}
	publish := func(binding Binding, source, chunks int, tag string) {
		t.Helper()
		parts := make([]Part, chunks)
		for position := range parts {
			values := shared[(source+position)%len(shared)]
			if position%2 == 0 {
				values = direction(dimension)
			}
			vector, err := knowledge.Quantize(values)
			if err != nil {
				t.Fatal(err)
			}
			parts[position] = Part{Title: "Notes", Text: fmt.Sprintf("Entry %d of source %d describes subject%d", position, source, position%7), Vector: &vector,
				Position: protocol.KnowledgePosition{Kind: "lines", Data: protocol.KnowledgePositionLinesPayload{Start: uint32(position + 1), End: uint32(position + 1)}}}
		}
		document := Document{Locator: fmt.Sprintf("notes/%d.md", source), Revision: fmt.Sprint(random.Uint64()), ModifiedMs: uint64(source), Tags: []string{tag}, Embedded: true}
		if err := index.Replace(ctx, binding, document, parts); err != nil {
			t.Fatal(err)
		}
	}
	var bindings []Binding
	for _, sources := range []int{120, 10} {
		binding := Binding{ID: protocol.NewKnowledgeID(), Generation: 1, Model: "bge-small-zh", Dimension: dimension, Rebuild: 1}
		if err := index.Bind(ctx, binding); err != nil {
			t.Fatal(err)
		}
		for source := range sources {
			publish(binding, source, 40, []string{"even", "odd"}[source%2])
		}
		bindings = append(bindings, binding)
	}
	large, small := bindings[0], bindings[1]
	embedding := Embedding{Model: "bge-small-zh", Values: shared[1]}
	granting := func(query protocol.KnowledgeQuery) string {
		grants := []map[string]any{}
		for _, binding := range bindings {
			if len(query.Collections) == 0 || slices.Contains(query.Collections, binding.ID) {
				grants = append(grants, map[string]any{"id": binding.ID, "generation": 1, "scope": hex.EncodeToString(binding.Scope[:]), "rebuild": 1, "semantic": true})
			}
		}
		return encoded(grants)
	}
	// serial is the earlier query: every admitted row is decoded and scored.
	serial := func(t *testing.T, db *sql.Conn, query protocol.KnowledgeQuery) []knowledge.Ranked {
		t.Helper()
		var path any
		if query.Path != nil {
			path = *query.Path
		}
		rows, err := db.QueryContext(ctx, "SELECT c.key,c.vector FROM chunks c "+filter+" AND json_extract(admitted.value,'$.semantic')=1 AND k.model=?7 AND k.dimension=?8 AND c.vector IS NOT NULL",
			granting(query), tagsJSON(query.Tags), path, nil, nil, "[]", vectorKey(embedding.Model, embedding.Identity), dimension)
		if err != nil {
			t.Fatal(err)
		}
		defer rows.Close()
		var ranked []knowledge.Ranked
		for rows.Next() {
			var key string
			var bytes []byte
			if err := rows.Scan(&key, &bytes); err != nil {
				t.Fatal(err)
			}
			vector, err := knowledge.DecodeVector(bytes, dimension)
			if err != nil {
				t.Fatal(err)
			}
			score, err := vector.Cosine(embedding.Values)
			if err != nil {
				t.Fatal(err)
			}
			ranked = append(ranked, knowledge.Ranked{Key: key, Score: score})
		}
		if err := rows.Err(); err != nil {
			t.Fatal(err)
		}
		slices.SortFunc(ranked, order)
		return ranked[:min(len(ranked), candidates)]
	}
	prefix := "notes/1"
	queries := map[string]func(*protocol.KnowledgeQuery){
		"unfiltered": func(*protocol.KnowledgeQuery) {},
		"path":       func(query *protocol.KnowledgeQuery) { query.Path = &prefix },
		"tag":        func(query *protocol.KnowledgeQuery) { query.Tags = []string{"odd"} },
		"collection": func(query *protocol.KnowledgeQuery) { query.Collections = []protocol.KnowledgeID{small.ID} },
		"none":       func(query *protocol.KnowledgeQuery) { query.Tags = []string{"absent"} },
	}
	// check runs every query and returns the unfiltered ranking.
	var check func(t *testing.T, state string) []knowledge.Ranked
	rank := func(t *testing.T, state string, queries map[string]func(*protocol.KnowledgeQuery)) (unfiltered []knowledge.Ranked) {
		t.Helper()
		for name, change := range queries {
			query := protocol.DefaultKnowledgeQuery()
			query.Query, query.Mode, query.Limit = "absentword", protocol.KnowledgeModeSemantic, knowledge.DefaultLimit
			change(&query)
			stamps := index.vectors.snapshot(bindings)
			if err := index.read(ctx, func(db *sql.Conn) error {
				got, mode, err := index.ranks(ctx, db, stamps, query, bindings, []Embedding{embedding}, nil)
				if err != nil {
					return err
				}
				want := serial(t, db, query)
				if (name == "none") != (len(want) == 0) || len(want) != 0 && mode != protocol.KnowledgeModeSemantic {
					t.Fatalf("%s, %s: reference has %d candidates in mode %s", state, name, len(want), mode)
				}
				if !same(got, want) {
					t.Fatalf("%s, %s: ranking differs from the serial scan\n got %v\nwant %v", state, name, got[:min(len(got), 3)], want[:min(len(want), 3)])
				}
				if name == "unfiltered" || len(queries) == 1 {
					unfiltered = got
				}
				return nil
			}); err != nil {
				t.Fatalf("%s, %s: %v", state, name, err)
			}
		}
		return unfiltered
	}
	check = func(t *testing.T, state string) []knowledge.Ranked { t.Helper(); return rank(t, state, queries) }
	kept := func(binding Binding) *cached {
		index.vectors.mu.Lock()
		defer index.vectors.mu.Unlock()
		return index.vectors.entries[binding.ID.String()]
	}
	t.Run("queries equal the serial scan of the database", func(t *testing.T) {
		first := check(t, "first read")
		if entry := kept(large); entry == nil || entry.block == nil || len(entry.block.keys) != 120*40 {
			t.Fatalf("vectors kept after a query = %+v", entry)
		}
		block := kept(large).block
		if again := check(t, "kept vectors"); !same(first, again) || kept(large).block != block {
			t.Fatal("a second query read the vectors again or ranked them differently")
		}
		// The collection limit of the fixture ties: the best scores repeat.
		if first[0].Score != first[candidates/2].Score {
			t.Fatal("the fixture has no tied candidates")
		}
	})
	t.Run("a write drops the kept vectors", func(t *testing.T) {
		check(t, "before the write")
		// Both collections hold this source, with 40 chunks each.
		replaced := "notes/3.md"
		source := map[string]func(*protocol.KnowledgeQuery){"source": func(query *protocol.KnowledgeQuery) { query.Path = &replaced }}
		before := rank(t, "before the write", source)
		publish(large, 3, 25, "odd")
		if kept(large) != nil {
			t.Fatal("vectors of a replaced source stayed in memory")
		}
		if kept(small) == nil {
			t.Fatal("a write to one collection dropped another's vectors")
		}
		check(t, "after the write")
		if after := rank(t, "after the write", source); len(before) != 80 || len(after) != 65 {
			t.Fatalf("the replaced source ranked %d chunks before and %d after", len(before), len(after))
		}
		if err := index.Prune(ctx, large, nil, []string{"notes/"}); err != nil || kept(large) != nil {
			t.Fatalf("vectors after a prune = %v %v", kept(large), err)
		}
		check(t, "after a prune")
		if err := index.Retain(ctx, []protocol.KnowledgeID{large.ID, small.ID}); err != nil || kept(large) != nil || kept(small) != nil {
			t.Fatalf("vectors after retaining collections = %v %v %v", kept(large), kept(small), err)
		}
		check(t, "after retaining")
	})
	t.Run("a collection being written is read from the database", func(t *testing.T) {
		want := check(t, "idle")
		finish := index.vectors.change(large.ID.String())
		if got := check(t, "write under way"); !same(got, want) {
			t.Fatal("ranking changed while a write was under way")
		}
		if kept(large) != nil {
			t.Fatal("vectors were kept while a write was under way")
		}
		// A read that began before the write ended keeps nothing either.
		stale := index.vectors.snapshot(bindings)
		finish()
		if index.vectors.idle(large.ID.String(), stale[large.ID.String()]) {
			t.Fatal("a stamp taken during a write counts as idle")
		}
		if got := check(t, "after the write"); !same(got, want) || kept(large) == nil {
			t.Fatal("vectors were not kept again after the write ended")
		}
	})
	t.Run("the memory bound", func(t *testing.T) {
		want := check(t, "within the budget")
		both := kept(large).size + kept(small).size
		if both != index.vectors.size || kept(large).size < 120*40*dimension {
			t.Fatalf("kept %d bytes, entries hold %d", index.vectors.size, both)
		}
		// Room for either collection alone: the one used longest ago goes.
		index.vectors.budget = kept(large).size + 1
		if err := index.Bind(ctx, small); err != nil {
			t.Fatal(err)
		}
		if got := check(t, "one collection fits"); !same(got, want) {
			t.Fatal("ranking changed under a smaller budget")
		}
		if index.vectors.size > index.vectors.budget || kept(large) != nil && kept(small) != nil {
			t.Fatalf("kept %d bytes under a budget of %d", index.vectors.size, index.vectors.budget)
		}
		// Room for neither: both are read from the database in blocks.
		index.vectors.budget = 1
		if err := index.Retain(ctx, []protocol.KnowledgeID{large.ID, small.ID}); err != nil {
			t.Fatal(err)
		}
		if got := check(t, "over the budget"); !same(got, want) {
			t.Fatal("ranking changed without kept vectors")
		}
		if index.vectors.size != 0 || kept(large) == nil || kept(large).block != nil {
			t.Fatalf("kept %d bytes over the budget: %+v", index.vectors.size, kept(large))
		}
		index.vectors.budget = vectorBudget
	})
	t.Run("keyword ranking equals the per-chunk filter", func(t *testing.T) {
		for name, change := range queries {
			query := protocol.DefaultKnowledgeQuery()
			query.Query, query.Mode, query.Limit = "describes subject3", protocol.KnowledgeModeKeyword, knowledge.DefaultLimit
			change(&query)
			var path any
			if query.Path != nil {
				path = *query.Path
			}
			if err := index.read(ctx, func(db *sql.Conn) error {
				got, _, err := index.ranks(ctx, db, nil, query, bindings, nil, nil)
				if err != nil {
					return err
				}
				rows, err := db.QueryContext(ctx, "WITH candidates AS MATERIALIZED (SELECT rowid AS id,-bm25(chunks_fts,2,1,2,1) AS score FROM chunks_fts WHERE chunks_fts MATCH ?7) SELECT c.key,f.score FROM candidates f JOIN chunks c ON c.id=f.id "+filter+" ORDER BY f.score DESC,c.key LIMIT 100",
					granting(query), tagsJSON(query.Tags), path, nil, nil, "[]", knowledge.Query(query.Query))
				if err != nil {
					return err
				}
				defer rows.Close()
				var want []knowledge.Ranked
				for rows.Next() {
					var key string
					var score float64
					if err := rows.Scan(&key, &score); err != nil {
						return err
					}
					want = append(want, knowledge.Ranked{Key: key, Score: float32(score)})
				}
				if (name == "none") != (len(want) == 0) || !same(got, want) {
					t.Fatalf("%s: keyword ranking of %d differs from the per-chunk filter's %d", name, len(got), len(want))
				}
				return rows.Err()
			}); err != nil {
				t.Fatalf("%s: %v", name, err)
			}
		}
	})
}
