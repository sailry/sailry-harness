package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A score read that failed is made again by the next projection.
func TestModelScoresReadAgain(t *testing.T) {
	t.Parallel()
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	snapshot.Providers = []protocol.ConversationProvider{{
		ID: protocol.ProviderID{4}, Revision: 1, Name: "Fixture", Enabled: true, Api: "responses", Authentication: "api_key",
		Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}},
	}}
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "read_model_scores" {
			return link.Response{}, false
		}
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture has no scores"}}, true
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r := &Runtime{ctx: t.Context(), controller: controller, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
	reads := func() (total int) {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		for _, request := range transport.requests {
			if request.Command.Kind == "read_model_scores" {
				total++
			}
		}
		return total
	}
	for attempt := 1; attempt <= 2; attempt++ {
		snapshot, source := r.modelInputs()
		r.readModelScores(source, snapshot.Providers)
		r.workers.Wait()
		pluginViewRunQueue(r)
		if got := reads(); got != attempt {
			t.Fatalf("after projection %d the scores were read %d times", attempt, got)
		}
	}
}

func TestModelProjectionReusesCatalog(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = protocol.NodeID{1}
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Revision, provider.Enabled = protocol.ProviderID{4}, 1, true
		provider.Name, provider.Api, provider.Authentication = "Fixture", "responses", "api_key"
		provider.Models = []protocol.ConversationModel{{ID: "first"}}
		snapshot.Providers = []protocol.ConversationProvider{provider}
		r, source := snapshotFixture(t, snapshot)
		source.respond = func(protocol.Request) link.Response {
			return link.Response{Output: protocol.Output{Kind: "model_scores", Data: protocol.DefaultConversationCatalogScores()}}
		}
		tester := pluginUpdateQueueTester(r)
		project := func() {
			t.Helper()
			r.projectModels()
			synctest.Wait()
			tester.Frame()
			synctest.Wait()
			if len(r.nodeModels) != 1 || len(r.App.Data.Models) != 1 {
				t.Fatalf("expected one projected model, got %v", r.App.Data.Models)
			}
		}
		refresh := func() {
			t.Helper()
			if err := r.controller.Refresh(t.Context()); err != nil {
				t.Fatal(err)
			}
			synctest.Wait()
		}
		project()
		first := &r.nodeModels[0]
		source.snapshot.Cursor++
		refresh()
		r.App.Data = &viewmodel.Data{}
		project()
		if &r.App.Data.Models[0] != first || len(source.sent("read_model_scores")) != 1 {
			t.Fatal("an unchanged catalog rebuilt its model list or reread scores")
		}

		// The snapshot can backfill model metadata at the saved provider's
		// existing revision.
		changed := provider
		changed.Models = []protocol.ConversationModel{{ID: "second"}}
		source.snapshot.Providers = []protocol.ConversationProvider{changed}
		source.snapshot.Cursor++
		refresh()
		project()
		if r.nodeModels[0].ID != modelKey(provider.ID, "second") || len(source.sent("read_model_scores")) != 2 {
			t.Fatal("a changed model catalog was reused at the same provider revision")
		}
	})
}

func TestModelProjectionUsesEffortHintForEncodedIDs(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = protocol.NodeID{1}
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Revision, provider.Enabled = protocol.ProviderID{4}, 1, true
		provider.Name, provider.Api, provider.Authentication = "Fixture", "responses", "api_key"
		provider.Models = []protocol.ConversationModel{
			{ID: "agy-cli/claude-sonnet-5-5-low", DefaultEffort: protocol.Effort{Kind: "default"}},
			{ID: "agy-cli/claude-sonnet-5-5-high", DefaultEffort: protocol.Effort{Kind: "default"}},
		}
		snapshot.Providers = []protocol.ConversationProvider{provider}
		r, source := snapshotFixture(t, snapshot)
		source.respond = func(req protocol.Request) link.Response {
			if req.Command.Kind != "read_model_scores" {
				return link.Response{}
			}
			return link.Response{Output: protocol.Output{Kind: "model_scores", Data: protocol.ConversationCatalogScores{
				Scores: []protocol.ConversationCatalogScoresModelScore{
					{
						Provider: provider.ID,
						Model:    "agy-cli/claude-sonnet-5-5-low",
						Efforts: []protocol.ConversationCatalogScoresEffortScore{
							{Effort: protocol.Effort{Kind: "low"}, Overall: 61.5},
							{Effort: protocol.Effort{Kind: "high"}, Overall: 84.2},
						},
					},
					{
						Provider: provider.ID,
						Model:    "agy-cli/claude-sonnet-5-5-high",
						Efforts: []protocol.ConversationCatalogScoresEffortScore{
							{Effort: protocol.Effort{Kind: "low"}, Overall: 61.5},
							{Effort: protocol.Effort{Kind: "high"}, Overall: 84.2},
						},
					},
				},
			}}}
		}
		tester := pluginUpdateQueueTester(r)
		r.projectModels()
		synctest.Wait()
		tester.Frame()
		synctest.Wait()
		if len(r.nodeModels) != 2 || r.nodeModels[0].Score == nil || r.nodeModels[1].Score == nil {
			t.Fatalf("projected models missing scores: %+v", r.nodeModels)
		}
		if float32(r.nodeModels[0].Score.Overall) != float32(61.5) || float32(r.nodeModels[1].Score.Overall) != float32(84.2) {
			t.Fatalf("effort-hint scores = (%.2f, %.2f), want (61.5, 84.2)", r.nodeModels[0].Score.Overall, r.nodeModels[1].Score.Overall)
		}
	})
}
