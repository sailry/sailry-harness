package link

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

type resourceFixtureHandler struct {
	fixtureHandler
	resource func(context.Context, protocol.NodeID, protocol.ResourceCall) (Admission, error)
}

func (handler resourceFixtureHandler) Resource(ctx context.Context, peer protocol.NodeID, call protocol.ResourceCall) (Admission, error) {
	return handler.resource(ctx, peer, call)
}

func TestServerResourceChannelUsesDirectExecutionIdentity(t *testing.T) {
	for _, scenario := range []string{"direct_executor", "shell", "controller", "ordinary_operation"} {
		t.Run(scenario, func(t *testing.T) {
			service := newFixtureLink(t)
			peer := protocol.NodeID{2}
			direction, hasNode := DirectionHost, true
			if scenario == "controller" {
				direction = DirectionController
			}
			if scenario == "shell" {
				hasNode = false
			}
			if err := service.pairPeer(t.Context(), peer, direction); err != nil {
				t.Fatal(err)
			}
			called := false
			request := protocol.NewRequest(protocol.NodeID{3}, protocol.Command{Kind: "list_knowledge_collections"})
			if scenario == "ordinary_operation" {
				request.Command = protocol.Command{Kind: "snapshot"}
			}
			service.handler = resourceFixtureHandler{resource: func(_ context.Context, authenticated protocol.NodeID, received protocol.ResourceCall) (Admission, error) {
				called = true
				if authenticated != peer || received.Request.ID != request.ID || received.Request.Target != request.Target {
					t.Fatal("resource caller or captured request changed")
				}
				completed := make(chan Response, 1)
				completed <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
				return Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completed}, nil
			}}
			call := protocol.ResourceCall{Request: request}
			envelope := service.envelope(wire.Call{Kind: wire.CallResource, Resource: &call})
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
			if scenario == "direct_executor" || scenario == "controller" {
				if !called || first.Kind != wire.ReplyReceipt || first.Receipt.ID != request.ID {
					t.Fatalf("trusted executor was not admitted: %#v", first)
				}
			} else if called || first.Kind != wire.ReplyRejected {
				t.Fatalf("resource boundary was bypassed: called=%v, reply=%#v", called, first)
			}
		})
	}
}

func TestRelayedRepliesPreserveFramesAndPerHopCompression(t *testing.T) {
	for _, sourceCompressed := range []bool{false, true} {
		for _, destinationCompressed := range []bool{false, true} {
			t.Run(fmt.Sprintf("%v_to_%v", sourceCompressed, destinationCompressed), func(t *testing.T) {
				service := newFixtureLink(t)
				receipt := protocol.Receipt{ID: protocol.NewRequestID(), Durable: true}
				pages := strings.Repeat("snapshot-payload", wire.PageBytes/16+128)
				frames := []wire.Reply{
					{Kind: wire.ReplyReceipt, Receipt: &receipt},
					{Kind: wire.ReplySnapshotStart, SnapshotStart: &wire.SnapshotHeader{
						Delivery: wire.DeliveryCompleted, Node: protocol.NodeID{3}, Cursor: 42, Bytes: uint64(len(pages)),
					}},
				}
				for index, start := uint64(0), 0; start < len(pages); index++ {
					end := min(start+wire.PageBytes, len(pages))
					frames = append(frames, wire.Reply{Kind: wire.ReplySnapshotPage, SnapshotPage: &wire.SnapshotPage{Index: index, Data: pages[start:end]}})
					start = end
				}
				var upstream bytes.Buffer
				var rawFrames []json.RawMessage
				for _, frame := range frames {
					raw, err := json.Marshal(frame)
					if err != nil {
						t.Fatal(err)
					}
					if frame.Kind == wire.ReplyReceipt {
						raw = append(raw[:len(raw)-1], []byte(`,"future_receipt":{"retained":true}}`)...)
					}
					rawFrames = append(rawFrames, raw)
					if err := wire.Write(&upstream, raw, sourceCompressed && frame.Kind != wire.ReplyReceipt); err != nil {
						t.Fatal(err)
					}
				}
				incoming, outgoing := newFixtureStream(nil), newFixtureStream(upstream.Bytes())
				if err := service.relayReplies(t.Context(), incoming, outgoing, destinationCompressed, sourceCompressed); err != nil {
					t.Fatal(err)
				}
				if !incoming.finished {
					t.Fatal("completed snapshot did not half-close the relay")
				}
				reader, payload := wire.NewReader(destinationCompressed), bytes.NewReader(incoming.written())
				for _, want := range rawFrames {
					got, err := reader.Raw(payload)
					if err != nil || !bytes.Equal(got, want) {
						t.Fatalf("relay changed a frame: %s %v", got, err)
					}
				}
				if payload.Len() != 0 {
					t.Fatal("relay duplicated snapshot frames")
				}
			})
		}
	}
}

