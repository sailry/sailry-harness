package link

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"reflect"
	"slices"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	testTencentRelayHost = "cn1.link.sailry.dux.plus"
	testTencentRelayIP   = "140.143.238.80"
	testTencentRelayPeer = "/p2p/12D3KooWJJnnLjPYHmQ4p7TduxDkuqYkPEjmGNGsiX9QPTf27nap"
	testTencentRelayAddr = "/dns4/" + testTencentRelayHost + "/tcp/4001" + testTencentRelayPeer
)

type testRelayInputDNS struct {
	txt func(string) ([]string, error)
	ips func(context.Context, string, string) ([]netip.Addr, error)
}

func (dns testRelayInputDNS) LookupTXT(_ context.Context, name string) ([]string, error) {
	return dns.txt(name)
}

func (dns testRelayInputDNS) LookupNetIP(ctx context.Context, family, name string) ([]netip.Addr, error) {
	return dns.ips(ctx, family, name)
}

func testRelayInputSelection(value string) protocol.NetworkSelection {
	return protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{value}}}
}

func testRelayInputPresets(addresses ...string) []protocol.NetworkPreset {
	presets := make([]protocol.NetworkPreset, len(addresses))
	for i, address := range addresses {
		presets[i].Selection = testRelayInputSelection(address)
	}
	return presets
}

func TestResolveRelayInputUsesSuppliedPresets(t *testing.T) {
	t.Parallel()
	const literal = "/ip4/" + testTencentRelayIP + "/tcp/4001" + testTencentRelayPeer
	for _, test := range []struct {
		name     string
		input    string
		want     string
		ipLookup bool
	}{
		{"hostname", testTencentRelayHost, testTencentRelayAddr, false},
		{"hostname and port", "CN1.LINK.SAILRY.DUX.PLUS.:4001", testTencentRelayAddr, false},
		{"https origin", "https://" + testTencentRelayHost + "/", testTencentRelayAddr, false},
		{"IP alias", testTencentRelayIP, testTencentRelayAddr, true},
		{"IP alias and port", testTencentRelayIP + ":4001", testTencentRelayAddr, true},
		{"literal IP", testTencentRelayIP + ":4001", literal, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			presets := testRelayInputPresets(testTencentRelayAddr)
			var dns relayInputDNS
			if test.want == literal {
				presets = testRelayInputPresets(testTencentRelayAddr, literal)
			}
			if test.ipLookup {
				dns = testRelayInputDNS{ips: func(_ context.Context, family, name string) ([]netip.Addr, error) {
					if family != "ip4" || name != testTencentRelayHost+"." {
						t.Fatalf("unexpected lookup %s %s", family, name)
					}
					return []netip.Addr{netip.MustParseAddr(testTencentRelayIP)}, nil
				}}
			}
			draft := testRelayInputSelection(test.input)
			selected, err := resolveNetworkSelection(t.Context(), draft, presets, dns)
			if err != nil || !reflect.DeepEqual(selected, testRelayInputSelection(test.want)) {
				t.Fatalf("selected relay = %+v, %v", selected, err)
			}
			if !slices.Equal(customRelayAddrs(draft.Data), []string{test.input}) {
				t.Fatal("resolution modified the relay draft")
			}
		})
	}
}

func TestResolveRelayInputDoesNotRestoreUnlistedRelay(t *testing.T) {
	t.Parallel()
	dns := testRelayInputDNS{txt: func(name string) ([]string, error) {
		if name != "_dnsaddr."+testTencentRelayHost+"." {
			t.Fatalf("unexpected TXT query %q", name)
		}
		return nil, &net.DNSError{Name: name, IsNotFound: true}
	}}
	for _, value := range []string{testTencentRelayHost + ":4001", testTencentRelayIP + ":4001"} {
		if _, err := resolveNetworkSelection(t.Context(), testRelayInputSelection(value), nil, dns); faultValue(err).Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("unlisted relay %q = %v", value, err)
		}
	}
}

