package link

import (
	"context"
	"net"
	"net/netip"
	"reflect"
	"slices"
	"testing"

	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type testRelayInputDNS struct {
	txt func(string) ([]string, error)
	ips func(string, string) ([]netip.Addr, error)
}

func (dns testRelayInputDNS) LookupTXT(_ context.Context, name string) ([]string, error) {
	return dns.txt(name)
}

func (dns testRelayInputDNS) LookupNetIP(_ context.Context, family, name string) ([]netip.Addr, error) {
	return dns.ips(family, name)
}

func testRelayInputSelection(value string) protocol.NetworkSelection {
	return protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{value}}}
}

func TestResolveBundledRelayInput(t *testing.T) {
	t.Parallel()
	const hostname = "cn1.link.sailry.dux.plus"
	const ip = "140.143.238.80"
	known, err := matchingRelayPreset(bundledRelayPresets(), "4001", func(base ma.Multiaddr) bool { return base[0].Value() == hostname })
	if err != nil || known == "" {
		t.Fatalf("bundled China relay = %q, %v", known, err)
	}
	for _, input := range []string{hostname, "CN1.LINK.SAILRY.DUX.PLUS.:4001", "https://" + hostname + "/", ip, ip + ":4001"} {
		t.Run(input, func(t *testing.T) {
			dns := testRelayInputDNS{ips: func(family, name string) ([]netip.Addr, error) {
				if family != "ip4" || name != hostname+"." {
					t.Fatalf("unexpected lookup %s %s", family, name)
				}
				return []netip.Addr{netip.MustParseAddr(ip)}, nil
			}}
			draft := testRelayInputSelection(input)
			selected, err := resolveNetworkSelection(t.Context(), draft, nil, dns)
			if err != nil || !reflect.DeepEqual(selected, testRelayInputSelection(known)) {
				t.Fatalf("selected relay = %+v, %v", selected, err)
			}
			if !slices.Equal(customRelayAddrs(draft.Data), []string{input}) {
				t.Fatal("resolution modified the relay draft")
			}
		})
	}
}

func TestResolveRelayInputUsesDNSADDRAndPort(t *testing.T) {
	t.Parallel()
	const host = "myrelay.example"
	const base = "/dns4/" + host
	const peer = "/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"
	const first = base + "/tcp/4001" + peer
	const second = base + "/udp/4002/quic-v1" + peer
	dns := testRelayInputDNS{txt: func(name string) ([]string, error) {
		if name != "_dnsaddr."+host+"." {
			t.Fatalf("unexpected TXT query %q", name)
		}
		return []string{"other text", "dnsaddr=" + base + "/tcp/4001", "dnsaddr=" + first, "dnsaddr=" + second}, nil
	}}
	selected, err := resolveNetworkSelection(t.Context(), testRelayInputSelection(host+":4001"), nil, dns)
	if err != nil || !reflect.DeepEqual(selected, testRelayInputSelection(first)) {
		t.Fatalf("selected DNS relay = %+v, %v", selected, err)
	}
	for _, value := range []string{host, host + ":4003"} {
		if _, err := resolveNetworkSelection(t.Context(), testRelayInputSelection(value), nil, dns); faultValue(err).Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("missing or ambiguous port %q = %v", value, err)
		}
	}
}

func TestResolveRelayInputKeepsExplicitSelectionAndRejectsInvalidAliases(t *testing.T) {
	t.Parallel()
	for _, sel := range []protocol.NetworkSelection{{Kind: "direct"}, testRelayInputSelection(fixtureRelayTCP)} {
		selection, err := resolveNetworkSelection(t.Context(), sel, nil, nil)
		if err != nil || !reflect.DeepEqual(selection, sel) {
			t.Fatalf("existing selection changed: %+v, %v", selection, err)
		}
	}
	for _, value := range []string{"https://user:pass@myrelay.example", "https://myrelay.example/ping", "myrelay.example:0", "myrelay.example:65536", "http://myrelay.example", "myrelay..example:4001"} {
		if _, err := resolveNetworkSelection(t.Context(), testRelayInputSelection(value), nil, nil); faultValue(err).Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("invalid alias %q = %v", value, err)
		}
	}
	dns := testRelayInputDNS{txt: func(name string) ([]string, error) {
		return nil, &net.DNSError{Name: name, IsNotFound: true}
	}}
	if _, err := resolveNetworkSelection(t.Context(), testRelayInputSelection("myrelay.example:4001"), nil, dns); faultValue(err).Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("alias without DNSADDR = %v", err)
	}
}
