package peerrelay

import (
	"errors"
	"fmt"
	"net/netip"
	"slices"
	"sync"
	"time"

	"github.com/libp2p/go-libp2p"
	"github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/peer"
	"github.com/libp2p/go-libp2p/p2p/protocol/circuitv2/relay"
	tls "github.com/libp2p/go-libp2p/p2p/security/tls"
	quic "github.com/libp2p/go-libp2p/p2p/transport/quic"
	"github.com/libp2p/go-libp2p/p2p/transport/tcp"
	ma "github.com/multiformats/go-multiaddr"
)

const maxAllowedPeers = 64

type Config struct {
	PrivateKey   crypto.PrivKey
	ListenAddr   ma.Multiaddr
	AnnounceAddr ma.Multiaddr
	Public       bool
	AllowedPeers []peer.ID
}

type Relay struct {
	addr  ma.Multiaddr
	close func() error
}

// ValidateConfig checks addresses and access rules without requiring a relay identity.
func ValidateConfig(cfg Config) error {
	if _, err := validateAddresses(cfg.ListenAddr, cfg.AnnounceAddr); err != nil {
		return err
	}
	_, err := validateAccess(cfg, "")
	return err
}

func New(cfg Config) (*Relay, error) {
	transport, err := validateAddresses(cfg.ListenAddr, cfg.AnnounceAddr)
	if err != nil {
		return nil, err
	}
	if cfg.PrivateKey == nil {
		return nil, errors.New("relay identity is required")
	}
	id, err := peer.IDFromPrivateKey(cfg.PrivateKey)
	if err != nil {
		return nil, fmt.Errorf("derive relay peer ID: %w", err)
	}
	peerAddr, err := ma.NewComponent("p2p", id.String())
	if err != nil {
		return nil, fmt.Errorf("make relay peer address: %w", err)
	}
	allowed, err := validateAccess(cfg, id)
	if err != nil {
		return nil, err
	}
	rc := relay.DefaultResources()
	rc.Limit = &relay.RelayLimit{Duration: 30 * time.Minute, Data: 256 << 20}
	rc.MaxCircuits = 8
	rc.BufferSize = 32 << 10
	var acl relay.ACLFilter
	if allowed != nil {
		acl = allowed
		rc.MaxReservations = len(allowed)
		rc.MaxReservationsPerIP = len(allowed)
		rc.MaxReservationsPerASN = len(allowed)
	}

	options := []libp2p.Option{
		libp2p.Identity(cfg.PrivateKey),
		libp2p.NoListenAddrs,
		libp2p.Security(tls.ID, tls.New),
		libp2p.DisableIdentifyAddressDiscovery(),
		libp2p.DisableMetrics(),
	}
	if transport == ma.P_TCP {
		options = append(options, libp2p.Transport(tcp.NewTCPTransport, tcp.DisableReuseport()))
	} else {
		options = append(options, libp2p.Transport(quic.NewTransport))
	}
	announce := slices.Clone(cfg.AnnounceAddr)
	if len(announce) != 0 {
		options = append(options, libp2p.AddrsFactory(func([]ma.Multiaddr) []ma.Multiaddr {
			return []ma.Multiaddr{announce}
		}))
	}
	host, err := libp2p.New(options...)
	if err != nil {
		return nil, fmt.Errorf("start relay host: %w", err)
	}
	service, err := relay.New(host, relay.WithResources(rc), relay.WithACL(acl),
		relay.WithReservationAddressFilter(func(ma.Multiaddr) bool { return true }))
	if err != nil {
		return nil, errors.Join(fmt.Errorf("start Circuit Relay v2: %w", err), host.Close())
	}
	closeRelay := sync.OnceValue(func() error { return errors.Join(service.Close(), host.Close()) })
	if err := host.Network().Listen(cfg.ListenAddr); err != nil {
		return nil, errors.Join(fmt.Errorf("listen on %s: %w", cfg.ListenAddr, err), closeRelay())
	}
	bound := host.Network().ListenAddresses()
	if len(bound) != 1 || len(bound[0]) < 2 || bound[0][1].Value() == "0" {
		return nil, errors.Join(errors.New("relay did not bind a single direct address"), closeRelay())
	}
	advertised := announce
	if len(advertised) == 0 {
		advertised = bound[0]
	}
	return &Relay{addr: advertised.Encapsulate(peerAddr), close: closeRelay}, nil
}

