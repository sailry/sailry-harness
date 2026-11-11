//go:build integration

package link

import (
	"context"
	"crypto/rand"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

// TestSilentPairingConnectionsLeaveAcceptWorkers binds real loopback endpoints.
// Run it with SAILRY_IROH_LIB naming the built iroh library.
func TestSilentPairingConnectionsLeaveAcceptWorkers(t *testing.T) {
	if err := iroh.LoadDefault(true); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 2*time.Minute)
	defer cancel()
	identity := func() Identity {
		value, err := OpenIdentity(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	peers := func(trusted ...protocol.NodeID) *fixturePeerStore {
		store := &fixturePeerStore{trusted: map[protocol.NodeID]bool{}, addresses: map[protocol.NodeID]EndpointAddress{}}
		for _, peer := range trusted {
			store.trusted[peer] = true
		}
		return store
	}
	loopback := NetworkScope{Bind: "127.0.0.1:0"}
	nodeIdentity, controllerIdentity := identity(), identity()
	unavailable := &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "fixture handler"}
	node, err := Bind(ctx, EndpointOptions{
		Identity: nodeIdentity, Scope: loopback, Peers: peers(controllerIdentity.ID()),
		Handler: fixtureHandler{
			dispatch: func(context.Context, protocol.NodeID, protocol.Request) (Admission, error) {
				return Admission{}, unavailable
			},
			subscribe: func(context.Context, protocol.NodeID, protocol.Topic) (Subscription, error) {
				return nil, unavailable
			},
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Close()
	address, err := node.Address(ctx)
	if err != nil {
		t.Fatal(err)
	}
	// Each silent stranger keeps its pairing connection until the Node closes it.
	crowd := func() {
		for range maxConnections {
			var seed [32]byte
			if _, err := rand.Read(seed[:]); err != nil {
				t.Fatal(err)
			}
			stranger, err := iroh.NewEndpoint(ctx, iroh.Config{SecretKey: seed, Mode: "direct", Bind: "127.0.0.1:0"})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = stranger.Close() })
			go func() { _, _ = stranger.Connect(ctx, address, wire.PairALPN) }()
		}
		time.Sleep(2 * time.Second)
	}
	// Every inspection dials again, so each one needs a free accept worker.
	inspect := func(scenario string) {
		controller, err := Bind(ctx, EndpointOptions{Identity: controllerIdentity, Scope: loopback, Peers: peers(nodeIdentity.ID())})
		if err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		started := time.Now()
		if _, err := controller.Inspect(ctx, address); err != nil {
			t.Fatalf("%s: paired controller could not inspect the Node: %v", scenario, err)
		}
		if elapsed := time.Since(started); elapsed > 3*time.Second {
			t.Fatalf("%s: paired controller waited %v for an accept worker", scenario, elapsed)
		}
	}
	crowd()
	inspect("no invitation")
	invitation, err := node.Invite(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer invitation.Close()
	crowd()
	inspect("open invitation")
}
