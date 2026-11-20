package link

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"maps"
	"runtime"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

const maxConnections = 16
const maxTasks = 64
const maxStreams = 64
const ioTimeout = 10 * time.Second

// livenessTimeout bounds the one inspection that verifies a cached connection
// after a network change. A connection left half-open by device sleep or NAT
// rebinding still reports itself open and would otherwise hold its streams
// until the QUIC idle timeout.
const livenessTimeout = 3 * time.Second

const peerRevokedCloseCode = 1

type EndpointAddress = peertransport.Address

type NetworkScope struct {
	Bind     string
	Internet bool
	Relays   []string
}

type EndpointOptions struct {
	Identity        Identity
	Scope           NetworkScope
	Handler         Handler
	Peers           PeerStore
	SoftwareVersion string
	Name            string
	// Paired runs after an outbound invitation has been accepted and saved.
	Paired func(protocol.NodeID)
}

type PeerInfo struct {
	Execution       bool
	Name            *string
	Platform        string
	SoftwareVersion *string
	Protocol        *uint16
	Latency         time.Duration
	Zstd            bool
	HasNode         *bool
}

func (info PeerInfo) Compatibility() error {
	return CheckHost(info.Protocol, info.SoftwareVersion)
}

type Link struct {
	nodeID          protocol.NodeID
	endpoint        peertransport.Endpoint
	ctx             context.Context
	cancel          context.CancelFunc
	handler         Handler
	store           PeerStore
	controller      *controllerStore
	softwareVersion string
	mu              sync.RWMutex
	name            *string
	trusted         map[protocol.NodeID]bool
	directions      map[protocol.NodeID]PeerDirection
	peerKinds       sync.Map // NodeID -> bool, learned through authenticated exchange
	addresses       map[protocol.NodeID]EndpointAddress
	outgoing        map[protocol.NodeID]*outgoingSlot
	removalNotices  map[protocol.NodeID]*HostRemovalNotice
	incoming        map[peertransport.Connection]protocol.NodeID
	trustUpdate     sync.Mutex
	relayUpdate     chan struct{}
	selection       *protocol.NetworkSelection
	peerChanged     chan struct{}
	paired          func(protocol.NodeID)
	offerMu         sync.Mutex
	offer           *pairingOffer
	work            chan func()
	workMu          sync.Mutex
	workStopped     bool
	workers         sync.WaitGroup
	closeOnce       sync.Once
	closeErr        error
	now             func() time.Time
	timeout         func(context.Context, time.Duration) (context.Context, context.CancelFunc)
	ticks           func() (<-chan time.Time, func())
}

type outgoingSlot struct {
	dial       chan struct{}
	mu         sync.Mutex
	ctx        context.Context
	cancel     context.CancelFunc
	users      int
	connection peertransport.Connection
	address    EndpointAddress
	info       PeerInfo
	telemetry  *connectionTelemetry
}

