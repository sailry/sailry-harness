package link

import (
	"context"
	"crypto/ed25519"
	"fmt"
	"log/slog"
	"slices"
	"sync"

	"github.com/libp2p/go-libp2p"
	lpcrypto "github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/host"
	"github.com/libp2p/go-libp2p/core/network"
	"github.com/libp2p/go-libp2p/core/peer"
	"github.com/libp2p/go-libp2p/p2p/metricshelper"
	"github.com/libp2p/go-libp2p/p2p/protocol/holepunch"
	quic "github.com/libp2p/go-libp2p/p2p/transport/quic"
	"github.com/libp2p/go-libp2p/p2p/transport/tcp"
	ma "github.com/multiformats/go-multiaddr"
	manet "github.com/multiformats/go-multiaddr/net"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type libp2pEndpoint struct {
	host   host.Host
	id     protocol.NodeID
	ctx    context.Context
	cancel context.CancelFunc
	gater  libp2pRelayGater
	notify network.NotifyBundle

	mu                sync.Mutex
	connections       map[*libp2pConnection]struct{}
	incomingNodes     map[peer.ID]*libp2pConnection
	incomingPairs     int
	outgoingNodes     int
	outgoingPairs     int
	pendingDials      int
	accepts           []*libp2pConnection
	acceptReady       chan struct{}
	streams           int
	peerWork          sync.WaitGroup
	activeRelays      []*libp2pRelay
	reservations      map[*libp2pRelay]struct{}
	reservationsReady chan struct{}
	relayWork         sync.WaitGroup
	relayUpdate       chan struct{}

	closeOnce sync.Once
	closeErr  error
}

var _ peertransport.Endpoint = (*libp2pEndpoint)(nil)

func newLibp2pEndpoint(ctx context.Context, secret [32]byte, bind string, relays []string) (peertransport.Endpoint, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	listen, err := libp2pListeners(bind)
	if err != nil {
		return nil, err
	}
	specs, err := parseLibp2pRelays(relays)
	if err != nil {
		return nil, err
	}
	key, err := lpcrypto.UnmarshalEd25519PrivateKey(ed25519.NewKeyFromSeed(secret[:]))
	if err != nil {
		return nil, fmt.Errorf("load libp2p identity: %w", err)
	}
	pub, err := key.GetPublic().Raw()
	if err != nil {
		return nil, fmt.Errorf("load libp2p public identity: %w", err)
	}
	peerID, err := peer.IDFromPublicKey(key.GetPublic())
	if err != nil {
		return nil, fmt.Errorf("load libp2p peer ID: %w", err)
	}
	for _, spec := range specs {
		if spec.info.ID == peerID {
			return nil, invalidLink("a libp2p endpoint cannot use itself as a relay")
		}
	}
	lifetime, cancel := context.WithCancel(ctx)
	endpoint := &libp2pEndpoint{
		id: protocol.NodeID(pub), ctx: lifetime, cancel: cancel,
		connections: make(map[*libp2pConnection]struct{}), incomingNodes: make(map[peer.ID]*libp2pConnection),
		acceptReady: make(chan struct{}), reservations: make(map[*libp2pRelay]struct{}),
		reservationsReady: make(chan struct{}), relayUpdate: make(chan struct{}, 1),
	}
	endpoint.gater.rules.Store(&libp2pRelayRules{active: specs, reserving: specs})
	h, err := libp2p.New(
		libp2p.Identity(key), libp2p.ListenAddrs(listen...),
		libp2p.Transport(tcp.NewTCPTransport), libp2p.Transport(quic.NewTransport),
		libp2p.EnableRelay(), libp2p.ConnectionGater(&endpoint.gater),
		libp2p.DisableMetrics(), libp2p.EnableHolePunching(holepunch.WithMetricsTracer(libp2pHolePunchMetrics{})),
	)
	if err != nil {
		cancel()
		return nil, fmt.Errorf("start libp2p Link endpoint: %w", err)
	}
	endpoint.host = h
	var hasTCP, hasQUIC bool
	for _, addr := range h.Network().ListenAddresses() {
		switch metricshelper.GetTransport(addr) {
		case "tcp":
			hasTCP = true
		case "quic-v1":
			hasQUIC = true
		}
	}
	if !hasTCP || !hasQUIC {
		cancel()
		_ = h.Close()
		return nil, invalidLink("libp2p requires both TCP and QUIC listeners on the bind address")
	}
	if err := lifetime.Err(); err != nil {
		cancel()
		_ = h.Close()
		return nil, err
	}
	endpoint.notify = network.NotifyBundle{
		ConnectedF:    func(_ network.Network, c network.Conn) { endpoint.pathChanged(c.RemotePeer(), nil) },
		DisconnectedF: func(_ network.Network, c network.Conn) { endpoint.pathChanged(c.RemotePeer(), c) },
	}
	h.Network().Notify(&endpoint.notify)
	h.SetStreamHandler(libp2pProtocol(libp2pNodeALPN), endpoint.receive)
	h.SetStreamHandler(libp2pProtocol(libp2pPairALPN), endpoint.receive)
	endpoint.mu.Lock()
	for _, spec := range specs {
		endpoint.activeRelays = append(endpoint.activeRelays, endpoint.startRelayLocked(spec))
	}
	endpoint.mu.Unlock()
	return endpoint, nil
}

func (endpoint *libp2pEndpoint) Address(ctx context.Context) (peertransport.Address, error) {
	if err := ctx.Err(); err != nil {
		return peertransport.Address{}, err
	}
	endpoint.mu.Lock()
	if endpoint.ctx.Err() != nil {
		endpoint.mu.Unlock()
		return peertransport.Address{}, peertransport.ErrClosed
	}
	relays := liveLibp2pRelays(endpoint.activeRelays)
	endpoint.mu.Unlock()
	// Resolve wildcard listeners against current interfaces, rather than
	// waiting for the host's periodic address update after a network change.
	local, _ := manet.ResolveUnspecifiedAddresses(endpoint.host.Network().ListenAddresses(), nil)
	direct := append(local, endpoint.host.Addrs()...)
	slices.SortFunc(direct, ma.Multiaddr.Compare)
	return encodeLibp2pAddress(endpoint.id, relays, direct), nil
}

func (endpoint *libp2pEndpoint) Online(ctx context.Context) error {
	_, err := endpoint.readyRelays(ctx)
	return err
}

func (endpoint *libp2pEndpoint) NetworkChange(ctx context.Context) error {
	endpoint.mu.Lock()
	for _, relay := range endpoint.activeRelays {
		relay.refresh()
	}
	endpoint.mu.Unlock()
	return endpoint.Online(ctx)
}

func (endpoint *libp2pEndpoint) Close() error {
	endpoint.closeOnce.Do(func() {
		endpoint.cancel()
		endpoint.mu.Lock()
		for conn := range endpoint.connections {
			conn.closeLocked("local", 0, "Link endpoint closed")
		}
		endpoint.mu.Unlock()
		endpoint.host.Network().StopNotify(&endpoint.notify)
		endpoint.closeErr = endpoint.host.Close()
		endpoint.relayWork.Wait()
		endpoint.peerWork.Wait()
	})
	return endpoint.closeErr
}

type libp2pHolePunchMetrics struct{}

func (libp2pHolePunchMetrics) DirectDialFinished(ok bool) {
	slog.Debug("libp2p direct dial before DCUtR", "success", ok)
}

func (libp2pHolePunchMetrics) HolePunchFinished(side string, attempts int, _, _ []ma.Multiaddr, direct network.ConnMultiaddrs) {
	transport := "none"
	if direct != nil {
		transport = metricshelper.GetTransport(direct.RemoteMultiaddr())
	}
	slog.Debug("libp2p DCUtR result", "side", side, "attempts", attempts, "success", direct != nil, "transport", transport)
}