func TestResolveRelayInputDisambiguatesPresetPorts(t *testing.T) {
	t.Parallel()
	const second = "/dns4/" + testTencentRelayHost + "/udp/4002/quic-v1" + testTencentRelayPeer
	presets := testRelayInputPresets(testTencentRelayAddr, testTencentRelayAddr, second)
	for _, host := range []string{testTencentRelayHost, testTencentRelayIP} {
		for _, test := range []struct{ suffix, want string }{
			{"", ""}, {":4001", testTencentRelayAddr}, {":4002", second},
		} {
			t.Run(host+test.suffix, func(t *testing.T) {
				lookups := 0
				dns := testRelayInputDNS{ips: func(_ context.Context, family, name string) ([]netip.Addr, error) {
					if host != testTencentRelayIP || family != "ip4" || name != testTencentRelayHost+"." {
						t.Fatalf("unexpected lookup %s %s", family, name)
					}
					lookups++
					return []netip.Addr{netip.MustParseAddr(testTencentRelayIP)}, nil
				}}
				selected, err := resolveNetworkSelection(t.Context(), testRelayInputSelection(host+test.suffix), presets, dns)
				if host == testTencentRelayIP && lookups != 1 {
					t.Fatalf("looked up the same name %d times", lookups)
				}
				if test.want == "" {
					if faultValue(err).Code != protocol.ErrorCodeInvalidRequest {
						t.Fatalf("ambiguous preset relay = %+v, %v", selected, err)
					}
				} else if err != nil || !reflect.DeepEqual(selected, testRelayInputSelection(test.want)) {
					t.Fatalf("selected preset relay = %+v, %v", selected, err)
				}
			})
		}
	}
}

func TestResolveRelayIPRequiresCompleteDNSMatch(t *testing.T) {
	t.Parallel()
	const ip = "2001:db8::7"
	const first = "/dns6/relay.example/tcp/4001" + testTencentRelayPeer
	const second = "/dns/other.example/tcp/4001" + testTencentRelayPeer
	for _, test := range []struct {
		name      string
		lookupErr error
		wantCode  protocol.ErrorCode
		cancel    bool
	}{
		{name: "one match", lookupErr: &net.DNSError{IsNotFound: true}},
		{name: "two matches at the same port", wantCode: protocol.ErrorCodeInvalidRequest},
		{name: "incomplete results", lookupErr: &net.DNSError{IsTemporary: true}, wantCode: protocol.ErrorCodeUnavailable},
		{name: "canceled", cancel: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			presets := testRelayInputPresets(
				"/dns4/wrong-family.example/tcp/4001"+testTencentRelayPeer,
				"/dns6/wrong-port.example/tcp/4002"+testTencentRelayPeer,
				first, second,
			)
			if test.cancel {
				presets = append(presets, testRelayInputPresets("/dns6/after-cancel.example/tcp/4001"+testTencentRelayPeer)...)
			}
			var deadline time.Time
			var queries []string
			dns := testRelayInputDNS{ips: func(ctx context.Context, family, name string) ([]netip.Addr, error) {
				if family != "ip6" || name != "relay.example." && name != "other.example." {
					t.Fatalf("unexpected lookup %s %s", family, name)
				}
				limit, ok := ctx.Deadline()
				if !ok || time.Until(limit) > ioTimeout || !deadline.IsZero() && !limit.Equal(deadline) {
					t.Fatalf("lookup deadline %v is not bounded by a shared deadline (first: %v)", limit, deadline)
				}
				deadline = limit
				queries = append(queries, name)
				if name == "other.example." {
					if test.cancel {
						cancel()
						return nil, nil
					}
					if test.lookupErr != nil {
						return nil, test.lookupErr
					}
				}
				return []netip.Addr{netip.MustParseAddr(ip)}, nil
			}}
			selected, err := resolveNetworkSelection(ctx, testRelayInputSelection("["+ip+"]:4001"), presets, dns)
			if !slices.Equal(queries, []string{"relay.example.", "other.example."}) {
				t.Fatalf("DNS queries = %v", queries)
			}
			switch {
			case test.cancel:
				if !errors.Is(err, context.Canceled) {
					t.Fatalf("canceled lookup = %+v, %v", selected, err)
				}
			case test.wantCode != "":
				if err == nil || faultValue(err).Code != test.wantCode {
					t.Fatalf("selected relay = %+v, %v; want %s", selected, err, test.wantCode)
				}
			default:
				if err != nil || !reflect.DeepEqual(selected, testRelayInputSelection(first)) {
					t.Fatalf("selected IPv6 relay = %+v, %v", selected, err)
				}
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
		return []string{"other text", "dnsaddr=" + base + "/tcp/4001", "dnsaddr=" + first, "dnsaddr=" + first, "dnsaddr=" + second}, nil
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
