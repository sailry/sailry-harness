package link

import (
	"context"
	"errors"
	"slices"
	"time"

	"github.com/libp2p/go-libp2p/core/host"
	"github.com/libp2p/go-libp2p/core/network"
	"github.com/libp2p/go-libp2p/core/peer"
	lpprotocol "github.com/libp2p/go-libp2p/core/protocol"
	relayclient "github.com/libp2p/go-libp2p/p2p/protocol/circuitv2/client"
	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
)

const libp2pRelayProtection = "sailry-link-relay"

type libp2pRelay struct {
	spec   libp2pRelaySpec
	cancel context.CancelFunc
	done   chan struct{}
	wake   chan struct{}

	leg     network.Conn
	expires time.Time
}

func (relay *libp2pRelay) refresh() {
	select {
	case relay.wake <- struct{}{}:
	default:
	}
}

func (endpoint *libp2pEndpoint) notifyReservationsLocked() {
	close(endpoint.reservationsReady)
	endpoint.reservationsReady = make(chan struct{})
}

func liveLibp2pRelays(relays []*libp2pRelay) []ma.Multiaddr {
	now := time.Now()
	var ready []ma.Multiaddr
	for _, relay := range relays {
		if relay.leg != nil && !relay.leg.IsClosed() && now.Before(relay.expires) {
			ready = append(ready, relay.spec.circuit)
		}
	}
	return ready
}

func (endpoint *libp2pEndpoint) readyRelays(ctx context.Context) ([]ma.Multiaddr, error) {
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if endpoint.ctx.Err() != nil {
			return nil, peertransport.ErrClosed
		}
		endpoint.mu.Lock()
		ready, direct, changed := liveLibp2pRelays(endpoint.activeRelays), len(endpoint.activeRelays) == 0, endpoint.reservationsReady
		endpoint.mu.Unlock()
		if direct || len(ready) > 0 {
			return ready, nil
		}
		select {
		case <-ctx.Done():
		case <-endpoint.ctx.Done():
		case <-changed:
		}
	}
}

func (endpoint *libp2pEndpoint) SetRelays(ctx context.Context, values []string) error {
	specs, err := parseLibp2pRelays(values)
	if err != nil {
		return err
	}
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-endpoint.ctx.Done():
		return peertransport.ErrClosed
	case endpoint.relayUpdate <- struct{}{}:
	}
	defer func() { <-endpoint.relayUpdate }()

	endpoint.mu.Lock()
	if err := ctx.Err(); err != nil {
		endpoint.mu.Unlock()
		return err
	}
	if endpoint.ctx.Err() != nil {
		endpoint.mu.Unlock()
		return peertransport.ErrClosed
	}
	previous := endpoint.activeRelays
	for _, spec := range specs {
		if spec.info.ID == endpoint.host.ID() {
			endpoint.mu.Unlock()
			return invalidLink("a libp2p endpoint cannot use itself as a relay")
		}
		for _, relay := range previous {
			if relay.spec.info.ID == spec.info.ID && !relay.spec.circuit.Equal(spec.circuit) {
				endpoint.mu.Unlock()
				return invalidLink("remove the existing relay before changing the address for its peer ID")
			}
		}
	}
	endpoint.storeRelayRulesLocked(previous, specs)
	var next, added []*libp2pRelay
	for _, spec := range specs {
		i := slices.IndexFunc(previous, func(r *libp2pRelay) bool { return r.spec.info.ID == spec.info.ID })
		if i >= 0 {
			next = append(next, previous[i])
		} else {
			relay := endpoint.startRelayLocked(spec)
			next, added = append(next, relay), append(added, relay)
		}
	}
	endpoint.mu.Unlock()

	for {
		endpoint.mu.Lock()
		err := ctx.Err()
		if endpoint.ctx.Err() != nil {
			err = peertransport.ErrClosed
		}
		if err != nil {
			endpoint.storeRelayRulesLocked(previous, nil)
			endpoint.mu.Unlock()
			endpoint.retireRelays(added, false)
			return err
		}
		if len(next) == 0 || len(liveLibp2pRelays(next)) > 0 {
			endpoint.activeRelays = next
			endpoint.storeRelayRulesLocked(next, nil)
			endpoint.notifyReservationsLocked()
			endpoint.mu.Unlock()
			var removed []*libp2pRelay
			for _, relay := range previous {
				if !slices.Contains(next, relay) {
					removed = append(removed, relay)
				}
			}
			endpoint.retireRelays(removed, true)
			return nil
		}
		changed := endpoint.reservationsReady
		endpoint.mu.Unlock()
		select {
		case <-ctx.Done():
		case <-endpoint.ctx.Done():
		case <-changed:
		}
	}
}

func (endpoint *libp2pEndpoint) storeRelayRulesLocked(active []*libp2pRelay, candidates []libp2pRelaySpec) {
	rules := &libp2pRelayRules{}
	for _, relay := range active {
		rules.active = append(rules.active, relay.spec)
	}
	rules.reserving = slices.Clone(rules.active)
	for _, spec := range candidates {
		if !rules.reservationPeer(spec.info.ID) {
			rules.reserving = append(rules.reserving, spec)
		}
	}
	endpoint.gater.rules.Store(rules)
}

