package link

import (
	"context"
	"errors"
	"fmt"
	"slices"

	"github.com/libp2p/go-libp2p/core/network"
	"github.com/libp2p/go-libp2p/core/peer"
	lpprotocol "github.com/libp2p/go-libp2p/core/protocol"
	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

const (
	libp2pNodeALPN        = wire.NodeALPN
	libp2pPairALPN        = wire.PairALPN
	libp2pMaxOpenStreams  = 2 * maxConnections * maxStreams
	libp2pMaxIncomingNode = maxConnections - maxPairings
)

func libp2pProtocol(alpn string) lpprotocol.ID { return lpprotocol.ID("/" + alpn) }

// Incoming Node streams share the Link's compression state per peer, not per
// physical connection. Pairing uses separate, single-stream lifetimes.
type libp2pConnection struct {
	endpoint *libp2pEndpoint
	peer     peer.ID
	id       protocol.NodeID
	alpn     string
	incoming bool
	ctx      context.Context
	cancel   context.CancelCauseFunc
	accepts  chan peertransport.Stream

	// Protected by endpoint.mu.
	path    network.Conn // A Pair connection is pinned once its only stream opens.
	route   string
	changed chan struct{}
	closed  *peertransport.CloseReason
	opened  bool
	streams int
	lanWake chan []string // nil requests fresh listeners over the circuit
}

var _ peertransport.Connection = (*libp2pConnection)(nil)

func (endpoint *libp2pEndpoint) Connect(ctx context.Context, address peertransport.Address, alpn string) (peertransport.Connection, error) {
	if alpn != libp2pNodeALPN && alpn != libp2pPairALPN {
		return nil, invalidLink("unsupported libp2p Link protocol")
	}
	info, err := libp2pAddressInfo(address)
	if err != nil {
		return nil, err
	}
	if info.ID == endpoint.host.ID() {
		return nil, invalidLink("invalid libp2p Link peer")
	}
	endpoint.mu.Lock()
	if endpoint.ctx.Err() != nil {
		endpoint.mu.Unlock()
		return nil, peertransport.ErrClosed
	}
	if endpoint.pendingDials >= 2*maxConnections {
		endpoint.mu.Unlock()
		return nil, libp2pCapacity()
	}
	endpoint.pendingDials++
	endpoint.peerWork.Add(1)
	endpoint.mu.Unlock()
	defer func() {
		endpoint.mu.Lock()
		endpoint.pendingDials--
		endpoint.mu.Unlock()
		endpoint.peerWork.Done()
	}()
	revokeDial := endpoint.gater.authorizeDial(info)
	defer revokeDial()
	dial, cancel := context.WithTimeout(ctx, ioTimeout)
	stop := context.AfterFunc(endpoint.ctx, cancel)
	defer func() { stop(); cancel() }()
	if err := endpoint.host.Connect(network.WithAllowLimitedConn(dial, "Sailry Link relay fallback"), info); err != nil {
		return nil, fmt.Errorf("connect libp2p peer: %w", err)
	}
	if err := dial.Err(); err != nil {
		return nil, err
	}
	endpoint.mu.Lock()
	defer endpoint.mu.Unlock()
	if endpoint.ctx.Err() != nil {
		return nil, peertransport.ErrClosed
	}
	var authenticated bool
	for _, path := range endpoint.host.Network().ConnsToPeer(info.ID) {
		if path.IsClosed() {
			continue
		}
		id, ok := libp2pNodeID(path)
		if ok && path.RemotePeer() == info.ID && id == address.ID {
			authenticated = true
			break
		}
	}
	if !authenticated {
		return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "libp2p peer identity could not be verified"}
	}
	conn := endpoint.addConnectionLocked(info.ID, address.ID, alpn, false, nil)
	if conn == nil {
		return nil, libp2pCapacity()
	}
	return conn, nil
}

func libp2pCapacity() error {
	return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "libp2p Link capacity exhausted"}
}