func Bind(ctx context.Context, options EndpointOptions) (*Link, error) {
	scope := options.Scope
	var selection *protocol.NetworkSelection
	bind := scope.Bind
	if bind == "" && (scope.Internet || len(scope.Relays) != 0) {
		bind = ":0"
	}
	if len(scope.Relays) != 0 {
		selection = &protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: append([]string{}, scope.Relays...)}}
	} else if scope.Internet {
		selection = &protocol.NetworkSelection{Kind: "direct"}
	}
	var relays []string
	if selection != nil {
		var err error
		relays, err = NetworkRelayURLs(ctx, *selection)
		if err != nil {
			return nil, err
		}
	}
	if options.Peers == nil {
		return nil, invalidLink("peer storage is required")
	}
	peers, err := options.Peers.Directions(ctx)
	if err != nil {
		return nil, err
	}
	addresses, err := options.Peers.Addresses(ctx)
	if err != nil {
		return nil, err
	}
	endpoint, err := newLibp2pEndpoint(ctx, options.Identity.SecretKey(), bind, relays)
	if err != nil {
		return nil, err
	}
	lifetime, cancel := context.WithCancel(ctx)
	link := &Link{
		nodeID: options.Identity.ID(), endpoint: endpoint, ctx: lifetime, cancel: cancel, handler: options.Handler,
		store: options.Peers, softwareVersion: options.SoftwareVersion, paired: options.Paired,
		trusted: map[protocol.NodeID]bool{}, directions: map[protocol.NodeID]PeerDirection{}, addresses: map[protocol.NodeID]EndpointAddress{},
		outgoing: map[protocol.NodeID]*outgoingSlot{}, incoming: map[peertransport.Connection]protocol.NodeID{},
		work: make(chan func(), maxTasks), now: time.Now, timeout: context.WithTimeout,
		selection: selection, ticks: connectionTicks,
	}
	if link.softwareVersion == "" {
		link.softwareVersion = ProductVersion
	}
	for _, peer := range peers {
		if !peer.Direction.Valid() || link.trusted[peer.ID] {
			cancel()
			_ = endpoint.Close()
			return nil, invalidLink("invalid stored peer direction")
		}
		link.trusted[peer.ID] = true
		link.directions[peer.ID] = peer.Direction
	}
	for _, address := range addresses {
		if link.trusted[address.ID] {
			link.addresses[address.ID] = cloneAddress(address)
		}
	}
	if err := link.SetName(options.Name); err != nil {
		cancel()
		_ = endpoint.Close()
		return nil, err
	}
	link.workers.Add(2)
	go link.drive()
	go link.driveWork()
	return link, nil
}

func OpenController(ctx context.Context, path string, scope NetworkScope) (*Link, error) {
	identity, store, err := openControllerStore(path)
	if err != nil {
		return nil, err
	}
	link, err := Bind(ctx, EndpointOptions{Identity: identity, Scope: scope, Peers: store})
	if err != nil {
		_ = store.Close()
		return nil, err
	}
	link.controller = store
	return link, nil
}

func (link *Link) SetName(name string) error {
	name = strings.TrimSpace(name)
	if len([]rune(name)) > 128 || strings.ContainsFunc(name, unicode.IsControl) {
		return invalidLink("invalid device name")
	}
	link.mu.Lock()
	defer link.mu.Unlock()
	link.name = nil
	if name != "" {
		link.name = &name
	}
	return nil
}

func (link *Link) Address(ctx context.Context) (EndpointAddress, error) {
	scoped, cancel := link.operation(ctx, true)
	defer cancel()
	address, err := link.endpoint.Address(scoped)
	if err != nil {
		return EndpointAddress{}, linkNetwork(err)
	}
	return address, nil
}

func (link *Link) Online(ctx context.Context) error {
	scoped, cancel := link.operation(ctx, true)
	defer cancel()
	return linkNetwork(link.endpoint.Online(scoped))
}

// NetworkChanged rebinds the endpoint, then verifies each cached outgoing
// connection with one inspection. A connection that does not answer within
// livenessTimeout is dropped, so its streams fail now and the next use redials.
func (link *Link) NetworkChanged(ctx context.Context) error {
	scoped, cancel := link.operation(ctx, true)
	err := linkNetwork(link.endpoint.NetworkChange(scoped))
	cancel()
	link.mu.RLock()
	slots := maps.Clone(link.outgoing)
	link.mu.RUnlock()
	var checks sync.WaitGroup
	for peer, slot := range slots {
		slot.mu.Lock()
		connection := slot.connection
		slot.mu.Unlock()
		if connection == nil {
			continue
		}
		checks.Go(func() {
			if err := link.answers(ctx, connection.OpenBi); err != nil && ctx.Err() == nil {
				expired := errors.Is(err, context.DeadlineExceeded)
				if expired {
					// Serialize physical path cleanup with replacement dials.
					select {
					case <-ctx.Done():
						return
					case <-slot.ctx.Done():
						return
					case <-slot.dial:
					}
					defer func() { slot.dial <- struct{}{} }()
					if ctx.Err() != nil || slot.ctx.Err() != nil {
						return
					}
				}
				link.drop(peer, connection, "peer did not answer", expired)
			}
		})
	}
	checks.Wait()
	return err
}

