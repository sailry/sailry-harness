package service

import (
	"fmt"
	"math/rand/v2"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// BenchmarkSemanticScan measures one query over a growing collection of
// 384-component vectors, the dimension of the default embedding model. Every
// semantic or hybrid query scores each admitted vector, so its cost grows
// with the corpus; docs/knowledge.md records the measured limit. The semantic
// case runs first after the collection grew, so its mean includes reading the
// vectors into memory once. The semantic and hybrid cases ask for a word that
// one chunk in 500 has; the common case asks for words every chunk has, which
// adds the keyword ranking of the whole corpus. The streamed case keeps no
// vectors, as for a collection over the memory budget or one being written.
//
//	go test ./internal/node/knowledge/service -run '^$' -bench SemanticScan -benchtime 20x
func BenchmarkSemanticScan(b *testing.B) {
	const dimension, perSource = 384, 50
	opened, err := store.Open(b.Context(), filepath.Join(b.TempDir(), "profile"), protocol.NodeID{1})
	if err != nil {
		b.Fatal(err)
	}
	defer opened.Close()
	index := newIndex(opened.Knowledge, nil)
	binding := Binding{ID: protocol.NewKnowledgeID(), Generation: 1, Model: localmodels.DefaultEmbedding, Dimension: dimension, Rebuild: 1}
	if err := index.Bind(b.Context(), binding); err != nil {
		b.Fatal(err)
	}
	random := rand.New(rand.NewPCG(1, 2))
	direction := func() []float32 {
		values := make([]float32, dimension)
		for position := range values {
			values[position] = float32(random.NormFloat64())
		}
		return values
	}
	chunks := 0
	grow := func(total int) {
		for ; chunks < total; chunks += perSource {
			parts := make([]Part, perSource)
			for position := range parts {
				vector, err := knowledge.Quantize(direction())
				if err != nil {
					b.Fatal(err)
				}
				parts[position] = Part{Title: "Notes", Text: fmt.Sprintf("Entry %d describes subject%d of the collection", chunks+position, (chunks+position)%500), Vector: &vector,
					Position: protocol.KnowledgePosition{Kind: "lines", Data: protocol.KnowledgePositionLinesPayload{Start: uint32(position + 1), End: uint32(position + 1)}}}
			}
			document := Document{Locator: fmt.Sprintf("notes/%d.md", chunks/perSource), Revision: "1", Embedded: true}
			if err := index.Replace(b.Context(), binding, document, parts); err != nil {
				b.Fatal(err)
			}
		}
	}
	embedding := []Embedding{{Model: binding.Model, Values: direction()}}
	for _, total := range []int{10_000, 25_000, 50_000, 100_000, 200_000} {
		grow(total)
		cases := []struct {
			name, text string
			mode       protocol.KnowledgeMode
		}{
			{"semantic", "subject123", protocol.KnowledgeModeSemantic},
			{"hybrid", "subject123", protocol.KnowledgeModeHybrid},
			{"common", "describes the collection", protocol.KnowledgeModeHybrid},
			{"streamed", "subject123", protocol.KnowledgeModeSemantic},
		}
		for _, test := range cases {
			b.Run(fmt.Sprintf("%s/%d", test.name, total), func(b *testing.B) {
				if test.name == "streamed" {
					index.vectors.budget = 1
					index.vectors.change("")()
					defer func() { index.vectors.budget = vectorBudget }()
				}
				query := protocol.DefaultKnowledgeQuery()
				query.Query, query.Mode, query.Limit = test.text, test.mode, knowledge.DefaultLimit
				for b.Loop() {
					page, err := index.Search(b.Context(), query, []Binding{binding}, embedding, false)
					if err != nil || len(page.Hits) != knowledge.DefaultLimit || page.Mode != test.mode {
						b.Fatalf("search: %d hits in mode %s, %v", len(page.Hits), page.Mode, err)
					}
				}
			})
		}
	}
}
