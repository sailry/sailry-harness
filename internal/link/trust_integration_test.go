//go:build integration

package link

import (
	"context"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func TestPairedHostCannotReverseCallDesktop(t *testing.T) {
	if err := iroh.LoadDefault(true); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), time.Minute)
	defer cancel()
	endpoint := func() *Link {
		identity, err := OpenIdentity(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		value, err := Bind(ctx, EndpointOptions{
			Identity: identity, Scope: NetworkScope{Bind: "127.0.0.1:0"},
			Peers: &fixturePeerStore{trusted: map[protocol.NodeID]bool{}, addresses: map[protocol.NodeID]EndpointAddress{}},
			Handler: fixtureHandler{dispatch: func(_ context.Context, _ protocol.NodeID, request protocol.Request) (Admission, error) {
				completed := make(chan Response, 1)
				completed <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
				return Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completed}, nil
			}},
		})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = value.Close() })
		return value
	}
	host, desktop := endpoint(), endpoint()
	invitation, err := host.Invite(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer invitation.Close()
	hostAddress, err := desktop.Pair(ctx, invitation.Ticket())
	if err != nil {
		t.Fatal(err)
	}
	desktopAddress, err := desktop.Address(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if desktop.Direction(hostAddress.ID) != DirectionHost || host.Direction(desktopAddress.ID) != DirectionController {
		t.Fatal("pairing did not preserve the inviter and claimant directions")
	}
	admitted, err := desktop.Remote(hostAddress).Dispatch(ctx, protocol.NewRequest(hostAddress.ID, protocol.Command{Kind: "snapshot"}))
	if err != nil {
		t.Fatal(err)
	}
	if completed := <-admitted.Completion; completed.Fault != nil {
		t.Fatal(completed.Fault)
	}
	// Bypass the caller's direction guard to verify that an authenticated,
	// hostile host is also refused at the Desktop's service boundary.
	connection, err := host.endpoint.Connect(ctx, desktopAddress, wire.NodeALPN)
	if err != nil {
		t.Fatal(err)
	}
	defer connection.Close(0, "fixture finished")
	stream, err := connection.OpenBi(ctx)
	if err != nil {
		t.Fatal(err)
	}
	request := protocol.NewRequest(desktopAddress.ID, protocol.Command{Kind: "snapshot"})
	if _, err := host.Remote(desktopAddress).dispatchStream(ctx, request, stream, false); err == nil || faultValue(err).Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("host reverse call reached the Desktop: %v", err)
	}
}
