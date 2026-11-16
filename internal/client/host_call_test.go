package client

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostCallKeepsOriginalTargetAndIdentity(t *testing.T) {
	entry, target := protocol.NodeID{41}, protocol.NodeID{42}
	var sent protocol.Request
	controller := New(knowledgeRouteTransport{clientTransport: &clientTransport{}, target: entry, entry: entry, sent: &sent})
	t.Cleanup(func() { _ = controller.Close() })
	request := protocol.NewRequest(target, protocol.Command{Kind: "call_host_handler", Data: protocol.HostHandlerCall{
		Handler: "save", Input: map[string]any{"id": "retained"},
	}})
	if _, err := controller.ExecuteHostCall(t.Context(), request); err != nil {
		t.Fatal(err)
	}
	if sent.ID != request.ID || sent.Target != target || sent.Command.Kind != request.Command.Kind {
		t.Fatalf("host route rewrote prepared request: %+v -> %+v", request, sent)
	}
	if _, err := controller.ExecuteHostCall(t.Context(), protocol.NewRequest(target, protocol.Command{Kind: "snapshot"})); err == nil {
		t.Fatal("host helper accepted an arbitrary command")
	}
}