// answers sends one inspection on a new stream and waits livenessTimeout for
// a reply. Any reply frame, including a rejection, shows the peer is reachable.
func (link *Link) answers(ctx context.Context, open func(context.Context) (contextStream, error)) (err error) {
	scoped, cancel := link.operation(ctx, false)
	defer cancel()
	bounded, expire := link.timeoutContext(scoped, livenessTimeout)
	defer expire()
	stream, err := open(bounded)
	if err == nil {
		defer stream.Close()
	}
	defer func() {
		if err != nil && errors.Is(bounded.Err(), context.DeadlineExceeded) {
			err = context.DeadlineExceeded
		}
	}()
	if err != nil {
		return linkNetwork(err)
	}
	if err := link.writeCall(bounded, stream, wire.Call{Kind: wire.CallInspect}, false); err != nil {
		return err
	}
	if err := stream.CloseWrite(); err != nil {
		return linkNetwork(err)
	}
	var reply wire.Reply
	return link.readFrame(bounded, stream, &reply, false)
}

func (link *Link) Peers(ctx context.Context) ([]EndpointAddress, error) {
	return link.store.Addresses(ctx)
}

func (link *Link) Remote(address EndpointAddress) *Remote {
	return &Remote{link: link, address: cloneAddress(address)}
}

func (link *Link) Inspect(ctx context.Context, address EndpointAddress) (PeerInfo, error) {
	connection, _, err := link.connection(ctx, address, false)
	if err != nil {
		return PeerInfo{}, err
	}
	info, err := link.inspectConnection(ctx, connection)
	if err == nil && info.Name != nil {
		err = link.RememberPeerName(ctx, address.ID, *info.Name)
	}
	return info, err
}

func (link *Link) SetTrust(ctx context.Context, peer protocol.NodeID, trusted bool) error {
	err := link.updateTrust(ctx, peer, func() error {
		return link.store.SetTrust(ctx, peer, trusted)
	})
	if err == nil && !trusted {
		err = link.RefreshPeers(ctx)
	}
	return err
}

// Disconnect closes the cached connection to a paired peer; the next use
// dials again. It is for a caller that knows the peer's process was replaced:
// a connection to a process that is gone is otherwise noticed only when an
// operation on it times out.
func (link *Link) Disconnect(peer protocol.NodeID) {
	link.drop(peer, nil, "peer restarted", false)
}

// drop closes the peer's cached connection so the next use redials. With an
// expected connection it leaves a newer one that replaced it in place.
func (link *Link) drop(peer protocol.NodeID, expected peertransport.Connection, reason string, discardPaths bool) {
	link.mu.RLock()
	slot := link.outgoing[peer]
	if slot == nil {
		link.mu.RUnlock()
		return
	}
	slot.mu.Lock()
	if expected != nil && slot.connection != expected {
		slot.mu.Unlock()
		link.mu.RUnlock()
		return
	}
	connection, telemetry := slot.connection, slot.telemetry
	if discardPaths && expected != nil {
		if stale, ok := connection.(interface{ DiscardPaths() error }); ok {
			_ = stale.DiscardPaths()
		}
	}
	slot.connection, slot.telemetry = nil, nil
	slot.mu.Unlock()
	link.mu.RUnlock()
	if telemetry != nil {
		telemetry.close()
		<-telemetry.done
	}
	if connection != nil {
		_ = connection.Close(0, reason)
	}
}