func (r *Relay) Addr() ma.Multiaddr { return slices.Clone(r.addr) }

func (r *Relay) Close() error { return r.close() }

type allowlist map[peer.ID]struct{}

func validateAccess(cfg Config, self peer.ID) (allowlist, error) {
	if cfg.Public {
		if len(cfg.AllowedPeers) != 0 {
			return nil, errors.New("public relay cannot be combined with a peer allowlist")
		}
		return nil, nil
	}
	return newAllowlist(cfg.AllowedPeers, self)
}

func newAllowlist(peers []peer.ID, self peer.ID) (allowlist, error) {
	if len(peers) == 0 || len(peers) > maxAllowedPeers {
		return nil, fmt.Errorf("relay requires between 1 and %d allowed peers", maxAllowedPeers)
	}
	allowed := make(allowlist, len(peers))
	for i, id := range peers {
		if _, err := ma.NewComponent("p2p", id.String()); err != nil {
			return nil, fmt.Errorf("invalid allowlist peer %d: %w", i+1, err)
		}
		if id == self {
			return nil, errors.New("allowlist must not contain the relay's own peer ID")
		}
		allowed[id] = struct{}{}
	}
	return allowed, nil
}

func (a allowlist) AllowReserve(id peer.ID, _ ma.Multiaddr) bool {
	_, allowed := a[id]
	return allowed
}

func (a allowlist) AllowConnect(source peer.ID, _ ma.Multiaddr, destination peer.ID) bool {
	_, sourceAllowed := a[source]
	_, destinationAllowed := a[destination]
	return sourceAllowed && destinationAllowed
}

func validateAddresses(listen, announce ma.Multiaddr) (int, error) {
	listenIP, transport, err := directAddress(listen)
	if err != nil {
		return 0, fmt.Errorf("listen address: %w", err)
	}
	if len(announce) == 0 {
		if listenIP.IsUnspecified() {
			return 0, errors.New("wildcard listen address requires an announce address")
		}
		return transport, nil
	}
	announceIP, announceTransport, err := directAddress(announce)
	if err != nil {
		return 0, fmt.Errorf("announce address: %w", err)
	}
	if listenIP.IsLoopback() || !announceIP.IsGlobalUnicast() || announce[1].Value() == "0" {
		return 0, errors.New("announce requires a non-loopback listener and a concrete non-loopback IP and port")
	}
	if announceTransport != transport {
		return 0, errors.New("listen and announce addresses must use the same transport")
	}
	return transport, nil
}

func directAddress(addr ma.Multiaddr) (netip.Addr, int, error) {
	if len(addr) >= 2 && (addr[0].Code() == ma.P_IP4 || addr[0].Code() == ma.P_IP6) {
		ip, err := netip.ParseAddr(addr[0].Value())
		ip = ip.Unmap()
		if err == nil && (ip.IsUnspecified() || ip.IsLoopback() || ip.IsGlobalUnicast()) {
			switch {
			case len(addr) == 2 && addr[1].Code() == ma.P_TCP:
				return ip, ma.P_TCP, nil
			case len(addr) == 3 && addr[1].Code() == ma.P_UDP && addr[2].Code() == ma.P_QUIC_V1:
				return ip, ma.P_QUIC_V1, nil
			}
		}
	}
	return netip.Addr{}, 0, errors.New("expected an IP multiaddr with TCP or UDP/QUIC-v1")
}
