package peerrelay

import (
	"crypto/ed25519"
	"testing"

	"github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"
)

func TestRelayAdmission(t *testing.T) {
	ids := make([]peer.ID, 4)
	for i := range ids {
		seed := [ed25519.SeedSize]byte{byte(i)}
		key, err := crypto.UnmarshalEd25519PrivateKey(ed25519.NewKeyFromSeed(seed[:]))
		if err != nil {
			t.Fatal(err)
		}
		ids[i], err = peer.IDFromPrivateKey(key)
		if err != nil {
			t.Fatal(err)
		}
	}
	self, first, second, outsider := ids[0], ids[1], ids[2], ids[3]
	for _, invalid := range [][]peer.ID{nil, {first, "invalid"}, {first, self}} {
		if _, err := newAllowlist(invalid, self); err == nil {
			t.Fatalf("admitted invalid allowlist %v", invalid)
		}
	}
	allowed, err := newAllowlist([]peer.ID{first, second}, self)
	if err != nil {
		t.Fatal(err)
	}
	if allowed.AllowReserve(outsider, nil) || !allowed.AllowReserve(first, nil) {
		t.Fatal("reservation admission does not match the allowlist")
	}
	if allowed.AllowConnect(outsider, nil, second) || allowed.AllowConnect(first, nil, outsider) || !allowed.AllowConnect(first, nil, second) {
		t.Fatal("circuit admission did not enforce the allowlist at both ends")
	}
}

func TestRelayAddressValidation(t *testing.T) {
	for _, tc := range []struct {
		name      string
		listen    string
		announce  string
		transport int
	}{
		{name: "local TCP", listen: "/ip4/127.0.0.1/tcp/0", transport: ma.P_TCP},
		{name: "local QUIC", listen: "/ip6/::1/udp/0/quic-v1", transport: ma.P_QUIC_V1},
		{name: "translated TCP port", listen: "/ip4/0.0.0.0/tcp/4001", announce: "/ip4/192.0.2.1/tcp/443", transport: ma.P_TCP},
		{name: "wildcard QUIC with announce", listen: "/ip6/::/udp/443/quic-v1", announce: "/ip6/2001:db8::1/udp/443/quic-v1", transport: ma.P_QUIC_V1},
		{name: "wildcard without announce", listen: "/ip4/0.0.0.0/tcp/4001"},
		{name: "wildcard with loopback announce", listen: "/ip6/::/udp/443/quic-v1", announce: "/ip6/::1/udp/443/quic-v1"},
		{name: "wildcard with mapped loopback announce", listen: "/ip6/::/tcp/443", announce: "/ip6/::ffff:127.0.0.1/tcp/443"},
		{name: "wildcard announce", listen: "/ip4/0.0.0.0/tcp/4001", announce: "/ip4/0.0.0.0/tcp/4001"},
		{name: "incompatible transport", listen: "/ip4/0.0.0.0/tcp/4001", announce: "/ip4/192.0.2.1/udp/4001/quic-v1"},
		{name: "zero announce port", listen: "/ip4/0.0.0.0/tcp/4001", announce: "/ip4/192.0.2.1/tcp/0"},
		{name: "loopback with external announce", listen: "/ip4/127.0.0.1/tcp/4001", announce: "/ip4/192.0.2.1/tcp/4001"},
		{name: "unsupported transport", listen: "/ip4/127.0.0.1/tcp/4001/ws"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var announce ma.Multiaddr
			if tc.announce != "" {
				announce = ma.StringCast(tc.announce)
			}
			got, err := validateAddresses(ma.StringCast(tc.listen), announce)
			if tc.transport == 0 {
				if err == nil {
					t.Fatal("invalid address configuration was accepted")
				}
			} else if err != nil || got != tc.transport {
				t.Fatalf("transport = %d, error = %v; want %d", got, err, tc.transport)
			}
		})
	}
}
