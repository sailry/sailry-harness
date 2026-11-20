package node

import (
	"context"
	"errors"
	"io"
	"net/http"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type networkPresetRoundTripper func(*http.Request) (*http.Response, error)

func (transport networkPresetRoundTripper) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

func TestNetworkPresetsEmptyAtStartupAndOffline(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment,
		MCP:         fixtureMCPOptions(),
		Network:     &link.NetworkScope{Internet: true, Bind: "127.0.0.1:0"},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	requests := 0
	node.networkDirectory.transport = networkPresetRoundTripper(func(request *http.Request) (*http.Response, error) {
		requests++
		if url := request.URL.String(); url != "https://sailry.dev/catalogs/network-presets-libp2p.json" {
			t.Errorf("catalog request URL = %q", url)
		}
		return nil, errors.New("offline")
	})
	list := func(refresh bool) protocol.NetworkListing {
		t.Helper()
		output := commandFixture(t, node, protocol.Command{
			Kind: "list_network_presets",
			Data: protocol.CommandListNetworkPresetsPayload{Refresh: refresh},
		})
		listing, ok := output.Data.(protocol.NetworkListing)
		if !ok || output.Kind != "network_presets" {
			t.Fatalf("list network presets = %#v", output)
		}
		return listing
	}

	initial := list(false)
	if requests != 0 || initial.Unavailable || len(initial.Presets) != 0 {
		t.Fatalf("startup listing = %#v, requests = %d", initial, requests)
	}

	offline := list(true)
	if requests != 1 || !offline.Unavailable || len(offline.Presets) != 0 {
		t.Fatalf("offline listing = %#v, requests = %d", offline, requests)
	}
	settings := commandFixture(t, node, protocol.Command{Kind: "read_network_settings"}).Data.(protocol.NetworkSettings)
	if settings.Revision != 0 || settings.Selection == nil || settings.Selection.Kind != "direct" ||
		settings.Effective == nil || settings.Effective.Kind != "direct" {
		t.Fatalf("network settings after an offline refresh = %#v", settings)
	}
}

func TestNetworkDirectoryReplacesOnlyWithValidRefresh(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	const relay = "/dns4/updated-relay.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"
	const china = `{"id":"china-tencent","label":{"label":"China (Tencent Cloud)"},"selection":{"kind":"custom","relays":["` + relay + `"]}}`
	const other = `{"id":"other","label":{"label":"Other"},"selection":{"kind":"custom","relays":["` + relay + `"]}}`
	const ignored = `
		{"id":"direct","label":{"label":"Reserved direct"},"selection":{"kind":"custom","relays":["` + relay + `"]}},
		{"id":"custom","label":{"label":"Reserved custom"},"selection":{"kind":"custom","relays":["` + relay + `"]}},
		{"id":"direct-alias","label":{"label":"Direct alias"},"selection":{"kind":"direct"}},
		{"id":"legacy-iroh","label":{"label":"Iroh"},"selection":{"kind":"iroh"}},
		{"id":"legacy-https","label":{"label":"HTTPS"},"selection":{"kind":"custom","relays":["https://iroh.example"]}}`
	catalog := func(entries string) string { return `{"version":1,"presets":[` + entries + `]}` }
	var body string
	var fetchErr error
	directory := &networkDirectory{
		transport: networkPresetRoundTripper(func(*http.Request) (*http.Response, error) {
			if fetchErr != nil {
				return nil, fetchErr
			}
			return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(body))}, nil
		}),
	}
	refresh := func(unavailable bool) []protocol.NetworkPreset {
		t.Helper()
		listing, err := directory.read(ctx, true)
		if err != nil || listing.Unavailable != unavailable {
			t.Fatalf("refresh %s = %#v, %v", body, listing, err)
		}
		return listing.Presets
	}

	body = catalog(china + "," + ignored + "," + other)
	loaded := slices.Clone(refresh(false))
	if len(loaded) != 2 || loaded[0].ID != "china-tencent" || loaded[0].Label.Label != "China (Tencent Cloud)" || loaded[1].ID != "other" {
		t.Fatalf("loaded presets = %#v", loaded)
	}
	if relays, err := link.NetworkRelayURLs(ctx, loaded[0].Selection); err != nil || !slices.Equal(relays, []string{relay}) {
		t.Fatalf("China relays = %v, %v", relays, err)
	}

	fetchErr = errors.New("offline")
	if offline := refresh(true); !reflect.DeepEqual(offline, loaded) {
		t.Fatalf("offline refresh changed presets: %#v", offline)
	}
	fetchErr = nil
	for _, invalid := range []struct{ name, body string }{
		{"invalid relay", catalog(other + `,{"id":"malformed","label":{"label":"Malformed"},"selection":{"kind":"custom","relays":["bad-address"]}}`)},
		{"missing presets", `{"version":1}`},
		{"null presets", `{"version":1,"presets":null}`},
		{"only ignored presets", catalog(ignored)},
	} {
		body = invalid.body
		if got := refresh(true); !reflect.DeepEqual(got, loaded) {
			t.Errorf("refresh with %s changed presets: %#v", invalid.name, got)
		}
	}

	body = catalog(other)
	if replaced := refresh(false); !reflect.DeepEqual(replaced, loaded[1:]) {
		t.Fatalf("refresh did not remove missing preset: %#v", replaced)
	}
	body = catalog("")
	if empty := refresh(false); len(empty) != 0 {
		t.Fatalf("empty catalog did not clear presets: %#v", empty)
	}
}
