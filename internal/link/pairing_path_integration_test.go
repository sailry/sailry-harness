//go:build integration

package link

import (
	"context"
	"testing"
	"time"

	"github.com/libp2p/go-libp2p/core/network"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestExplicitRelayPairingPreservesLiveStreams(t *testing.T) {
	for _, initial := range []string{"relay", "tcp"} {
		t.Run(initial, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
			defer cancel()
			relay, counter := startLibp2pTestRelay(t, 1)
			nodeID, controllerID := libp2pTestIdentity(41), libp2pTestIdentity(81)
			handler := &libp2pTestHandler{node: nodeID.ID(), controller: controllerID.ID()}
			node := bindLibp2pTestNode(t, ctx, NetworkScope{Bind: "127.0.0.1:0", Relays: []string{relay}}, nodeID, handler)
			controller := bindLibp2pTestNode(t, ctx, NetworkScope{Bind: "127.0.0.1:0"}, controllerID, nil)
			waitLibp2pTestAddress(t, ctx, node, "relay")
			address := pairLibp2pTest(t, ctx, node, controller, initial, relay)
			remote := controller.Remote(address)
			var circuit *libp2pStream
			if initial == "relay" {
				circuit = openLANTestStream(t, ctx, remote, nodeID.ID(), false)
				if _, err := circuit.WriteContext(ctx, []byte("before:")); err != nil {
					t.Fatal(err)
				}
				// Establish a second physical path without requiring a private LAN.
				all, err := node.Address(ctx)
				if err != nil {
					t.Fatal(err)
				}
				all.Addrs = libp2pAddressHints(t, all, "tcp")
				direct, err := libp2pAddressInfo(all)
				if err != nil {
					t.Fatal(err)
				}
				ep := controller.endpoint.(*libp2pEndpoint)
				if err := ep.host.Connect(network.WithForceDirectDial(ctx, "test second path"), direct); err != nil {
					t.Fatal(err)
				}
			}
			direct := openLANTestStream(t, ctx, remote, nodeID.ID(), true)
			if _, err := direct.WriteContext(ctx, []byte("before:")); err != nil {
				t.Fatal(err)
			}
			offer, err := node.Invite(ctx)
			if err != nil {
				t.Fatal(err)
			}
			defer offer.Close()
			received := make(chan string, 1)
			ep := node.endpoint.(*libp2pEndpoint)
			ep.host.SetStreamHandler(libp2pProtocol(libp2pPairALPN), func(raw network.Stream) {
				received <- libp2pRouteOf(raw.Conn())
				ep.receive(raw)
			})
			priorCircuits := counter.connects.Load()
			_, err = controller.pairSelected(ctx, offer.Ticket(), []string{relay})
			if circuit == nil {
				if err == nil || faultValue(err).Code != protocol.ErrorCodeUnavailable || offer.IsPaired() {
					t.Fatalf("pairing without a matching physical path: %v, paired=%t", err, offer.IsPaired())
				}
			} else {
				if err != nil {
					t.Fatalf("pair over the existing selected circuit: %v", err)
				}
				if err := offer.Paired(ctx); err != nil {
					t.Fatal(err)
				}
				if got := <-received; got != "relay" {
					t.Fatalf("Host received the pairing stream over %s", got)
				}
				finishLANTestStream(t, ctx, circuit, "relay", "before:relay")
			}
			if counter.connects.Load() != priorCircuits {
				t.Fatal("pairing unexpectedly dialed a new circuit")
			}
			finishLANTestStream(t, ctx, direct, "direct", "before:direct")
		})
	}
}
