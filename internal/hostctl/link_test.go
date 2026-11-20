package hostctl

import (
	"bufio"
	"bytes"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSelectNetworkUsesHostCatalogAndCustomStep(t *testing.T) {
	t.Parallel()
	listing := protocol.NetworkListing{Presets: []protocol.NetworkPreset{{
		ID: "edgeone", Label: protocol.PluginDesktopNavigation{Label: "China EdgeOne", Locales: map[string]string{"zh-CN": "中国（EdgeOne）"}},
		Selection: protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"/dns4/relay.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"}}},
	}}}
	input := bufio.NewReader(strings.NewReader("custom\n/dns4/custom.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV\n\n"))
	var output bytes.Buffer
	selected, err := selectNetwork(t.Context(), input, &output, listing, "", "", true)
	if err != nil {
		t.Fatal(err)
	}
	got, err := link.NetworkRelayURLs(t.Context(), selected)
	if err != nil || len(got) != 1 || got[0] != "/dns4/custom.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV" {
		t.Fatalf("selected custom network = %v, %v", got, err)
	}
	if !strings.Contains(output.String(), "China EdgeOne") || !strings.Contains(output.String(), "Custom relay") || !strings.Contains(output.String(), "Press Enter") {
		t.Fatalf("wizard did not show catalog and confirmation steps: %q", output.String())
	}
}

func TestSelectNetworkResolvesCatalogHostnameAndPort(t *testing.T) {
	t.Parallel()
	const address = "/dns4/relay.example/tcp/4001/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"
	selection := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{address}}}
	listing := protocol.NetworkListing{Presets: []protocol.NetworkPreset{{ID: "region", Label: protocol.PluginDesktopNavigation{Label: "Region"}, Selection: selection}}}
	for _, input := range []string{"relay.example:4001", "https://relay.example"} {
		selected, err := selectNetwork(t.Context(), nil, &bytes.Buffer{}, listing, "custom", input, false)
		if err != nil || !equalNetworkSelection(&selected, &selection) {
			t.Fatalf("custom %q = %+v, %v", input, selected, err)
		}
	}
}

func TestSelectNetworkRequiresExplicitChoiceAndValidCustomAddress(t *testing.T) {
	t.Parallel()
	listing := protocol.NetworkListing{Presets: []protocol.NetworkPreset{{
		ID: "direct", Label: protocol.PluginDesktopNavigation{Label: "Direct only"}, Selection: protocol.NetworkSelection{Kind: "direct"},
	}}}
	for _, row := range []struct{ relay, address string }{{"missing", ""}, {"custom", "http://relay.example"}, {"custom", "https://user:pass@relay.example"}} {
		_, err := selectNetwork(t.Context(), bufio.NewReader(strings.NewReader("")), &bytes.Buffer{}, listing, row.relay, row.address, false)
		if err == nil {
			t.Fatalf("accepted relay=%q address=%q", row.relay, row.address)
		}
	}
}

func TestReportShareShowsHumanExpiryWithoutUnixTimestamp(t *testing.T) {
	t.Parallel()
	var output bytes.Buffer
	err := reportShare(link.ShareState{Kind: "ready", Code: "493243", ExpiresAtMS: uint64(time.Now().Add(58 * time.Second).UnixMilli())}, &output, &bytes.Buffer{})
	if err != nil {
		t.Fatal(err)
	}
	text := output.String()
	if !strings.Contains(text, "Pairing code: 493243") || !strings.Contains(text, "expires in") || strings.Contains(text, "Unix ms") {
		t.Fatalf("pairing output is not human readable: %q", text)
	}
}
