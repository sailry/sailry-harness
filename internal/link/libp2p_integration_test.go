package link

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/libp2p/go-libp2p"
	libp2pcrypto "github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/network"
	"github.com/libp2p/go-libp2p/core/peer"
	circuitrelay "github.com/libp2p/go-libp2p/p2p/protocol/circuitv2/relay"
	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type libp2pRelayCounter struct {
	connections atomic.Int64
	connects    atomic.Int64
}

func (*libp2pRelayCounter) AllowReserve(peer.ID, ma.Multiaddr) bool { return true }

func (counter *libp2pRelayCounter) AllowConnect(peer.ID, ma.Multiaddr, peer.ID) bool {
	counter.connects.Add(1)
	return true
}

func startLibp2pTestRelay(t *testing.T, seed byte) (string, *libp2pRelayCounter) {
	t.Helper()
	identity := libp2pTestIdentity(seed)
	secret := ed25519.NewKeyFromSeed(identity.seed[:])
	key, err := libp2pcrypto.UnmarshalEd25519PrivateKey(secret)
	if err != nil {
		t.Fatal(err)
	}
	host, err := libp2p.New(
		libp2p.Identity(key),
		libp2p.ListenAddrStrings("/ip4/127.0.0.1/tcp/0"),
		libp2p.DisableRelay(),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = host.Close() })
	counter := &libp2pRelayCounter{}
	host.Network().Notify(&network.NotifyBundle{
		ConnectedF: func(network.Network, network.Conn) { counter.connections.Add(1) },
	})
	service, err := circuitrelay.New(host,
		circuitrelay.WithACL(counter),
		circuitrelay.WithReservationAddressFilter(func(ma.Multiaddr) bool { return true }),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close() })
	if len(host.Addrs()) != 1 || !strings.HasPrefix(host.Addrs()[0].String(), "/ip4/127.0.0.1/tcp/") {
		t.Fatalf("relay did not bind only loopback TCP: %v", host.Addrs())
	}
	return fmt.Sprintf("%s/p2p/%s", host.Addrs()[0], host.ID()), counter
}

func libp2pTestIdentity(first byte) Identity {
	var identity Identity
	for index := range identity.seed {
		identity.seed[index] = first + byte(index)
	}
	return identity
}

type libp2pTestHandler struct {
	node          protocol.NodeID
	controller    protocol.NodeID
	calls         atomic.Int64
	subscriptions chan *libp2pQuietSubscription
}

func (handler *libp2pTestHandler) Dispatch(_ context.Context, caller protocol.NodeID, request protocol.Request) (Admission, error) {
	handler.calls.Add(1)
	if caller != handler.controller || request.Target != handler.node {
		return Admission{}, fmt.Errorf("unexpected certified caller or request target")
	}
	completed := make(chan Response, 1)
	completed <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
	return Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completed}, nil
}

func (handler *libp2pTestHandler) Subscribe(_ context.Context, caller protocol.NodeID, _ protocol.Topic) (Subscription, error) {
	if caller != handler.controller {
		return nil, fmt.Errorf("unexpected certified subscriber")
	}
	subscription := &libp2pQuietSubscription{closed: make(chan struct{})}
	handler.subscriptions <- subscription
	return subscription, nil
}

func (handler *libp2pTestHandler) Open(_ context.Context, caller protocol.NodeID, _ protocol.StreamID) (Stream, error) {
	if caller != handler.controller {
		return nil, fmt.Errorf("unexpected certified stream caller")
	}
	client, server := StreamPair()
	go func() {
		defer server.Close()
		data, err := io.ReadAll(server)
		if err == nil {
			_, _ = io.WriteString(server, "stored:"+string(data))
			_ = server.CloseWrite()
		}
	}()
	return client, nil
}

type libp2pQuietSubscription struct {
	closed chan struct{}
	once   sync.Once
}