func (endpoint *libp2pEndpoint) startRelayLocked(spec libp2pRelaySpec) *libp2pRelay {
	ctx, cancel := context.WithCancel(endpoint.ctx)
	relay := &libp2pRelay{spec: spec, cancel: cancel, done: make(chan struct{}), wake: make(chan struct{}, 1)}
	endpoint.reservations[relay] = struct{}{}
	endpoint.relayWork.Add(1)
	go endpoint.maintainRelay(ctx, relay)
	return relay
}

func (endpoint *libp2pEndpoint) retireRelays(relays []*libp2pRelay, disconnect bool) {
	for _, relay := range relays {
		relay.cancel()
	}
	for _, relay := range relays {
		<-relay.done
		endpoint.mu.Lock()
		delete(endpoint.reservations, relay)
		endpoint.mu.Unlock()
		if disconnect && endpoint.ctx.Err() == nil {
			_ = endpoint.host.Network().ClosePeer(relay.spec.info.ID)
		}
	}
}

func (endpoint *libp2pEndpoint) maintainRelay(ctx context.Context, relay *libp2pRelay) {
	defer endpoint.relayWork.Done()
	defer close(relay.done)
	id := relay.spec.info.ID
	endpoint.host.ConnManager().Protect(id, libp2pRelayProtection)
	defer endpoint.host.ConnManager().Unprotect(id, libp2pRelayProtection)
	var delay time.Duration
	backoff := time.Second
	for ctx.Err() == nil {
		if delay != 0 {
			timer := time.NewTimer(delay)
			select {
			case <-ctx.Done():
				timer.Stop()
				return
			case <-relay.wake:
				timer.Stop()
			case <-timer.C:
			}
		}
		leg, expiry, err := endpoint.reserveRelay(ctx, relay)
		if ctx.Err() != nil {
			return
		}
		endpoint.mu.Lock()
		relay.leg, relay.expires = leg, expiry
		endpoint.notifyReservationsLocked()
		endpoint.mu.Unlock()
		if err == nil {
			delay = max(time.Second, time.Until(expiry)/2)
			backoff = time.Second
		} else {
			delay = backoff
			backoff = min(15*time.Second, 2*backoff)
		}
	}
}

// Reserve sets its own I/O deadline once it opens a stream. Reset that stream
// on cancellation so selection deadlines and endpoint teardown also bound I/O.
type libp2pReservationHost struct {
	host.Host
	spec libp2pRelaySpec
	leg  network.Conn
	stop func()
}

func (h *libp2pReservationHost) NewStream(ctx context.Context, id peer.ID, protocols ...lpprotocol.ID) (network.Stream, error) {
	raw, err := h.Host.NewStream(network.WithNoDial(ctx, "static relay reservation"), id, protocols...)
	if err != nil {
		return nil, err
	}
	leg := raw.Conn()
	key := leg.RemotePublicKey()
	if key == nil || !id.MatchesPublicKey(key) || !h.spec.matchesLeg(leg.RemotePeer(), leg.RemoteMultiaddr()) {
		_ = raw.Reset()
		return nil, errors.New("libp2p relay connection does not match its configured path")
	}
	h.leg = leg
	interrupted := make(chan struct{})
	stop := context.AfterFunc(ctx, func() {
		_ = raw.Reset()
		close(interrupted)
	})
	h.stop = func() {
		if !stop() {
			<-interrupted
		}
	}
	return raw, nil
}

func (endpoint *libp2pEndpoint) reserveRelay(ctx context.Context, relay *libp2pRelay) (network.Conn, time.Time, error) {
	scoped, cancel := context.WithTimeout(ctx, ioTimeout)
	defer cancel()
	if err := endpoint.host.Connect(scoped, relay.spec.info); err != nil {
		return nil, time.Time{}, err
	}
	h := &libp2pReservationHost{Host: endpoint.host, spec: relay.spec}
	reservation, err := relayclient.Reserve(scoped, h, relay.spec.info)
	if h.stop != nil {
		h.stop()
	}
	if scoped.Err() != nil {
		endpoint.mu.Lock()
		selected := slices.Contains(endpoint.activeRelays, relay)
		endpoint.mu.Unlock()
		if selected && ctx.Err() == nil && h.leg != nil {
			_ = h.leg.Close()
		}
		return nil, time.Time{}, scoped.Err()
	}
	if err != nil {
		return nil, time.Time{}, err
	}
	if voucher := reservation.Voucher; voucher != nil && (voucher.Relay != relay.spec.info.ID || voucher.Peer != h.ID() || !voucher.Expiration.Equal(reservation.Expiration)) {
		return nil, time.Time{}, errors.New("libp2p relay reservation voucher does not match the reservation")
	}
	return h.leg, reservation.Expiration, nil
}
