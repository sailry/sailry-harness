package link

import (
	"bytes"
	"context"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func TestPairingDirectionDoesNotGrantReverseExecution(t *testing.T) {
	t.Parallel()
	peer := protocol.NodeID{7}
	link := newFixtureLink(t)
	if err := link.pairPeer(t.Context(), peer, DirectionController); err != nil {
		t.Fatal(err)
	}
	if got := link.Direction(peer); got != DirectionController || !got.AllowsInbound() || got.AllowsOutbound() {
		t.Fatalf("inviter relationship: %q", got)
	}
	if _, _, err := link.connection(t.Context(), EndpointAddress{ID: peer}, true); err == nil || faultValue(err).Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("controller was usable as an execution host: %v", err)
	}
	if err := link.pairPeer(t.Context(), peer, DirectionHost); err != nil {
		t.Fatal(err)
	}
	if got := link.Direction(peer); got != DirectionBoth {
		t.Fatalf("opposite independently paired relationship: %q", got)
	}
	if err := link.SetDirection(t.Context(), peer, DirectionHost); err != nil {
		t.Fatal(err)
	}
	if got := link.Direction(peer); got != DirectionHost || got.AllowsInbound() || !got.AllowsOutbound() {
		t.Fatalf("claimant relationship: %q", got)
	}
	if err := link.SetTrust(t.Context(), peer, true); err != nil || link.Direction(peer) != DirectionHost {
		t.Fatalf("retaining trust widened its direction: %q, %v", link.Direction(peer), err)
	}
	if err := link.SetDirection(t.Context(), protocol.NodeID{8}, DirectionBoth); err == nil {
		t.Fatal("changing direction paired an unknown peer")
	}
	if err := link.SetDirection(t.Context(), peer, "invalid"); err == nil || link.Direction(peer) != DirectionHost {
		t.Fatalf("invalid direction changed trust: %v", err)
	}
}

func TestHostPeerCannotCallLocalExecution(t *testing.T) {
	t.Parallel()
	peer := protocol.NodeID{7}
	request := protocol.NewRequest(protocol.NodeID{1}, protocol.Command{Kind: "snapshot"})
	topic := protocol.Topic{Kind: "node"}
	resource := protocol.NewStreamID()
	for _, call := range []wire.Call{
		{Kind: wire.CallCommand, Request: &request},
		{Kind: wire.CallSubscribe, Topic: &topic},
		{Kind: wire.CallOpenStream, Stream: &resource},
		{Kind: wire.CallInspect},
	} {
		t.Run(string(call.Kind), func(t *testing.T) {
			link := newFixtureLink(t)
			if err := link.pairPeer(t.Context(), peer, DirectionHost); err != nil {
				t.Fatal(err)
			}
			// No handler method may be reached for the host-only peer.
			link.handler = fixtureHandler{}
			var incoming bytes.Buffer
			if err := wire.Write(&incoming, link.envelope(call), false); err != nil {
				t.Fatal(err)
			}
			stream := newFixtureStream(incoming.Bytes())
			var compression atomic.Bool
			if err := link.serveRequest(t.Context(), peer, stream, make(chan struct{}, 1), &compression); err != nil {
				t.Fatal(err)
			}
			var reply wire.Reply
			if err := wire.Read(bytes.NewReader(stream.written()), &reply, false); err != nil {
				t.Fatal(err)
			}
			if call.Kind == wire.CallInspect {
				if reply.Kind != wire.ReplyEndpoint {
					t.Fatalf("paired endpoint metadata unavailable: %#v", reply)
				}
			} else if reply.Kind != wire.ReplyRejected || reply.Rejected == nil || reply.Rejected.Code != protocol.ErrorCodePermissionDenied {
				t.Fatalf("host-only peer reached local execution: %#v", reply)
			}
		})
	}
}

func TestNarrowingDirectionReleasesExecutionConnection(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	peer := protocol.NodeID{7}
	if err := link.pairPeer(t.Context(), peer, DirectionBoth); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(link.ctx)
	defer cancel()
	slot := &outgoingSlot{ctx: ctx, cancel: cancel}
	link.outgoing[peer] = slot
	if err := link.SetDirection(t.Context(), peer, DirectionController); err != nil {
		t.Fatal(err)
	}
	if ctx.Err() == nil || link.outgoing[peer] != nil || !link.isTrusted(peer) {
		t.Fatal("narrowing retained an outbound execution slot or revoked the whole pairing")
	}
	if err := link.SetTrust(t.Context(), peer, false); err != nil {
		t.Fatal(err)
	}
	if link.isTrusted(peer) || link.Direction(peer) != "" {
		t.Fatal("revocation retained a direction")
	}
}

func TestUncertainDirectionChangeReconcilesPersistedAccess(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	peer := protocol.NodeID{7}
	if err := link.pairPeer(t.Context(), peer, DirectionBoth); err != nil {
		t.Fatal(err)
	}
	store := link.store.(*fixturePeerStore)
	store.trustOutcome = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture sync uncertain"}
	if err := link.SetDirection(t.Context(), peer, DirectionHost); err == nil || faultValue(err).Code != protocol.ErrorCodeOutcomeUnknown {
		t.Fatalf("uncertain narrowing: %v", err)
	}
	if link.Direction(peer) != DirectionHost {
		t.Fatal("uncertain committed narrowing retained inbound access")
	}
}