func (subscription *libp2pQuietSubscription) Next(ctx context.Context) (protocol.Update, error) {
	<-ctx.Done()
	return protocol.Update{}, ctx.Err()
}

func (subscription *libp2pQuietSubscription) Close() error {
	subscription.once.Do(func() { close(subscription.closed) })
	return nil
}

func bindLibp2pTestNode(t *testing.T, ctx context.Context, scope NetworkScope, identity Identity, handler Handler) *Link {
	t.Helper()
	node, err := Bind(ctx, EndpointOptions{
		Identity: identity,
		Scope:    scope,
		Handler:  handler,
		Peers: &fixturePeerStore{
			trusted:   map[protocol.NodeID]bool{},
			addresses: map[protocol.NodeID]EndpointAddress{},
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = node.Close() })
	return node
}

func openLibp2pTestController(t *testing.T, ctx context.Context, scope NetworkScope, directory string) *Link {
	t.Helper()
	controller, err := OpenController(ctx, directory, scope)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	return controller
}

func libp2pAddressHints(t *testing.T, address EndpointAddress, transport string) []json.RawMessage {
	t.Helper()
	var selected []json.RawMessage
	for _, raw := range address.Addrs {
		var hint struct {
			Libp2p string `json:"Libp2p"`
		}
		if err := json.Unmarshal(raw, &hint); err != nil || hint.Libp2p == "" {
			continue
		}
		parsed, err := ma.NewMultiaddr(hint.Libp2p)
		if err != nil {
			t.Fatalf("invalid libp2p address %q: %v", hint.Libp2p, err)
		}
		_, last := ma.SplitLast(parsed)
		if last == nil || last.Protocol().Code == ma.P_P2P {
			t.Fatalf("address contains the target peer ID: %q", hint.Libp2p)
		}
		circuit := strings.Contains(hint.Libp2p, "/p2p-circuit")
		match := transport == "relay" && circuit ||
			transport == "tcp" && !circuit && strings.Contains(hint.Libp2p, "/tcp/") ||
			transport == "quic" && !circuit && strings.Contains(hint.Libp2p, "/quic-v1")
		if match {
			if !strings.HasPrefix(hint.Libp2p, "/ip4/127.0.0.1/") {
				t.Fatalf("non-loopback address in test: %q", hint.Libp2p)
			}
			selected = append(selected, append(json.RawMessage(nil), raw...))
		}
	}
	return selected
}

func waitLibp2pTestAddress(t *testing.T, ctx context.Context, node *Link, transport string) {
	t.Helper()
	ticker := time.NewTicker(25 * time.Millisecond)
	defer ticker.Stop()
	var address EndpointAddress
	var err error
	for {
		address, err = node.Address(ctx)
		if err == nil && len(libp2pAddressHints(t, address, transport)) != 0 {
			return
		}
		select {
		case <-ctx.Done():
			t.Fatalf("no %s address advertised: %v; last error: %v", transport, address.Addrs, err)
		case <-ticker.C:
		}
	}
}

func pairLibp2pTest(t *testing.T, ctx context.Context, node, controller *Link, transport, relayAddress string) EndpointAddress {
	t.Helper()
	waitLibp2pTestAddress(t, ctx, node, transport)
	invitation, err := node.Invite(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer invitation.Close()
	var ticket pairingTicket
	if err := json.Unmarshal([]byte(invitation.Ticket()), &ticket); err != nil {
		t.Fatal(err)
	}
	ticket.Address.Addrs = libp2pAddressHints(t, ticket.Address, transport)
	if len(ticket.Address.Addrs) == 0 {
		t.Fatalf("invitation did not contain a %s address", transport)
	}
	if transport == "relay" {
		for _, raw := range ticket.Address.Addrs {
			var hint struct{ Libp2p string }
			if err := json.Unmarshal(raw, &hint); err != nil || hint.Libp2p != relayAddress+"/p2p-circuit" {
				t.Fatalf("unexpected circuit address %s: %v", raw, err)
			}
		}
	}
	encoded, err := json.Marshal(ticket)
	if err != nil {
		t.Fatal(err)
	}
	address, err := controller.Pair(ctx, string(encoded))
	if err != nil {
		t.Fatalf("pair over %s: %v", transport, err)
	}
	if err := invitation.Paired(ctx); err != nil {
		t.Fatalf("pairing acknowledgment over %s: %v", transport, err)
	}
	if !invitation.IsPaired() || address.ID != ticket.Address.ID || controller.Direction(address.ID) != DirectionHost {
		t.Fatal("pairing did not record the authenticated host")
	}
	return address
}

func assertLibp2pTestRoute(t *testing.T, ctx context.Context, remote *Remote, expected Route) {
	t.Helper()
	watchCtx, cancel := context.WithCancel(ctx)
	changes := make(chan ConnectionInfo, 8)
	done := make(chan error, 1)
	go func() {
		done <- remote.WatchConnection(watchCtx, func(info ConnectionInfo) {
			select {
			case changes <- info:
			case <-watchCtx.Done():
			}
		})
	}()
	stopped := false
	defer func() {
		cancel()
		if !stopped {
			<-done
		}
	}()
	for {
		select {
		case info := <-changes:
			if info.Route != nil {
				if *info.Route != expected {
					t.Fatalf("selected route = %q, want %q", *info.Route, expected)
				}
				return
			}
		case err := <-done:
			stopped = true
			t.Fatalf("connection observer ended: %v", err)
		case <-ctx.Done():
			t.Fatalf("no selected %s route: %v", expected, ctx.Err())
		}
	}
}

func libp2pTestPeerID(t *testing.T, target protocol.NodeID) peer.ID {
	t.Helper()
	key, err := libp2pcrypto.UnmarshalEd25519PublicKey(target[:])
	if err != nil {
		t.Fatal(err)
	}
	peerID, err := peer.IDFromPublicKey(key)
	if err != nil {
		t.Fatal(err)
	}
	return peerID
}

func observeLibp2pTestDirectPaths(t *testing.T, controller *Link, target protocol.NodeID) *atomic.Int64 {
	t.Helper()
	endpoint, ok := controller.endpoint.(*libp2pEndpoint)
	if !ok {
		t.Fatal("Link did not use the libp2p endpoint")
	}
	peerID := libp2pTestPeerID(t, target)
	count := &atomic.Int64{}
	observer := &network.NotifyBundle{ConnectedF: func(_ network.Network, connection network.Conn) {
		if connection.RemotePeer() == peerID && !strings.Contains(connection.RemoteMultiaddr().String(), "/p2p-circuit") {
			count.Add(1)
		}
	}}
	endpoint.host.Network().Notify(observer)
	t.Cleanup(func() { endpoint.host.Network().StopNotify(observer) })
	return count
}

func assertLibp2pTestPhysicalPath(t *testing.T, controller *Link, target protocol.NodeID, transport string) {
	t.Helper()
	endpoint, ok := controller.endpoint.(*libp2pEndpoint)
	if !ok {
		t.Fatal("Link did not use the libp2p endpoint")
	}
	peerID := libp2pTestPeerID(t, target)
	paths := 0
	for _, connection := range endpoint.host.Network().ConnsToPeer(peerID) {
		if connection.IsClosed() {
			continue
		}
		paths++
		address := connection.RemoteMultiaddr().String()
		circuit := strings.Contains(address, "/p2p-circuit")
		match := transport == "relay" && circuit ||
			transport == "tcp" && !circuit && strings.Contains(address, "/tcp/") ||
			transport == "quic" && !circuit && strings.Contains(address, "/quic-v1")
		if !match {
			t.Fatalf("unexpected physical path for %s: %s", transport, address)
		}
	}
	if paths == 0 {
		t.Fatalf("no active physical %s path to the host", transport)
	}
}

func assertLibp2pTestReceipt(t *testing.T, ctx context.Context, remote *Remote, target protocol.NodeID) {
	t.Helper()
	request := protocol.NewRequest(target, protocol.Command{Kind: "snapshot"})
	admission, err := remote.Dispatch(ctx, request)
	if err != nil || admission.Receipt.ID != request.ID || admission.Receipt.Durable != request.Command.Durable() {
		t.Fatalf("command receipt = %#v, %v", admission.Receipt, err)
	}
	select {
	case response, ok := <-admission.Completion:
		if !ok || response.Fault != nil || response.Output.Kind != "notifications_updated" {
			t.Fatalf("command completion = %#v, open=%t", response, ok)
		}
	case <-ctx.Done():
		t.Fatalf("command completion: %v", ctx.Err())
	}
}

func assertLibp2pTestStream(t *testing.T, ctx context.Context, remote *Remote) {
	t.Helper()
	stream, err := remote.Open(ctx, protocol.NewStreamID())
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	result := make(chan struct {
		data []byte
		err  error
	}, 1)
	go func() {
		if _, err := io.WriteString(stream, "upload"); err != nil {
			result <- struct {
				data []byte
				err  error
			}{err: err}
			return
		}
		if err := stream.CloseWrite(); err != nil {
			result <- struct {
				data []byte
				err  error
			}{err: err}
			return
		}
		data, err := io.ReadAll(stream)
		result <- struct {
			data []byte
			err  error
		}{data, err}
	}()
	select {
	case value := <-result:
		if value.err != nil || string(value.data) != "stored:upload" {
			t.Fatalf("stream acknowledgment = %q, %v", value.data, value.err)
		}
	case <-ctx.Done():
		_ = stream.Close()
		t.Fatalf("stream acknowledgment: %v", ctx.Err())
	}
}

func assertLibp2pTestQuietRelease(t *testing.T, ctx context.Context, controller *Link, remote *Remote, handler *libp2pTestHandler, disconnect bool) {
	t.Helper()
	subscription, err := remote.Subscribe(ctx, protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	var served *libp2pQuietSubscription
	select {
	case served = <-handler.subscriptions:
	case <-ctx.Done():
		t.Fatalf("quiet subscription was not admitted: %v", ctx.Err())
	}
	if disconnect {
		controller.Disconnect(remote.Target())
	} else if err := subscription.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-served.closed:
	case <-ctx.Done():
		t.Fatalf("quiet subscription was not released, disconnect=%t", disconnect)
	}
}

func TestLibp2pLinkLoopbackRoutes(t *testing.T) {
	for _, test := range []struct {
		name  string
		route Route
	}{
		{name: "tcp", route: RouteP2P},
		{name: "quic", route: RouteP2P},
		{name: "relay", route: RouteRelay},
	} {
		t.Run(test.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 45*time.Second)
			defer cancel()
			relayAddress, relayCounter := startLibp2pTestRelay(t, 1)
			scope := NetworkScope{Bind: "127.0.0.1:0", Relays: []string{relayAddress}}
			nodeIdentity, controllerIdentity := libp2pTestIdentity(41), libp2pTestIdentity(81)
			handler := &libp2pTestHandler{
				node: nodeIdentity.ID(), controller: controllerIdentity.ID(),
				subscriptions: make(chan *libp2pQuietSubscription, 2),
			}
			node := bindLibp2pTestNode(t, ctx, scope, nodeIdentity, handler)
			parent := t.TempDir()
			if err := os.Chmod(parent, 0o700); err != nil {
				t.Fatal(err)
			}
			directory, lock, err := prepareControllerFiles(filepath.Join(parent, "controller"))
			if err != nil {
				t.Fatal(err)
			}
			if err := lock.Close(); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(directory, identityName), controllerIdentity.seed[:], 0o600); err != nil {
				t.Fatal(err)
			}
			controller := openLibp2pTestController(t, ctx, scope, directory)
			controllerAddress, err := controller.Address(ctx)
			if err != nil || controllerAddress.ID != controllerIdentity.ID() {
				t.Fatalf("controller identity = %v, %v", controllerAddress.ID, err)
			}
			directPaths := observeLibp2pTestDirectPaths(t, controller, nodeIdentity.ID())
			address := pairLibp2pTest(t, ctx, node, controller, test.name, relayAddress)
			assertLibp2pTestPhysicalPath(t, controller, nodeIdentity.ID(), test.name)
			if node.Direction(controllerIdentity.ID()) != DirectionController {
				t.Fatal("host did not acknowledge the paired controller")
			}
			remote := controller.Remote(address)
			assertLibp2pTestReceipt(t, ctx, remote, nodeIdentity.ID())
			assertLibp2pTestRoute(t, ctx, remote, test.route)
			assertLibp2pTestStream(t, ctx, remote)
			assertLibp2pTestQuietRelease(t, ctx, controller, remote, handler, false)
			assertLibp2pTestRoute(t, ctx, remote, test.route)
			assertLibp2pTestPhysicalPath(t, controller, nodeIdentity.ID(), test.name)
			if test.name == "relay" && (relayCounter.connects.Load() == 0 || directPaths.Load() != 0) {
				t.Fatal("relay route did not stay on a Circuit Relay connection")
			}
			if test.name != "relay" && relayCounter.connects.Load() != 0 {
				t.Fatal("direct route requested a Circuit Relay connection")
			}
			assertLibp2pTestQuietRelease(t, ctx, controller, remote, handler, true)
			if test.name == "relay" && directPaths.Load() != 0 {
				t.Fatal("relay subscription disconnect used a direct path")
			}

			if err := controller.Close(); err != nil {
				t.Fatal(err)
			}
			controller = openLibp2pTestController(t, ctx, scope, directory)
			restartedDirectPaths := observeLibp2pTestDirectPaths(t, controller, nodeIdentity.ID())
			restarted, err := controller.Address(ctx)
			if err != nil || restarted.ID != controllerIdentity.ID() {
				t.Fatalf("restarted controller identity = %v, %v", restarted.ID, err)
			}
			peers, err := controller.Peers(ctx)
			if err != nil || len(peers) != 1 || peers[0].ID != nodeIdentity.ID() {
				t.Fatalf("restarted controller peers = %v, %v", peers, err)
			}
			assertLibp2pTestReceipt(t, ctx, controller.Remote(peers[0]), nodeIdentity.ID())
			assertLibp2pTestRoute(t, ctx, controller.Remote(peers[0]), test.route)
			assertLibp2pTestPhysicalPath(t, controller, nodeIdentity.ID(), test.name)
			if test.name == "relay" && restartedDirectPaths.Load() != 0 {
				t.Fatal("restarted controller used a direct path instead of its saved circuit")
			}

			observation, err := controller.Remote(peers[0]).Subscribe(ctx, protocol.Topic{Kind: "node"})
			if err != nil {
				t.Fatal(err)
			}
			defer observation.Close()
			if err := node.SetTrust(ctx, controllerIdentity.ID(), false); err != nil {
				t.Fatal(err)
			}
			if _, err := observation.Next(ctx); faultValue(err).Code != protocol.ErrorCodePermissionDenied {
				t.Fatalf("revoked observation did not receive permission denied: %v", err)
			}
		})
	}
}