func (link *Link) applyTrust(peer protocol.NodeID, direction PeerDirection) {
	link.mu.Lock()
	previous := link.directions[peer]
	trusted := direction.Valid()
	if previous != direction {
		link.peerKinds.Delete(peer)
	}
	if link.directions == nil {
		link.directions = make(map[protocol.NodeID]PeerDirection)
	}
	link.trusted[peer] = trusted
	link.directions[peer] = direction
	var outgoing *outgoingSlot
	var incoming []peertransport.Connection
	if !trusted {
		delete(link.trusted, peer)
		delete(link.directions, peer)
		delete(link.addresses, peer)
		link.peerKinds.Delete(peer)
	}
	if !trusted || previous.AllowsOutbound() && !direction.AllowsOutbound() {
		outgoing = link.outgoing[peer]
		delete(link.outgoing, peer)
	}
	if !trusted || previous.AllowsInbound() && !direction.AllowsInbound() {
		for connection, caller := range link.incoming {
			if caller == peer {
				incoming = append(incoming, connection)
			}
		}
	}
	link.mu.Unlock()
	if outgoing != nil {
		outgoing.cancel()
		outgoing.mu.Lock()
		connection := outgoing.connection
		telemetry := outgoing.telemetry
		outgoing.connection = nil
		outgoing.telemetry = nil
		outgoing.mu.Unlock()
		if telemetry != nil {
			telemetry.close()
			<-telemetry.done
		}
		if connection != nil && !link.retainRemovalConnection(peer, connection) {
			_ = connection.Close(peerRevokedCloseCode, "peer revoked")
		}
	}
	for _, connection := range incoming {
		_ = connection.Close(peerRevokedCloseCode, "peer revoked")
	}
	link.notifyPeers()
}

func (link *Link) remember(ctx context.Context, address EndpointAddress) error {
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	if !link.isTrusted(address.ID) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer is not paired"}
	}
	if err := link.store.Remember(ctx, address); err != nil {
		if faultValue(err).Code == protocol.ErrorCodeOutcomeUnknown {
			return errors.Join(err, link.reconcilePeer(ctx, address.ID))
		}
		return err
	}
	link.mu.Lock()
	link.addresses[address.ID] = cloneAddress(address)
	link.mu.Unlock()
	link.notifyPeers()
	return nil
}

func (link *Link) reconcilePeer(ctx context.Context, peer protocol.NodeID) error {
	scoped, cancel := link.operation(context.WithoutCancel(ctx), false)
	defer cancel()
	peers, err := link.store.Directions(scoped)
	if err != nil {
		return err
	}
	addresses, err := link.store.Addresses(scoped)
	if err != nil {
		return err
	}
	var direction PeerDirection
	for _, stored := range peers {
		if !stored.Direction.Valid() {
			return invalidLink("invalid stored peer direction")
		}
		if stored.ID == peer {
			if direction != "" {
				return invalidLink("duplicate stored peer identity")
			}
			direction = stored.Direction
		}
	}
	link.applyTrust(peer, direction)
	if direction.Valid() {
		link.mu.Lock()
		delete(link.addresses, peer)
		for _, address := range addresses {
			if address.ID == peer {
				link.addresses[peer] = cloneAddress(address)
				break
			}
		}
		link.mu.Unlock()
		link.notifyPeers()
	}
	return nil
}

func (link *Link) Close() error {
	link.closeOnce.Do(func() {
		link.cancel()
		link.trustUpdate.Lock()
		var telemetry []*connectionTelemetry
		link.mu.RLock()
		for _, slot := range link.outgoing {
			slot.mu.Lock()
			if slot.telemetry != nil {
				telemetry = append(telemetry, slot.telemetry)
			}
			slot.mu.Unlock()
		}
		link.mu.RUnlock()
		link.trustUpdate.Unlock()
		link.mu.RLock()
		var notices []*HostRemovalNotice
		for _, notice := range link.removalNotices {
			notices = append(notices, notice)
		}
		link.mu.RUnlock()
		for _, notice := range notices {
			notice.Close()
		}
		for _, state := range telemetry {
			state.close()
		}
		link.closeErr = link.endpoint.Close()
		for _, state := range telemetry {
			<-state.done
		}
		link.workers.Wait()
		if link.controller != nil {
			link.closeErr = errors.Join(link.closeErr, link.controller.Close())
		}
	})
	return link.closeErr
}