func (endpoint *libp2pEndpoint) addConnectionLocked(p peer.ID, id protocol.NodeID, alpn string, incoming bool, path network.Conn) *libp2pConnection {
	var backlog int
	switch {
	case incoming && alpn == libp2pNodeALPN:
		if len(endpoint.incomingNodes) >= libp2pMaxIncomingNode {
			return nil
		}
		backlog = maxStreams
	case incoming:
		if endpoint.incomingPairs >= maxPairings {
			return nil
		}
		endpoint.incomingPairs++
		backlog = 1
	case alpn == libp2pNodeALPN:
		if endpoint.outgoingNodes == 2*maxConnections {
			return nil
		}
		endpoint.outgoingNodes++
	default:
		if endpoint.outgoingPairs == maxPairings {
			return nil
		}
		endpoint.outgoingPairs++
	}
	ctx, cancel := context.WithCancelCause(endpoint.ctx)
	conn := &libp2pConnection{endpoint: endpoint, peer: p, id: id, alpn: alpn, incoming: incoming, path: path,
		ctx: ctx, cancel: cancel, changed: make(chan struct{}),
	}
	conn.route = conn.preferredRouteLocked()
	endpoint.connections[conn] = struct{}{}
	if incoming {
		conn.accepts = make(chan peertransport.Stream, backlog)
		if alpn == libp2pNodeALPN {
			endpoint.incomingNodes[p] = conn
		}
	}
	return conn
}

func (endpoint *libp2pEndpoint) receive(raw network.Stream) {
	id, ok := libp2pNodeID(raw.Conn())
	if !ok {
		_ = raw.ResetWithError(network.StreamProtocolViolation)
		return
	}
	if !endpoint.queueIncoming(raw, id) {
		_ = raw.ResetWithError(network.StreamResourceLimitExceeded)
	}
}

func (endpoint *libp2pEndpoint) queueIncoming(raw network.Stream, id protocol.NodeID) bool {
	endpoint.mu.Lock()
	defer endpoint.mu.Unlock()
	if endpoint.ctx.Err() != nil || raw.Conn().IsClosed() || endpoint.streams >= libp2pMaxOpenStreams {
		return false
	}
	p := raw.Conn().RemotePeer()
	var conn *libp2pConnection
	switch raw.Protocol() {
	case libp2pProtocol(libp2pNodeALPN):
		conn = endpoint.incomingNodes[p]
	case libp2pProtocol(libp2pPairALPN):
	default:
		return false
	}
	first := conn == nil
	if first {
		if raw.Protocol() == libp2pProtocol(libp2pNodeALPN) {
			conn = endpoint.addConnectionLocked(p, id, libp2pNodeALPN, true, nil)
		} else {
			conn = endpoint.addConnectionLocked(p, id, libp2pPairALPN, true, raw.Conn())
		}
	}
	if conn == nil || !conn.admitStreamLocked() {
		if first && conn != nil {
			conn.closeLocked("local", 0, "Link stream not admitted")
		}
		return false
	}
	conn.accepts <- newLibp2pStream(raw, conn.ctx, conn.releaseStream)
	if first {
		endpoint.accepts = append(endpoint.accepts, conn)
		close(endpoint.acceptReady)
		endpoint.acceptReady = make(chan struct{})
	}
	return true
}

func (endpoint *libp2pEndpoint) Accept(ctx context.Context) (peertransport.Connection, error) {
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		endpoint.mu.Lock()
		if endpoint.ctx.Err() != nil {
			endpoint.mu.Unlock()
			return nil, peertransport.ErrClosed
		}
		if len(endpoint.accepts) > 0 {
			conn := endpoint.accepts[0]
			endpoint.accepts = slices.Delete(endpoint.accepts, 0, 1)
			endpoint.mu.Unlock()
			return conn, nil
		}
		ready := endpoint.acceptReady
		endpoint.mu.Unlock()
		select {
		case <-ctx.Done():
		case <-endpoint.ctx.Done():
		case <-ready:
		}
	}
}