func TestLibp2pLinkRejectsUnpairedCaller(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	relayAddress, _ := startLibp2pTestRelay(t, 1)
	scope := NetworkScope{Bind: "127.0.0.1:0", Relays: []string{relayAddress}}
	nodeIdentity, controllerIdentity, attackerIdentity := libp2pTestIdentity(121), libp2pTestIdentity(161), libp2pTestIdentity(201)
	handler := &libp2pTestHandler{node: nodeIdentity.ID(), controller: controllerIdentity.ID()}
	node := bindLibp2pTestNode(t, ctx, scope, nodeIdentity, handler)
	controller := bindLibp2pTestNode(t, ctx, scope, controllerIdentity, nil)
	address := pairLibp2pTest(t, ctx, node, controller, "tcp", relayAddress)
	assertLibp2pTestReceipt(t, ctx, controller.Remote(address), nodeIdentity.ID())
	assertLibp2pTestRoute(t, ctx, controller.Remote(address), RouteP2P)
	assertLibp2pTestPhysicalPath(t, controller, nodeIdentity.ID(), "tcp")
	callsBefore := handler.calls.Load()
	if callsBefore != 1 {
		t.Fatalf("paired controller reached handler %d times", callsBefore)
	}
	attacker := bindLibp2pTestNode(t, ctx, scope, attackerIdentity, nil)
	if err := attacker.SetTrust(ctx, nodeIdentity.ID(), true); err != nil {
		t.Fatal(err)
	}
	if !attacker.Direction(nodeIdentity.ID()).AllowsOutbound() || node.Direction(attackerIdentity.ID()).Valid() {
		t.Fatal("test did not establish asymmetric trust")
	}
	request := protocol.NewRequest(nodeIdentity.ID(), protocol.Command{Kind: "snapshot"})
	if _, err := attacker.Remote(address).Dispatch(ctx, request); err == nil {
		t.Fatal("unpaired caller reached the Node")
	}
	wrongIdentity := libp2pTestIdentity(241).ID()
	if err := attacker.SetTrust(ctx, wrongIdentity, true); err != nil {
		t.Fatal(err)
	}
	wrongAddress := cloneAddress(address)
	wrongAddress.ID = wrongIdentity
	wrongRequest := protocol.NewRequest(wrongIdentity, protocol.Command{Kind: "snapshot"})
	if _, err := attacker.Remote(wrongAddress).Dispatch(ctx, wrongRequest); err == nil {
		t.Fatal("endpoint accepted an incorrect target identity")
	}
	if handler.calls.Load() != callsBefore {
		t.Fatal("unpaired or incorrectly identified caller reached the execution handler")
	}
}

