package client

import (
	"context"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type scopedKnowledgeTransport struct {
	knowledgeRouteTransport
	calls *[]protocol.ScopedKnowledgeCall
}

func (transport scopedKnowledgeTransport) ScopedKnowledge(_ context.Context, call protocol.ScopedKnowledgeCall) (link.Admission, error) {
	*transport.calls = append(*transport.calls, call)
	output := protocol.Output{Kind: "knowledge_collections", Data: []protocol.KnowledgeCollection{}}
	if call.Original != nil {
		output = protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{
			ID: call.Original.ID, Outcome: protocol.RequestOutcome{Kind: "not_admitted"},
		}}
	}
	done := make(chan link.Response, 1)
	done <- link.Response{Output: output}
	close(done)
	return link.Admission{Receipt: protocol.Receipt{ID: call.Request.ID, Durable: call.Request.Command.Durable()}, Completion: done}, nil
}

func TestScopedKnowledgeKeepsExecutionEntryAndStorageRequest(t *testing.T) {
	execution, storage := protocol.NodeID{2}, protocol.NodeID{3}
	var calls []protocol.ScopedKnowledgeCall
	var commands []protocol.Request
	value := New(scopedKnowledgeTransport{
		knowledgeRouteTransport: knowledgeRouteTransport{clientTransport: &clientTransport{}, target: execution, entry: protocol.NodeID{1}, sent: &commands},
		calls:                   &calls,
	})
	t.Cleanup(func() { _ = value.Close() })
	scope := protocol.DefaultPluginContext()
	request := protocol.NewRequest(storage, protocol.Command{Kind: "delete_knowledge_collection", Data: protocol.CommandDeleteKnowledgeCollectionPayload{ID: protocol.NewKnowledgeID()}})
	request.Plugin = &scope
	if _, err := value.ExecuteScopedKnowledge(t.Context(), request); err != nil {
		t.Fatal(err)
	}
	if outcome, err := value.ScopedKnowledgeOutcome(t.Context(), request); err != nil || outcome.Kind != "not_admitted" {
		t.Fatalf("scoped recovery: %+v: %v", outcome, err)
	}
	if len(commands) != 0 || len(calls) != 2 || calls[0].Request.Target != storage || calls[0].Request.ID != request.ID ||
		calls[1].Original == nil || calls[1].Original.ID != request.ID || calls[1].Request.Target != storage {
		t.Fatalf("scoped call used an ordinary storage route or rewrote its request: commands=%+v calls=%+v", commands, calls)
	}
}
