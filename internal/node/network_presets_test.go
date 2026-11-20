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

func TestNetworkPresetsAvailableAtStartupAndOffline(t *testing.T) {
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
	node.networkDirectory.transport = networkPresetRoundTripper(func(*http.Request) (*http.Response, error) {
		requests++
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
	if requests != 0 || initial.Unavailable || len(initial.Presets) != 2 {
		t.Fatalf("startup listing = %#v, requests = %d", initial, requests)
	}
	direct, china := initial.Presets[0], initial.Presets[1]
	if direct.ID != "direct" || direct.Selection.Kind != "direct" || direct.Selection.Data != nil {
		t.Fatalf("direct preset = %#v", direct)
	}
	if china.ID != "china-tencent" || china.Selection.Kind != "custom" || china.Label.Label != "China (Tencent Cloud)" || china.Label.Locales["zh-CN"] != "中国（腾讯云）" {
		t.Fatalf("China preset = %#v", china)
	}
	const chinaRelay = "/dns4/cn1.link.sailry.dux.plus/tcp/4001/p2p/12D3KooWJJnnLjPYHmQ4p7TduxDkuqYkPEjmGNGsiX9QPTf27nap"
	if relays, err := link.NetworkRelayURLs(ctx, china.Selection); err != nil || !slices.Equal(relays, []string{chinaRelay}) {
		t.Fatalf("China relays = %v, %v", relays, err)
	}

	offline := list(true)
	if requests != 1 || !offline.Unavailable || !reflect.DeepEqual(offline.Presets, initial.Presets) {
		t.Fatalf("offline listing = %#v, requests = %d", offline, requests)
	}
	settings := commandFixture(t, node, protocol.Command{Kind: "read_network_settings"}).Data.(protocol.NetworkSettings)
	if settings.Revision != 0 || settings.Selection == nil || settings.Selection.Kind != "direct" ||
		settings.Effective == nil || settings.Effective.Kind != "direct" {
		t.Fatalf("network settings after an offline refresh = %#v", settings)
	}
}

func TestNetworkDirectoryMergesValidatedRefreshByID(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	directory, err := newNetworkDirectory(ctx)
	if err != nil {
		t.Fatal(err)
	}
	bundled := slices.Clone(directory.listing.Presets)
	refresh := func(body string, unavailable bool) []protocol.NetworkPreset {
		t.Helper()
		directory.transport = networkPresetRoundTripper(func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(body))}, nil
		})
		listing, err := directory.read(ctx, true)
		if err != nil || listing.Unavailable != unavailable {
			t.Fatalf("refresh %s = %#v, %v", body, listing, err)
		}
		return listing.Presets
	}
	const updatedRelay = "/dns4/updated-relay.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"
	merged := refresh(`{"version":1,"presets":[
		{"id":"global","label":{"label":"Global"},"selection":{"kind":"direct"}},
		{"id":"china-tencent","label":{"label":"Updated China"},"selection":{"kind":"custom","relays":["`+updatedRelay+`"]}},
		{"id":"direct","label":{"label":"Updated direct"},"selection":{"kind":"direct"}},
		{"id":"other","label":{"label":"Other"},"selection":{"kind":"direct"}}
	]}`, false)
	if len(merged) != 4 || merged[0].ID != "direct" || merged[0].Label.Label != "Updated direct" ||
		merged[1].ID != "china-tencent" || merged[1].Label.Label != "Updated China" || len(merged[1].Label.Locales) != 0 ||
		merged[2].ID != "global" || merged[3].ID != "other" {
		t.Fatalf("merged presets = %#v", merged)
	}
	if relays, err := link.NetworkRelayURLs(ctx, merged[1].Selection); err != nil || !slices.Equal(relays, []string{updatedRelay}) {
		t.Fatalf("updated China relays = %v, %v", relays, err)
	}

	invalid := refresh(`{"version":1,"presets":[
		{"id":"direct","label":{"label":"Partial update"},"selection":{"kind":"direct"}},
		{"id":"china-tencent","label":{"label":"Invalid China"},"selection":{"kind":"custom","relays":["bad-address"]}}
	]}`, true)
	if !reflect.DeepEqual(invalid, merged) {
		t.Fatalf("invalid refresh changed presets: %#v", invalid)
	}

	legacy := refresh(`{"version":1,"presets":[
		{"id":"global","label":{"label":"Old global"},"selection":{"kind":"iroh"}},
		{"id":"china-tencent","label":{"label":"Old China"},"selection":{"kind":"custom","relays":["https://iroh.example"]}}
	]}`, false)
	if !reflect.DeepEqual(legacy, bundled) {
		t.Fatalf("legacy refresh changed bundled presets: %#v", legacy)
	}

	alias := refresh(`{"version":1,"presets":[
		{"id":"direct","label":{"label":"Not direct"},"selection":{"kind":"custom","relays":["`+updatedRelay+`"]}}
	]}`, false)
	if !reflect.DeepEqual(alias, bundled) {
		t.Fatalf("relay hidden behind the direct ID changed bundled presets: %#v", alias)
	}
}
