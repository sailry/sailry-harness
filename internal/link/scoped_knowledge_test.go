package link

import (
	"bytes"
	"context"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

type scopedKnowledgeFixtureHandler struct {
	fixtureHandler
	scoped func(context.Context, protocol.NodeID, protocol.ScopedKnowledgeCall) (Admission, error)
}

func (handler scopedKnowledgeFixtureHandler) ScopedKnowledge(ctx context.Context, peer protocol.NodeID, call protocol.ScopedKnowledgeCall) (Admission, error) {
	return handler.scoped(ctx, peer, call)
}

func TestScopedKnowledgeChannelUsesOrdinaryInboundIdentity(t *testing.T) {
	for _, scenario := range []string{"controller", "shell", "reverse_executor", "unscoped", "ordinary_command"} {
		t.Run(scenario, func(t *testing.T) {
			service := newFixtureLink(t)
			peer := protocol.NodeID{2}
			direction, hasNode := DirectionController, true
			if scenario == "reverse_executor" {
				direction = DirectionHost
			}
			if scenario == "shell" {
				hasNode = false
			}
			if err := service.pairPeer(t.Context(), peer, direction); err != nil {
				t.Fatal(err)
			}
			scope := protocol.DefaultPluginContext()
			request := protocol.NewRequest(protocol.NodeID{3}, protocol.Command{Kind: "list_knowledge_collections"})
			request.Plugin = &scope
			if scenario == "unscoped" {
				request.Plugin = nil
			}
			if scenario == "ordinary_command" {
				request.Command = protocol.Command{Kind: "snapshot"}
			}
			called := false
			service.handler = scopedKnowledgeFixtureHandler{scoped: func(_ context.Context, authenticated protocol.NodeID, received protocol.ScopedKnowledgeCall) (Admission, error) {
				called = true
				if authenticated != peer || received.Request.ID != request.ID || received.Request.Target != request.Target {
					t.Fatal("scoped knowledge changed the actor or storage request")
				}
				done := make(chan Response, 1)
				done <- Response{Output: protocol.Output{Kind: "knowledge_collections", Data: []protocol.KnowledgeCollection{}}}
				close(done)
				return Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: done}, nil
			}}
			call := protocol.ScopedKnowledgeCall{Request: request}
			envelope := service.envelope(wire.Call{Kind: wire.CallScopedKnowledge, ScopedKnowledge: &call})
			envelope.HasNode = &hasNode
			var frames bytes.Buffer
			if err := wire.Write(&frames, envelope, false); err != nil {
				t.Fatal(err)
			}
			stream := newFixtureStream(frames.Bytes())
			var compression atomic.Bool
			if err := service.serveRequest(t.Context(), peer, stream, make(chan struct{}, 1), &compression); err != nil {
				t.Fatal(err)
			}
			var first wire.Reply
			if err := wire.Read(bytes.NewReader(stream.written()), &first, false); err != nil {
				t.Fatal(err)
			}
			allowed := scenario == "controller" || scenario == "shell"
			if called != allowed || allowed && (first.Kind != wire.ReplyReceipt || first.Receipt.ID != request.ID) ||
				!allowed && first.Kind != wire.ReplyRejected {
				t.Fatalf("scoped knowledge boundary: called=%v reply=%+v", called, first)
			}
		})
	}
}