func (link *Link) isTrusted(peer protocol.NodeID) bool {
	link.mu.RLock()
	defer link.mu.RUnlock()
	return link.trusted[peer]
}

func (link *Link) operation(ctx context.Context, timed bool) (context.Context, context.CancelFunc) {
	scoped, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(link.ctx, cancel)
	if !timed {
		return scoped, func() { stop(); cancel() }
	}
	deadline, expire := link.timeoutContext(scoped, ioTimeout)
	return deadline, func() { stop(); expire(); cancel() }
}

func (link *Link) timeoutContext(ctx context.Context, duration time.Duration) (context.Context, context.CancelFunc) {
	if link.timeout != nil {
		return link.timeout(ctx, duration)
	}
	return context.WithTimeout(ctx, duration)
}

func (link *Link) envelope(call wire.Call) wire.Envelope {
	version := uint16(protocol.VERSION)
	hasNode := link.handler != nil
	return wire.Envelope{SoftwareVersion: &link.softwareVersion, Protocol: &version, Zstd: true, Call: call, HasNode: &hasNode}
}

func (link *Link) endpointReply() wire.Endpoint {
	link.mu.RLock()
	name := link.name
	link.mu.RUnlock()
	version := uint16(protocol.VERSION)
	hasNode := link.handler != nil
	platform := runtime.GOOS
	if platform == "darwin" {
		platform = "macos"
	}
	return wire.Endpoint{
		SoftwareVersion: &link.softwareVersion, Protocol: &version, Zstd: true,
		Execution: link.handler != nil, Name: name, Platform: platform,
		HasNode: &hasNode,
	}
}

func cloneAddress(address EndpointAddress) EndpointAddress {
	copy := EndpointAddress{ID: address.ID, Addrs: make([]json.RawMessage, len(address.Addrs))}
	for index, value := range address.Addrs {
		copy.Addrs[index] = append(json.RawMessage{}, value...)
	}
	return copy
}

func mergeAddress(address, remembered EndpointAddress) EndpointAddress {
	values := map[string]json.RawMessage{}
	for _, value := range append(append([]json.RawMessage{}, address.Addrs...), remembered.Addrs...) {
		var compact bytes.Buffer
		if json.Compact(&compact, value) == nil {
			values[compact.String()] = append(json.RawMessage{}, compact.Bytes()...)
		}
	}
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	merged := EndpointAddress{ID: address.ID, Addrs: make([]json.RawMessage, 0, len(keys))}
	for _, key := range keys {
		merged.Addrs = append(merged.Addrs, values[key])
	}
	return merged
}

func addressIncludes(address, subset EndpointAddress) bool {
	if address.ID != subset.ID {
		return false
	}
	known := make(map[string]struct{}, len(address.Addrs))
	var compact bytes.Buffer
	for _, value := range address.Addrs {
		compact.Reset()
		if json.Compact(&compact, value) == nil {
			known[compact.String()] = struct{}{}
		}
	}
	for _, value := range subset.Addrs {
		compact.Reset()
		if json.Compact(&compact, value) != nil {
			continue
		}
		if _, ok := known[compact.String()]; !ok {
			return false
		}
	}
	return true
}

func invalidLink(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func linkNetwork(err error) error {
	if err == nil {
		return nil
	}
	if errors.Is(err, libp2pRevokedByPeer) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer revoked access"}
	}
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	var wrapped *FaultError
	if errors.As(err, &wrapped) {
		return &wrapped.Fault
	}
	var value protocol.Fault
	if errors.As(err, &value) {
		return &value
	}
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Link connection is unavailable"}
}
