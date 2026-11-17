package link

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"runtime"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

const maxConnections = 16
const maxTasks = 64
const maxStreams = 64
const ioTimeout = 10 * time.Second

type EndpointAddress = iroh.Address

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
	endpoint        *iroh.Endpoint
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
	incoming        map[*iroh.Connection]protocol.NodeID
	trustUpdate     sync.Mutex
	relayUpdate     chan struct{}
	selection       *protocol.NetworkSelection
	relayURLs       []string
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
	connection *iroh.Connection
	address    EndpointAddress
	info       PeerInfo
	telemetry  *connectionTelemetry
}

func Bind(ctx context.Context, options EndpointOptions) (*Link, error) {
	config := iroh.Config{
		SecretKey: options.Identity.SecretKey(),
		Mode:      "direct",
		Bind:      options.Scope.Bind,
	}
	if options.Scope.Internet && len(options.Scope.Relays) != 0 {
		return nil, invalidLink("select one relay network")
	}
	if options.Scope.Internet {
		config.Mode = "internet"
	}
	if len(options.Scope.Relays) != 0 {
		config.Mode, config.Relays = "custom", append([]string{}, options.Scope.Relays...)
	}
	return bindConfigured(ctx, options, config)
}

func bindConfigured(ctx context.Context, options EndpointOptions, config iroh.Config) (*Link, error) {
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
	endpoint, err := iroh.NewEndpoint(ctx, config)
	if err != nil {
		return nil, linkNetwork(err)
	}
	var selection *protocol.NetworkSelection
	switch config.Mode {
	case "internet":
		selection = &protocol.NetworkSelection{Kind: "iroh"}
	case "custom":
		selection = &protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: append([]string{}, config.Relays...)}}
	}
	var relays []string
	if selection != nil {
		relays, err = NetworkRelayURLs(ctx, *selection)
		if err != nil {
			_ = endpoint.Close()
			return nil, err
		}
	}
	lifetime, cancel := context.WithCancel(ctx)
	link := &Link{
		nodeID: options.Identity.ID(), endpoint: endpoint, ctx: lifetime, cancel: cancel, handler: options.Handler,
		store: options.Peers, softwareVersion: options.SoftwareVersion, paired: options.Paired,
		trusted: map[protocol.NodeID]bool{}, directions: map[protocol.NodeID]PeerDirection{}, addresses: map[protocol.NodeID]EndpointAddress{},
		outgoing: map[protocol.NodeID]*outgoingSlot{}, incoming: map[*iroh.Connection]protocol.NodeID{},
		work: make(chan func(), maxTasks), now: time.Now, timeout: context.WithTimeout,
		selection: selection, relayURLs: relays, ticks: connectionTicks,
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

func (link *Link) NetworkChanged(ctx context.Context) error {
	scoped, cancel := link.operation(ctx, true)
	defer cancel()
	return linkNetwork(link.endpoint.NetworkChange(scoped))
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
	link.mu.RLock()
	slot := link.outgoing[peer]
	link.mu.RUnlock()
	if slot == nil {
		return
	}
	slot.mu.Lock()
	connection, telemetry := slot.connection, slot.telemetry
	slot.connection, slot.telemetry = nil, nil
	slot.mu.Unlock()
	if telemetry != nil {
		telemetry.close()
		<-telemetry.done
	}
	if connection != nil {
		_ = connection.Close(0, "peer restarted")
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
	var incoming []*iroh.Connection
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
		if connection != nil {
			_ = connection.Close(1, "peer revoked")
		}
	}
	for _, connection := range incoming {
		_ = connection.Close(1, "peer revoked")
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