func (conn *libp2pConnection) AcceptBi(ctx context.Context) (peertransport.Stream, error) {
	if !conn.incoming {
		return nil, invalidLink("outgoing libp2p Link connections do not accept streams")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-conn.ctx.Done():
		return nil, peertransport.ErrClosed
	case stream := <-conn.accepts:
		if conn.ctx.Err() != nil {
			_ = stream.Close()
			return nil, peertransport.ErrClosed
		}
		return stream, nil
	}
}

func (conn *libp2pConnection) OpenBi(ctx context.Context) (peertransport.Stream, error) {
	if conn.incoming {
		return nil, invalidLink("incoming libp2p Link connections do not open streams")
	}
	return conn.openBi(ctx)
}

func (conn *libp2pConnection) openBi(ctx context.Context) (peertransport.Stream, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	endpoint := conn.endpoint
	endpoint.mu.Lock()
	conn.refreshPathLocked()
	if conn.ctx.Err() != nil {
		endpoint.mu.Unlock()
		return nil, peertransport.ErrClosed
	}
	if conn.alpn == libp2pPairALPN && conn.opened {
		endpoint.mu.Unlock()
		return nil, invalidLink("libp2p pairing uses one stream")
	}
	if !conn.admitStreamLocked() {
		endpoint.mu.Unlock()
		return nil, libp2pCapacity()
	}
	conn.opened = true
	endpoint.mu.Unlock()

	opening, cancel := context.WithTimeout(ctx, ioTimeout)
	stop := context.AfterFunc(conn.ctx, cancel)
	defer func() { stop(); cancel() }()
	// Swarm prefers a direct connection, with a limited circuit as fallback.
	// The stream stays on its original path once opened; it is never replayed.
	opts := network.WithAllowLimitedConn(network.WithNoDial(opening, "existing Sailry Link session"), "Sailry Link relay fallback")
	raw, err := endpoint.host.NewStream(opts, conn.peer, libp2pProtocol(conn.alpn))
	if err != nil {
		conn.releaseStream()
		return nil, err
	}
	owned := false
	defer func() {
		if !owned {
			_ = raw.Reset()
			conn.releaseStream()
		}
	}()
	if id, ok := libp2pNodeID(raw.Conn()); !ok || id != conn.id || raw.Conn().RemotePeer() != conn.peer {
		return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "libp2p stream peer identity could not be verified"}
	}
	endpoint.mu.Lock()
	defer endpoint.mu.Unlock()
	if err := opening.Err(); err != nil {
		return nil, err
	}
	if conn.ctx.Err() != nil || raw.Conn().IsClosed() {
		return nil, peertransport.ErrClosed
	}
	if conn.alpn == libp2pPairALPN {
		conn.path = raw.Conn()
		conn.refreshPathLocked()
	}
	owned = true
	return newLibp2pStream(raw, conn.ctx, conn.releaseStream), nil
}

func (conn *libp2pConnection) admitStreamLocked() bool {
	endpoint := conn.endpoint
	if conn.ctx.Err() != nil || endpoint.streams >= libp2pMaxOpenStreams || conn.streams >= maxStreams {
		return false
	}
	conn.streams++
	endpoint.streams++
	endpoint.peerWork.Add(1)
	return true
}

func (conn *libp2pConnection) releaseStream() {
	endpoint := conn.endpoint
	endpoint.mu.Lock()
	conn.streams--
	endpoint.streams--
	endpoint.mu.Unlock()
	endpoint.peerWork.Done()
}

func libp2pRouteOf(path network.Conn) string {
	if path.IsClosed() {
		return ""
	}
	if _, err := path.RemoteMultiaddr().ValueForProtocol(ma.P_CIRCUIT); err == nil {
		return "relay"
	}
	return "p2p"
}

func (conn *libp2pConnection) preferredRouteLocked() string {
	if conn.path != nil {
		return libp2pRouteOf(conn.path)
	}
	var route string
	for _, path := range conn.endpoint.host.Network().ConnsToPeer(conn.peer) {
		switch libp2pRouteOf(path) {
		case "p2p":
			return "p2p"
		case "relay":
			route = "relay"
		}
	}
	return route
}

