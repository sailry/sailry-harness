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

func TestScopedKnowledgeKeepsExactExecutionRequestAndReceipt(t *testing.T) {
	execution := protocol.NodeID{2}
	var calls []protocol.ScopedKnowledgeCall
	var command protocol.Request
	value := New(scopedKnowledgeTransport{
		knowledgeRouteTransport: knowledgeRouteTransport{clientTransport: &clientTransport{}, target: execution, entry: protocol.NodeID{1}, sent: &command},
		calls:                   &calls,
	})
	t.Cleanup(func() { _ = value.Close() })
	scope := protocol.DefaultPluginContext()
	request := protocol.NewRequest(execution, protocol.Command{Kind: "delete_knowledge_collection", Data: protocol.CommandDeleteKnowledgeCollectionPayload{ID: protocol.NewKnowledgeID()}})
	request.Plugin = &scope
	if _, err := value.ExecuteScopedKnowledge(t.Context(), request); err != nil {
		t.Fatal(err)
	}
	if outcome, err := value.ScopedKnowledgeOutcome(t.Context(), request); err != nil || outcome.Kind != "not_admitted" {
		t.Fatalf("scoped recovery: %+v: %v", outcome, err)
	}
	if command.ID != (protocol.RequestID{}) || len(calls) != 2 || calls[0].Request.Target != execution || calls[0].Request.ID != request.ID ||
		calls[1].Original == nil || calls[1].Original.ID != request.ID || calls[1].Request.Target != execution {
		t.Fatalf("scoped call used an ordinary storage route or rewrote its request: command=%+v calls=%+v", command, calls)
	}
	request.Target = protocol.NodeID{3}
	if _, err := value.ExecuteScopedKnowledge(t.Context(), request); faultCode(err) != protocol.ErrorCodeWrongTarget {
		t.Fatalf("scoped execution accepted a different Node: %v", err)
	}
	if _, err := value.ScopedKnowledgeOutcome(t.Context(), request); faultCode(err) != protocol.ErrorCodeWrongTarget {
		t.Fatalf("scoped inspection accepted a different Node: %v", err)
	}
	if len(calls) != 2 {
		t.Fatalf("foreign requests reached the transport: %+v", calls)
	}
}
