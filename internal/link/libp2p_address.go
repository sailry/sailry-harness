package link

import (
	"encoding/json"
	"fmt"
	"net"
	"net/netip"
	"slices"
	"strconv"
	"sync"
	"sync/atomic"

	"github.com/libp2p/go-libp2p/core/control"
	lpcrypto "github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/network"
	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"
	manet "github.com/multiformats/go-multiaddr/net"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const libp2pMaxHints = 16
const libp2pMaxHintBytes = 512

func libp2pListeners(bind string) ([]ma.Multiaddr, error) {
	if bind == "" {
		bind = "127.0.0.1:0"
	}
	host, port, err := net.SplitHostPort(bind)
	if err != nil {
		return nil, invalidLink("libp2p bind requires an IP address and port")
	}
	p, err := strconv.ParseUint(port, 10, 16)
	if err != nil {
		return nil, invalidLink("libp2p bind has an invalid port")
	}
	var hosts []string
	if host == "" {
		hosts = []string{"/ip4/0.0.0.0", "/ip6/::"}
	} else {
		ip, err := netip.ParseAddr(host)
		if err != nil || ip.Zone() != "" {
			return nil, invalidLink("libp2p bind requires an IP address")
		}
		ip = ip.Unmap()
		family := "ip6"
		if ip.Is4() {
			family = "ip4"
		}
		hosts = []string{"/" + family + "/" + ip.String()}
	}
	addrs := make([]ma.Multiaddr, 0, 2*len(hosts))
	for _, host := range hosts {
		addrs = append(addrs, ma.StringCast(fmt.Sprintf("%s/tcp/%d", host, p)), ma.StringCast(fmt.Sprintf("%s/udp/%d/quic-v1", host, p)))
	}
	return addrs, nil
}

func libp2pDirectAddr(addr ma.Multiaddr, dns bool) bool {
	if len(addr) != 2 && len(addr) != 3 {
		return false
	}
	switch addr[0].Protocol().Code {
	case ma.P_IP4, ma.P_IP6:
		ip, err := netip.ParseAddr(addr[0].Value())
		if err != nil || ip.IsUnspecified() || ip.IsMulticast() || ip.Is6() && ip.IsLinkLocalUnicast() {
			return false
		}
	case ma.P_DNS, ma.P_DNS4, ma.P_DNS6:
		if !dns {
			return false
		}
	default:
		return false
	}
	if addr[1].Value() == "0" {
		return false
	}
	return len(addr) == 2 && addr[1].Protocol().Code == ma.P_TCP ||
		len(addr) == 3 && addr[1].Protocol().Code == ma.P_UDP && addr[2].Protocol().Code == ma.P_QUIC_V1
}

type libp2pRelaySpec struct {
	info    peer.AddrInfo
	circuit ma.Multiaddr
}

func parseLibp2pRelay(value string) (libp2pRelaySpec, error) {
	if len(value) > libp2pMaxHintBytes-len("/p2p-circuit") {
		return libp2pRelaySpec{}, invalidLink("libp2p relay multiaddr is too long")
	}
	address, err := ma.NewMultiaddr(value)
	if err != nil {
		return libp2pRelaySpec{}, invalidLink("invalid libp2p relay multiaddr")
	}
	base, id := peer.SplitAddr(address)
	if id == "" || !libp2pDirectAddr(base, true) {
		return libp2pRelaySpec{}, invalidLink("libp2p relay requires TCP or QUIC with a /p2p peer ID")
	}
	return libp2pRelaySpec{
		info:    peer.AddrInfo{ID: id, Addrs: []ma.Multiaddr{base}},
		circuit: ma.Join(address, ma.StringCast("/p2p-circuit")),
	}, nil
}

func canonicalLibp2pRelay(value string) (string, error) {
	spec, err := parseLibp2pRelay(value)
	if err != nil {
		return "", err
	}
	return spec.circuit[:len(spec.circuit)-1].String(), nil
}

func parseLibp2pRelays(values []string) ([]libp2pRelaySpec, error) {
	if len(values) > 2 {
		return nil, invalidLink("libp2p supports at most two static relays")
	}
	relays := make([]libp2pRelaySpec, 0, len(values))
	for _, value := range values {
		relay, err := parseLibp2pRelay(value)
		if err != nil {
			return nil, err
		}
		for _, prior := range relays {
			if relay.info.ID == prior.info.ID {
				return nil, invalidLink("libp2p relays require distinct peer IDs")
			}
		}
		relays = append(relays, relay)
	}
	return relays, nil
}

func libp2pCircuitRelay(addr ma.Multiaddr) (peer.ID, bool) {
	if len(addr) == 0 || addr[len(addr)-1].Protocol().Code != ma.P_CIRCUIT {
		return "", false
	}
	base, id := peer.SplitAddr(addr[:len(addr)-1])
	return id, id != "" && libp2pDirectAddr(base, true)
}

func libp2pAddressInfo(address peertransport.Address) (peer.AddrInfo, error) {
	if len(address.Addrs) > libp2pMaxHints {
		return peer.AddrInfo{}, invalidLink("too many libp2p address hints")
	}
	key, err := lpcrypto.UnmarshalEd25519PublicKey(address.ID[:])
	if err != nil {
		return peer.AddrInfo{}, invalidLink("invalid libp2p peer identity")
	}
	id, err := peer.IDFromPublicKey(key)
	if err != nil {
		return peer.AddrInfo{}, invalidLink("invalid libp2p peer identity")
	}
	info := peer.AddrInfo{ID: id}
	for _, raw := range address.Addrs {
		var hint map[string]json.RawMessage
		if json.Unmarshal(raw, &hint) != nil {
			return peer.AddrInfo{}, invalidLink("invalid libp2p address hint")
		}
		encoded, ok := hint["Libp2p"]
		if !ok {
			continue
		}
		var value string
		if len(hint) != 1 || json.Unmarshal(encoded, &value) != nil || len(value) > libp2pMaxHintBytes {
			return peer.AddrInfo{}, invalidLink("invalid libp2p address hint")
		}
		addr, err := ma.NewMultiaddr(value)
		if err != nil || addr.String() != value {
			return peer.AddrInfo{}, invalidLink("libp2p address hint requires a canonical multiaddr")
		}
		relay, circuit := libp2pCircuitRelay(addr)
		if circuit && relay == id {
			return peer.AddrInfo{}, invalidLink("a libp2p circuit must use a different relay peer ID")
		}
		if !circuit && !libp2pDirectAddr(addr, false) {
			return peer.AddrInfo{}, invalidLink("unsupported libp2p address hint")
		}
		info.Addrs = append(info.Addrs, addr)
	}
	return info, nil
}

func libp2pNodeID(conn network.Conn) (protocol.NodeID, bool) {
	var id protocol.NodeID
	key := conn.RemotePublicKey()
	if key == nil || key.Type() != lpcrypto.Ed25519 || !conn.RemotePeer().MatchesPublicKey(key) {
		return id, false
	}
	raw, err := key.Raw()
	if err != nil || len(raw) != len(id) {
		return id, false
	}
	copy(id[:], raw)
	return id, true
}

type libp2pRelayRules struct {
	active    []libp2pRelaySpec
	reserving []libp2pRelaySpec
}

func (rules *libp2pRelayRules) acceptsCircuit(id peer.ID) bool {
	for _, relay := range rules.active {
		if relay.info.ID == id {
			return true
		}
	}
	return false
}

func (rules *libp2pRelayRules) reservationPeer(id peer.ID) bool {
	for _, relay := range rules.reserving {
		if relay.info.ID == id {
			return true
		}
	}
	return false
}

func (relay libp2pRelaySpec) matchesLeg(id peer.ID, addr ma.Multiaddr) bool {
	if id != relay.info.ID {
		return false
	}
	base := relay.info.Addrs[0]
	if addr.Equal(base) {
		return true
	}
	// Swarm resolves DNS before invoking the dial gater. The configured
	// transport, port and authenticated relay key still have to match.
	if len(base) != len(addr) || !libp2pDirectAddr(addr, false) || !addr[1:].Equal(base[1:]) {
		return false
	}
	switch base[0].Protocol().Code {
	case ma.P_DNS:
		return true
	case ma.P_DNS4:
		return addr[0].Protocol().Code == ma.P_IP4
	case ma.P_DNS6:
		return addr[0].Protocol().Code == ma.P_IP6
	default:
		return false
	}
}

type libp2pRelayGater struct {
	rules atomic.Pointer[libp2pRelayRules]
	mu    sync.Mutex
	dials map[*libp2pCircuitDial]struct{}
	lan   map[peer.ID][]ma.Multiaddr
}

// Swarm merges peerstore addresses into DialPeer. Narrow direct candidates
// for this peer while the LAN dial is outstanding; circuit admission and all
// already established connections (including concurrent DCUtR) stay intact.
func (gater *libp2pRelayGater) limitLANDial(id peer.ID, allowed []ma.Multiaddr) (func(), bool) {
	gater.mu.Lock()
	defer gater.mu.Unlock()
	if _, busy := gater.lan[id]; busy || gater.rules.Load().reservationPeer(id) || len(allowed) == 0 {
		return nil, false
	}
	if gater.lan == nil {
		gater.lan = make(map[peer.ID][]ma.Multiaddr)
	}
	gater.lan[id] = allowed
	return func() {
		gater.mu.Lock()
		delete(gater.lan, id)
		gater.mu.Unlock()
	}, true
}

type libp2pCircuitDial struct {
	peer   peer.ID
	routes []ma.Multiaddr
}

// A peer's advertised circuit is only eligible during its explicit dial.
// The relay's authenticated leg can then be reused without a local reservation.
func (gater *libp2pRelayGater) authorizeDial(info peer.AddrInfo) func() {
	dial := &libp2pCircuitDial{peer: info.ID}
	for _, addr := range info.Addrs {
		if _, ok := libp2pCircuitRelay(addr); ok {
			dial.routes = append(dial.routes, addr)
		}
	}
	if len(dial.routes) == 0 {
		return func() {}
	}
	gater.mu.Lock()
	if gater.dials == nil {
		gater.dials = make(map[*libp2pCircuitDial]struct{})
	}
	gater.dials[dial] = struct{}{}
	gater.mu.Unlock()
	return func() {
		gater.mu.Lock()
		delete(gater.dials, dial)
		gater.mu.Unlock()
	}
}

func (gater *libp2pRelayGater) allowsDialCircuit(id peer.ID, addr ma.Multiaddr, relay peer.ID) bool {
	gater.mu.Lock()
	defer gater.mu.Unlock()
	for dial := range gater.dials {
		if dial.peer != id {
			continue
		}
		for _, route := range dial.routes {
			if relay == "" {
				if route.Equal(addr) {
					return true
				}
			} else if candidate, _ := libp2pCircuitRelay(route); candidate == relay {
				// The live leg reports the relay's resolved address, not the
				// textual address the peer advertised. Authenticate its ID.
				return true
			}
		}
	}
	return false
}

func (*libp2pRelayGater) InterceptPeerDial(peer.ID) bool              { return true }
func (*libp2pRelayGater) InterceptAccept(network.ConnMultiaddrs) bool { return true }
func (gater *libp2pRelayGater) InterceptSecured(dir network.Direction, id peer.ID, _ network.ConnMultiaddrs) bool {
	return dir != network.DirInbound || !gater.rules.Load().reservationPeer(id)
}

func (gater *libp2pRelayGater) InterceptAddrDial(id peer.ID, addr ma.Multiaddr) bool {
	gater.mu.Lock()
	allowed, limited := gater.lan[id]
	gater.mu.Unlock()
	if limited && libp2pDirectAddr(addr, false) && !slices.ContainsFunc(allowed, addr.Equal) {
		return false
	}
	rules := gater.rules.Load()
	if rules.reservationPeer(id) {
		for _, relay := range rules.reserving {
			if relay.matchesLeg(id, addr) {
				return true
			}
		}
		return false
	}
	if _, err := addr.ValueForProtocol(ma.P_CIRCUIT); err == nil {
		return gater.allowsDialCircuit(id, addr, "")
	}
	return libp2pDirectAddr(addr, false)
}

func (gater *libp2pRelayGater) InterceptUpgraded(conn network.Conn) (bool, control.DisconnectReason) {
	id, addr := conn.RemotePeer(), conn.RemoteMultiaddr()
	rules := gater.rules.Load()
	key := conn.RemotePublicKey()
	if key == nil || !id.MatchesPublicKey(key) {
		return false, 0
	}
	if rules.reservationPeer(id) {
		return conn.Stat().Direction == network.DirOutbound && gater.InterceptAddrDial(id, addr), 0
	}
	if _, err := addr.ValueForProtocol(ma.P_CIRCUIT); err == nil {
		relay, ok := libp2pCircuitRelay(addr)
		if !ok {
			return false, 0
		}
		if conn.Stat().Direction == network.DirInbound {
			return rules.acceptsCircuit(relay), 0
		}
		return gater.allowsDialCircuit(id, nil, relay), 0
	}
	return true, 0
}

func encodeLibp2pAddress(id protocol.NodeID, circuits, direct []ma.Multiaddr) peertransport.Address {
	address := peertransport.Address{ID: id, Addrs: make([]json.RawMessage, 0, libp2pMaxHints)}
	seen := make(map[string]bool)
	appendHint := func(addr ma.Multiaddr) {
		text := addr.String()
		if len(address.Addrs) == libp2pMaxHints || seen[text] {
			return
		}
		seen[text] = true
		hint, _ := json.Marshal(struct{ Libp2p string }{text})
		address.Addrs = append(address.Addrs, hint)
	}
	for _, addr := range circuits {
		appendHint(addr)
	}
	for _, addr := range direct {
		if libp2pDirectAddr(addr, false) && !manet.IsIPLoopback(addr) {
			appendHint(addr)
		}
	}
	for _, addr := range direct {
		if libp2pDirectAddr(addr, false) && manet.IsIPLoopback(addr) {
			appendHint(addr)
		}
	}
	return address
}