func (conn *libp2pConnection) refreshPathLocked() {
	if conn.closed != nil {
		return
	}
	route := conn.preferredRouteLocked()
	if route == "" {
		conn.closeLocked("transport", 0, "all libp2p paths closed")
		return
	}
	if conn.route != route {
		conn.route = route
		if route == "relay" {
			conn.wakeLANLocked(nil)
		}
		close(conn.changed)
		conn.changed = make(chan struct{})
	}
}

func (endpoint *libp2pEndpoint) pathChanged(p peer.ID, lost network.Conn) {
	endpoint.mu.Lock()
	defer endpoint.mu.Unlock()
	if lost != nil {
		for relay := range endpoint.reservations {
			if relay.leg == lost {
				relay.leg = nil
				relay.refresh()
				endpoint.notifyReservationsLocked()
			}
		}
	}
	for conn := range endpoint.connections {
		if conn.peer == p {
			conn.refreshPathLocked()
		}
	}
}

func (conn *libp2pConnection) Info(ctx context.Context) (peertransport.ConnectionInfo, error) {
	if err := ctx.Err(); err != nil {
		return peertransport.ConnectionInfo{}, err
	}
	conn.endpoint.mu.Lock()
	defer conn.endpoint.mu.Unlock()
	conn.refreshPathLocked()
	info := peertransport.ConnectionInfo{RemoteID: conn.id, ALPN: conn.alpn, Route: conn.route}
	if conn.closed != nil {
		reason := *conn.closed
		info.Closed = &reason
	}
	return info, nil
}

func (conn *libp2pConnection) WaitPath(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	conn.endpoint.mu.Lock()
	previous := conn.route
	conn.refreshPathLocked()
	changed, closed, current := conn.changed, conn.closed != nil, conn.route
	conn.endpoint.mu.Unlock()
	if closed {
		return peertransport.ErrClosed
	}
	if previous != current {
		return nil
	}
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-conn.ctx.Done():
		return peertransport.ErrClosed
	case <-changed:
		return nil
	}
}

func (conn *libp2pConnection) Close(code uint64, reason string) error {
	conn.endpoint.mu.Lock()
	conn.closeLocked("local", code, reason)
	conn.endpoint.mu.Unlock()
	return nil
}

// DiscardPaths removes reusable physical paths after Link's liveness check fails.
// Closing an ordinary logical connection does not invalidate shared paths.
func (conn *libp2pConnection) DiscardPaths() error {
	endpoint := conn.endpoint
	endpoint.mu.Lock()
	var paths []network.Conn
	if conn.path != nil {
		paths = []network.Conn{conn.path}
	} else {
		paths = endpoint.host.Network().ConnsToPeer(conn.peer)
	}
	endpoint.mu.Unlock()

	var failure error
	for _, path := range paths {
		failure = errors.Join(failure, path.Close())
	}
	return failure
}

func (conn *libp2pConnection) closeLocked(kind string, code uint64, reason string) {
	endpoint := conn.endpoint
	if conn.closed != nil {
		return
	}
	conn.closed = &peertransport.CloseReason{Kind: kind, Code: code, Reason: reason}
	conn.route = ""
	var cause error
	if kind == "local" && code == peerRevokedCloseCode && conn.incoming && conn.alpn == libp2pNodeALPN {
		cause = errLibp2pAccessRevoked
	}
	conn.cancel(cause)
	close(conn.changed)
	delete(endpoint.connections, conn)
	if conn.incoming {
		endpoint.accepts = slices.DeleteFunc(endpoint.accepts, func(queued *libp2pConnection) bool { return queued == conn })
		for range len(conn.accepts) {
			select {
			case <-conn.accepts:
			default:
			}
		}
		if conn.alpn == libp2pNodeALPN {
			delete(endpoint.incomingNodes, conn.peer)
		} else {
			endpoint.incomingPairs--
		}
	} else if conn.alpn == libp2pNodeALPN {
		endpoint.outgoingNodes--
	} else {
		endpoint.outgoingPairs--
	}
}
