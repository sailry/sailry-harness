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
}

type PeerInfo struct {
	Execution       bool
	Name            *string
	Platform        string
	SoftwareVersion *string
	Protocol        *uint16
	Latency         time.Duration
	Zstd            bool
}

func (info PeerInfo) Compatibility() error {
	return CheckHost(info.Protocol, info.SoftwareVersion)
}

type Link struct {
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
	addresses       map[protocol.NodeID]EndpointAddress
	outgoing        map[protocol.NodeID]*outgoingSlot
	incoming        map[*iroh.Connection]protocol.NodeID
	trustUpdate     sync.Mutex
	relayUpdate     chan struct{}
	selection       *protocol.NetworkSelection
	relayURLs       []string
	peerChanged     chan struct{}
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
	peers, err := options.Peers.Peers(ctx)
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
		endpoint: endpoint, ctx: lifetime, cancel: cancel, handler: options.Handler,
		store: options.Peers, softwareVersion: options.SoftwareVersion,
		trusted: map[protocol.NodeID]bool{}, addresses: map[protocol.NodeID]EndpointAddress{},
		outgoing: map[protocol.NodeID]*outgoingSlot{}, incoming: map[*iroh.Connection]protocol.NodeID{},
		work: make(chan func(), maxTasks), now: time.Now, timeout: context.WithTimeout,
		selection: selection, relayURLs: relays, ticks: connectionTicks,
	}
	if link.softwareVersion == "" {
		link.softwareVersion = ProductVersion
	}
	for _, peer := range peers {
		link.trusted[peer] = true
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
	return link.inspectConnection(ctx, connection)
}

func (link *Link) SetTrust(ctx context.Context, peer protocol.NodeID, trusted bool) error {
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	if err := link.ctx.Err(); err != nil {
		return linkNetwork(err)
	}
	if err := link.store.SetTrust(ctx, peer, trusted); err != nil {
		if faultValue(err).Code == protocol.ErrorCodeOutcomeUnknown {
			reconciled := link.reconcilePeer(ctx, peer)
			if reconciled != nil && !trusted {
				link.applyTrust(peer, false)
			}
			return errors.Join(err, reconciled)
		}
		return err
	}
	link.applyTrust(peer, trusted)
	return nil
}

func (link *Link) applyTrust(peer protocol.NodeID, trusted bool) {
	link.mu.Lock()
	link.trusted[peer] = trusted
	var outgoing *outgoingSlot
	var incoming []*iroh.Connection
	if !trusted {
		delete(link.trusted, peer)
		delete(link.addresses, peer)
		outgoing = link.outgoing[peer]
		delete(link.outgoing, peer)
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
	peers, err := link.store.Peers(scoped)
	if err != nil {
		return err
	}
	addresses, err := link.store.Addresses(scoped)
	if err != nil {
		return err
	}
	trusted := false
	for _, stored := range peers {
		trusted = trusted || stored == peer
	}
	link.applyTrust(peer, trusted)
	if trusted {
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
	return wire.Envelope{SoftwareVersion: &link.softwareVersion, Protocol: &version, Zstd: true, Call: call}
}

func (link *Link) endpointReply() wire.Endpoint {
	link.mu.RLock()
	name := link.name
	link.mu.RUnlock()
	version := uint16(protocol.VERSION)
	platform := runtime.GOOS
	if platform == "darwin" {
		platform = "macos"
	}
	return wire.Endpoint{
		SoftwareVersion: &link.softwareVersion, Protocol: &version, Zstd: true,
		Execution: link.handler != nil, Name: name, Platform: platform,
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
	merged := mergeAddress(address, subset)
	normalized := mergeAddress(address, EndpointAddress{ID: address.ID})
	return address.ID == subset.ID && len(merged.Addrs) == len(normalized.Addrs)
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
