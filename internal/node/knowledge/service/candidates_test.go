package service

import (
	"context"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestStorageCandidatesCarryOnlySourceIdentity(t *testing.T) {
	fixture := newHarness(t)
	fixture.install()
	model := localmodels.DefaultEmbedding
	collection := protocol.DefaultKnowledgeCollection()
	collection.ID, collection.Revision, collection.Progress.Generation = protocol.NewKnowledgeID(), 1, 1
	collection.Definition.Name = "Personal memory"
	collection.Definition.Model = &model
	collection.Definition.Source = protocol.KnowledgeSource{Kind: "storage", Data: &protocol.KnowledgeSourceStoragePayload{Scope: protocol.PluginStorageScopeNode}}
	stored := store.KnowledgeStored{Collection: collection}
	query := protocol.DefaultPluginStorageSearch()
	text := "vector"
	query.Query, query.Mode = &text, protocol.KnowledgeModeSemantic
	request := fixture.request("read_knowledge_candidates", &query)
	fixture.service.options.Plan = func(context.Context, protocol.Request) (store.SemanticPlan, error) {
		return store.SemanticPlan{Collections: []store.KnowledgeStored{stored}, Eligible: map[string]bool{"entry/record": true}}, nil
	}
	fixture.service.options.Choice = func() protocol.ModelsLocalChoice { return protocol.ModelsLocalChoice{Enabled: true, Model: &model} }
	binding := fixture.service.binding(stored, fixture.service.options.Choice())
	if err := fixture.service.index.Bind(t.Context(), binding); err != nil {
		t.Fatal(err)
	}
	revision := "1111111111111111111111111111111111111111111111111111111111111111"
	vector, err := knowledge.Quantize([]float32{1, 0})
	if err != nil {
		t.Fatal(err)
	}
	if err := fixture.service.index.Replace(t.Context(), binding, Document{Locator: "entry/record", Revision: revision, Embedded: true}, []Part{{Title: "Private title", Text: "Private contents", Vector: &vector}}); err != nil {
		t.Fatal(err)
	}
	candidates, err := fixture.service.StorageCandidates(t.Context(), request)
	if err != nil || len(candidates.Candidates) != 1 || candidates.Candidates[0].Key != "entry/record" || candidates.Candidates[0].Revision != revision || !candidates.Available {
		t.Fatalf("candidate source identity: %+v: %v", candidates, err)
	}
}