func TestLibp2pPairingSelectionAndHostRelay(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	selectedRelay, selectedCounter := startLibp2pTestRelay(t, 1)
	hostRelay, hostCounter := startLibp2pTestRelay(t, 2)
	nodeIdentity, controllerIdentity := libp2pTestIdentity(41), libp2pTestIdentity(81)
	handler := &libp2pTestHandler{node: nodeIdentity.ID(), controller: controllerIdentity.ID()}
	node := bindLibp2pTestNode(t, ctx, NetworkScope{Bind: "127.0.0.1:0", Relays: []string{hostRelay}}, nodeIdentity, handler)
	controller := bindLibp2pTestNode(t, ctx, NetworkScope{Bind: "127.0.0.1:0"}, controllerIdentity, nil)
	controllerAddress, err := controller.Address(ctx)
	if err != nil || len(libp2pAddressHints(t, controllerAddress, "relay")) != 0 {
		t.Fatalf("controller unexpectedly advertised a relay: %v, %v", controllerAddress.Addrs, err)
	}
	waitLibp2pTestAddress(t, ctx, node, "relay")
	invitation, err := node.Invite(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer invitation.Close()
	var ticket pairingTicket
	if err := json.Unmarshal([]byte(invitation.Ticket()), &ticket); err != nil {
		t.Fatal(err)
	}
	ticket.Address.Addrs = libp2pAddressHints(t, ticket.Address, "relay")
	if len(ticket.Address.Addrs) != 1 {
		t.Fatalf("Host did not advertise exactly one relay: %v", ticket.Address.Addrs)
	}
	encoded, err := json.Marshal(ticket)
	if err != nil {
		t.Fatal(err)
	}
	var redemptions atomic.Int64
	const pin = "493243"
	exchange := httptest.NewServer(http.HandlerFunc(func(response http.ResponseWriter, request *http.Request) {
		var body struct {
			RequestID string `json:"request_id"`
			Code      string `json:"code"`
		}
		if request.Method != http.MethodPost || request.URL.Path != "/v1/codes/redeem" ||
			json.NewDecoder(request.Body).Decode(&body) != nil || body.Code != pin || !validPairingRequestID(body.RequestID) {
			http.Error(response, "invalid redemption", http.StatusBadRequest)
			return
		}
		redemptions.Add(1)
		_ = json.NewEncoder(response).Encode(map[string]any{
			"ticket": string(encoded), "expires_at_ms": invitation.ExpiresAtMS(),
		})
	}))
	t.Cleanup(exchange.Close)
	baselineConnections, baselineCircuits := hostCounter.connections.Load(), hostCounter.connects.Load()
	selection := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{selectedRelay}}}
	var claim PairingClaim
	if _, err := controller.PairPINOnNetwork(ctx, exchange.URL, pin, selection, &claim); faultValue(err).Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("explicit selection accepted the Host's different relay: %v", err)
	}
	if redemptions.Load() != 1 || invitation.IsPaired() || node.Direction(controllerIdentity.ID()).Valid() ||
		hostCounter.connections.Load() != baselineConnections || hostCounter.connects.Load() != baselineCircuits ||
		selectedCounter.connections.Load() != 0 || selectedCounter.connects.Load() != 0 {
		t.Fatal("explicit selection contacted a relay or paired the Node")
	}
	directPaths := observeLibp2pTestDirectPaths(t, controller, nodeIdentity.ID())
	address, err := controller.PairPIN(ctx, exchange.URL, pin, &claim)
	if err != nil {
		t.Fatalf("pairing through the Host's relay: %v", err)
	}
	if err := invitation.Paired(ctx); err != nil {
		t.Fatalf("Host relay pairing acknowledgment: %v", err)
	}
	peers, err := node.Peers(ctx)
	if err != nil || len(peers) != 1 || peers[0].ID != controllerIdentity.ID() || len(libp2pAddressHints(t, peers[0], "relay")) != 0 {
		t.Fatalf("Host did not save the claimant's relay-free address: %v, %v", peers, err)
	}
	if current := controller.NetworkSelection(); current != nil {
		t.Fatalf("Host route changed the controller's network selection: %v", current)
	}
	remote := controller.Remote(address)
	assertLibp2pTestReceipt(t, ctx, remote, nodeIdentity.ID())
	assertLibp2pTestRoute(t, ctx, remote, RouteRelay)
	assertLibp2pTestPhysicalPath(t, controller, nodeIdentity.ID(), "relay")
	if redemptions.Load() != 2 || hostCounter.connects.Load() <= baselineCircuits || directPaths.Load() != 0 {
		t.Fatal("pairing did not stay on the Host's relay")
	}
	invitation.Close()

	// An explicit Direct pairing must not write its claim on the already-open
	// Circuit Relay path, even when the new ticket advertises a direct address.
	directOffer, err := node.Invite(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer directOffer.Close()
	if _, err := controller.pairSelected(ctx, directOffer.Ticket(), []string{}); faultValue(err).Code != protocol.ErrorCodeUnavailable {
		t.Fatalf("explicit Direct pairing was not rejected on the existing circuit: %v", err)
	}
	if directOffer.IsPaired() {
		t.Fatal("explicit Direct pairing sent a claim on the existing circuit")
	}
	assertLibp2pTestReceipt(t, ctx, remote, nodeIdentity.ID())
	assertLibp2pTestPhysicalPath(t, controller, nodeIdentity.ID(), "relay")
}