func TestRelayRejectsMalformedPagesAndLeavesLostCompletionUnknown(t *testing.T) {
	for _, scenario := range []string{"orphan", "wrong_index", "empty", "interrupted", "missing_completion"} {
		t.Run(scenario, func(t *testing.T) {
			service := newFixtureLink(t)
			start := wire.Reply{Kind: wire.ReplySnapshotStart, SnapshotStart: &wire.SnapshotHeader{Delivery: wire.DeliveryCompleted, Node: protocol.NodeID{3}, Bytes: 3}}
			page := wire.Reply{Kind: wire.ReplySnapshotPage, SnapshotPage: &wire.SnapshotPage{Index: 0, Data: "abc"}}
			frames := []wire.Reply{start}
			switch scenario {
			case "orphan":
				frames = []wire.Reply{page}
			case "wrong_index":
				page.SnapshotPage.Index = 1
				frames = append(frames, page)
			case "empty":
				page.SnapshotPage.Data = ""
				frames = append(frames, page)
			case "interrupted":
				frames = append(frames, wire.Reply{Kind: wire.ReplyCompleted, Completed: &wire.Response{Output: &protocol.Output{Kind: "notifications_updated"}}})
			case "missing_completion":
				receipt := protocol.Receipt{ID: protocol.NewRequestID(), Durable: true}
				frames = []wire.Reply{{Kind: wire.ReplyReceipt, Receipt: &receipt}}
			}
			incoming := newFixtureStream(nil)
			if err := service.relayReplies(t.Context(), incoming, newFixtureStream(fixtureFrames(t, frames...)), false, false); err == nil {
				t.Fatal("malformed or truncated completion was reported as complete")
			}
			if incoming.finished {
				t.Fatal("unknown outcome was reported by a clean relay completion")
			}
		})
	}
}

func TestRelayOnlyServesOwnShellAndMatchingTarget(t *testing.T) {
	for _, scenario := range []string{"desktop", "wrong_target", "reverse_resource"} {
		t.Run(scenario, func(t *testing.T) {
			service := newFixtureLink(t)
			shell, target := protocol.NodeID{2}, protocol.NodeID{3}
			if err := service.pairPeer(t.Context(), shell, DirectionController); err != nil {
				t.Fatal(err)
			}
			if err := service.pairPeer(t.Context(), target, DirectionHost); err != nil {
				t.Fatal(err)
			}
			service.peerKinds.Store(shell, scenario == "desktop")
			request := protocol.NewRequest(target, protocol.Command{Kind: "snapshot"})
			envelope := service.envelope(wire.Call{Kind: wire.CallCommand, Request: &request})
			envelope.Target = &target
			if scenario == "wrong_target" {
				request.Target = protocol.NodeID{9}
			}
			if scenario == "reverse_resource" {
				envelope.Call = wire.Call{Kind: wire.CallResource}
			}
			stream := newFixtureStream(nil)
			if err := service.relay(t.Context(), shell, stream, envelope, false); err != nil {
				t.Fatal(err)
			}
			var reply wire.Reply
			if err := wire.Read(bytes.NewReader(stream.written()), &reply, false); err != nil || reply.Kind != wire.ReplyRejected {
				t.Fatalf("invalid route reached a target: %#v %v", reply, err)
			}
		})
	}
}
