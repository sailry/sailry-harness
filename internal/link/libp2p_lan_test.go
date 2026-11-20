package link

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/libp2p/go-libp2p/core/host"
	"github.com/libp2p/go-libp2p/core/network"
	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"
)

func TestLANCandidatesAreExplicitAndOnLink(t *testing.T) {
	prefixes := []netip.Prefix{netip.MustParsePrefix("192.168.1.42/24"), netip.MustParsePrefix("fd12:3456::a/64")}
	for _, addr := range []string{
		"/ip4/127.0.0.1/tcp/9000", "/ip4/0.0.0.0/tcp/9000", "/ip4/224.0.0.1/tcp/9000", "/ip4/8.8.8.8/tcp/53",
		"/ip4/10.0.0.1/tcp/9000", "/ip4/192.168.10.22/tcp/9000", "/ip4/192.168.1.0/tcp/9000", "/ip4/192.168.1.255/tcp/9000",
		"/ip6/::1/tcp/9000", "/ip6/fe80::1/tcp/9000", "/ip6/fd12:3456:1::1/tcp/9000", "/ip6/::ffff:192.168.1.2/tcp/9000",
		"/dns4/host.local/tcp/9000", "/ip4/192.168.1.2/tcp/0", "/ip4/192.168.1.2/tcp/9000/ws", "/ip4/192.168.1.2/udp/9000",
		fixtureRelayTCP + "/p2p-circuit", strings.Repeat("x", libp2pMaxHintBytes+1),
	} {
		if got := lanCandidates([]string{addr}, prefixes); len(got) != 0 {
			t.Errorf("accepted %q: %v", addr, got)
		}
	}
	tcp, quic := "/ip4/192.168.1.2/tcp/9000", "/ip6/fd12:3456::b/udp/9000/quic-v1"
	got := lanCandidates([]string{tcp, quic, tcp}, prefixes)
	if len(got) != 2 || got[0].String() != tcp || got[1].String() != quic {
		t.Fatalf("candidates: %v", got)
	}
	if got := lanCandidates(slices.Repeat([]string{tcp}, libp2pLANLimit+1), prefixes); len(got) != 0 {
		t.Fatal("oversized listener list accepted")
	}
}

type lanTestNetwork struct {
	network.Network
	listen []ma.Multiaddr
}

func (n lanTestNetwork) ListenAddresses() []ma.Multiaddr { return n.listen }

type lanTestHost struct {
	host.Host
	network network.Network
}

func (h lanTestHost) Network() network.Network { return h.network }

func TestLANAdvertisesOnlyLiveLocalListeners(t *testing.T) {
	ep := &libp2pEndpoint{host: lanTestHost{network: lanTestNetwork{listen: []ma.Multiaddr{
		ma.StringCast("/ip4/0.0.0.0/tcp/23001"), ma.StringCast("/ip6/::/udp/23002/quic-v1"), ma.StringCast("/ip4/10.5.1.4/tcp/23003"),
	}}}}
	var addrs []net.Addr
	for _, text := range []string{"192.168.1.42/24", "fd12:3456::a/64", "8.8.8.8/24", "127.0.0.1/8", "10.5.1.4/0"} {
		ip, prefix, err := net.ParseCIDR(text)
		if err != nil {
			t.Fatal(err)
		}
		prefix.IP = ip
		addrs = append(addrs, prefix)
	}
	ep.lanInterfaces = func() ([]net.Addr, error) { return addrs, nil }
	want := []string{"/ip4/192.168.1.42/tcp/23001", "/ip6/fd12:3456::a/udp/23002/quic-v1"}
	if got := ep.lanListeners(); !slices.Equal(got, want) {
		t.Fatalf("advertised %v, want %v", got, want)
	}
	ep.lanInterfaces = func() ([]net.Addr, error) { return nil, errors.New("interfaces unavailable") }
	if got := ep.lanListeners(); len(got) != 0 {
		t.Fatalf("guessed listeners: %v", got)
	}
}

func TestLANGaterLimitsOnlyTheActivePeerAndKeepsCircuit(t *testing.T) {
	var gate libp2pRelayGater
	gate.rules.Store(&libp2pRelayRules{})
	const target, other peer.ID = "target", "other"
	route, err := parseLibp2pRelay(fixtureRelayTCP)
	if err != nil {
		t.Fatal(err)
	}
	releaseCircuit := gate.authorizeDial(peer.AddrInfo{ID: target, Addrs: []ma.Multiaddr{route.circuit}})
	defer releaseCircuit()
	lan := ma.StringCast("/ip4/192.168.1.2/tcp/23001")
	finish, ok := gate.limitLANDial(target, []ma.Multiaddr{lan})
	if !ok {
		t.Fatal("first LAN dial not admitted")
	}
	if _, ok := gate.limitLANDial(target, []ma.Multiaddr{lan}); ok {
		t.Fatal("concurrent LAN dial replaced the allowlist")
	}
	if !gate.InterceptAddrDial(target, lan) || !gate.InterceptAddrDial(target, route.circuit) {
		t.Fatal("lost allowed path")
	}
	for _, raw := range []string{"/ip4/192.168.1.2/tcp/23002", "/ip4/192.168.1.3/tcp/23001", "/ip4/192.168.1.2/udp/23001/quic-v1", "/ip4/8.8.8.8/tcp/443", "/ip4/127.0.0.1/tcp/1000"} {
		addr := ma.StringCast(raw)
		if gate.InterceptAddrDial(target, addr) || !gate.InterceptAddrDial(other, addr) {
			t.Errorf("unexpected scope for %s", raw)
		}
	}
	finish()
	if !gate.InterceptAddrDial(target, ma.StringCast("/ip4/8.8.8.8/tcp/443")) {
		t.Fatal("ordinary direct dials remain restricted")
	}
}

func TestLANWatcherEndsWithRevokedSlot(t *testing.T) {
	link := newFixtureLink(t)
	ep := &libp2pEndpoint{}
	conn := &libp2pConnection{endpoint: ep, ctx: t.Context()}
	slot, revoke := context.WithCancel(t.Context())
	link.watchLAN(slot, conn, nil)
	revoke()
	done := make(chan struct{})
	go func() { ep.peerWork.Wait(); close(done) }()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("LAN worker outlived its slot on a retained connection")
	}
}
