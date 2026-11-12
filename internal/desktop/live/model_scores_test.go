package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
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
		r.readModelScores()
		r.workers.Wait()
		pluginViewRunQueue(r)
		if got := reads(); got != attempt {
			t.Fatalf("after projection %d the scores were read %d times", attempt, got)
		}
	}
}
