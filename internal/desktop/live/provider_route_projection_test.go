package live

import (
	"bytes"
	"encoding/json"
	"reflect"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderRouteProjection(t *testing.T) {
	for _, test := range []struct {
		name   string
		api    protocol.ConversationModelApi
		route  *protocol.ConversationPresetsRouteKey
		method string
	}{
		{name: "Responses without a route", api: protocol.ConversationModelApiResponses, method: "builtin:compatible-responses/0"},
		{name: "Chat Completions without a route", api: protocol.ConversationModelApiChatCompletions, method: "builtin:compatible-responses/1"},
		{name: "explicit Chat Completions route", api: protocol.ConversationModelApiChatCompletions,
			route: &protocol.ConversationPresetsRouteKey{Protocol: protocol.ConversationPresetsProtocolOpenAiChatCompletions}, method: "builtin:compatible-responses/1"},
	} {
		t.Run(test.name, func(t *testing.T) {
			provider := providerRouteFixture()
			provider.Api, provider.Preset.Route = test.api, test.route
			listing := providers.Listing(protocol.ConversationPresetsCatalog{}, false)
			channel, _ := projectProviderRouteFixture(t, provider, listing)
			want := protocol.NodeID{1}.String() + "/" + test.method
			if channel.Method != want {
				t.Fatalf("projected method = %q, want %q", channel.Method, want)
			}
		})
	}
}

func TestProviderProjectionKeepsNoAuthAndRevision(t *testing.T) {
	provider := providerRouteFixture()
	provider.Credential = nil
	listing := providers.Listing(protocol.ConversationPresetsCatalog{}, false)
	for i := range listing.Entries {
		if listing.Entries[i].Preset.ID == provider.Preset.ID {
			listing.Entries[i].Preset.Auth = protocol.ConversationPresetsAuth{Kind: "none"}
		}
	}
	channel, _ := projectProviderRouteFixture(t, provider, listing)
	if channel.Auth != viewmodel.AuthNone || channel.TargetRevisions[protocol.NodeID{1}.String()] != provider.Revision {
		t.Fatalf("no-auth channel projection = %+v", channel)
	}
}

func TestLegacyCompatibleProviderRouteProjection(t *testing.T) {
	for _, test := range []struct {
		name   string
		route  *protocol.ConversationPresetsRouteKey
		target string
	}{
		{name: "without a route", target: "offered"},
		{name: "explicit Chat Completions route", target: "offered", route: &protocol.ConversationPresetsRouteKey{Protocol: protocol.ConversationPresetsProtocolOpenAiChatCompletions}},
		{name: "replacement is not offered", target: "unoffered"},
		{name: "replacement is unavailable", target: "missing"},
	} {
		t.Run(test.name, func(t *testing.T) {
			provider := providerRouteFixture()
			provider.Preset.ID, provider.Preset.Route = "compatible-chat", test.route
			listing := providers.Listing(protocol.ConversationPresetsCatalog{}, false)
			if test.target == "missing" {
				listing.Entries = slices.DeleteFunc(listing.Entries, func(entry protocol.ConversationPresetsEntry) bool {
					return entry.Preset.ID == "compatible-responses"
				})
			} else if test.target == "unoffered" {
				for i := range listing.Entries {
					if listing.Entries[i].Preset.ID == "compatible-responses" {
						listing.Entries[i].Preset.Offered = false
					}
				}
			}
			channel, brands := projectProviderRouteFixture(t, provider, listing)
			node := protocol.NodeID{1}.String()
			if test.target != "offered" {
				if channel.Method != node+"/builtin:compatible-chat/0" {
					t.Fatalf("legacy method = %q without an offered replacement", channel.Method)
				}
				return
			}
			if channel.Method != node+"/builtin:compatible-responses/1" {
				t.Fatalf("legacy method = %q, want the offered Chat Completions route", channel.Method)
			}
			for _, brand := range brands {
				for _, method := range brand.Methods {
					for _, route := range method.Routes {
						if route.ID == channel.Method {
							if route.API != channel.API || len(method.Routes) != 2 {
								t.Fatalf("legacy provider cannot switch its protocol through %+v", method)
							}
							return
						}
					}
				}
			}
			t.Fatal("the projected legacy route is not offered in the editor")
		})
	}
}

func providerRouteFixture() protocol.ConversationProvider {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Name = protocol.ProviderID{3}, 8, "Configured gateway"
	provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey
	provider.Endpoint = "https://gateway.example.test/v1"
	provider.Preset = &protocol.ConversationPresetsPresetRef{Source: protocol.ConversationPresetsSource{Kind: "builtin"}, ID: "compatible-responses"}
	provider.Credential = &protocol.CredentialRef{Node: protocol.NodeID{1}, ID: protocol.CredentialID{2}}
	first := protocol.DefaultConversationModel()
	first.ID, first.Context, first.Output = "first", 128000, 16384
	second := protocol.DefaultConversationModel()
	name, compaction := "Gateway reasoner", uint32(4096)
	second.ID, second.DisplayName, second.Context, second.Output, second.CompactionTokens = "reasoner", &name, 240000, 32768, &compaction
	second.Vision, second.Tools, second.Reasoning, second.CustomCapabilities, second.CustomEfforts = true, true, true, true, true
	second.Efforts, second.DefaultEffort = []protocol.Effort{{Kind: "low"}, {Kind: "high"}}, protocol.Effort{Kind: "high"}
	second.Capabilities = &protocol.ConversationDiscoveryCapabilities{Options: map[string]json.RawMessage{"adapter": json.RawMessage(`"preserved"`)}}
	provider.Models, provider.DefaultModel = []protocol.ConversationModel{first, second}, second.ID
	return provider
}

func projectProviderRouteFixture(t *testing.T, provider protocol.ConversationProvider, listing protocol.ConversationPresetsListing) (viewmodel.Channel, []viewmodel.Brand) {
	t.Helper()
	before, err := json.Marshal(provider)
	if err != nil {
		t.Fatal(err)
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Providers = protocol.NodeID{1}, []protocol.ConversationProvider{provider}
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	defer controller.Close()
	app := &shell.App{Settings: settings.New()}
	app.Page = shell.SettingsPage
	app.Settings.Open(settings.Providers)
	r := &Runtime{ctx: t.Context(), controller: controller, snapshotRead: true, snapshotCopy: snapshot,
		authorizations: &authorizationState{credentialLoad: true, credentials: map[protocol.CredentialID]protocol.Credential{}},
		App:            app, Preferences: &preferences.Store{Data: preferences.Default()}, providerPresetsLoaded: true, media: mediaState{reading: true}}
	r.applyPresets(listing)
	r.projectProviderSnapshot()
	if len(r.App.Settings.Channels) != 1 {
		t.Fatalf("projected channels = %+v", r.App.Settings.Channels)
	}
	channel := r.App.Settings.Channels[0]
	if channel.ID != provider.ID.String() || channel.Revision != provider.Revision || channel.Name != provider.Name || channel.API != string(provider.Api) ||
		channel.Endpoint != provider.Endpoint || channel.Enabled != provider.Enabled || (provider.Credential != nil) != channel.KeySet || channel.Default != 1 {
		t.Fatalf("provider configuration changed in the projection: %+v", channel)
	}
	wantModels := []viewmodel.ChannelModel{channelModel(provider.Models[0]), channelModel(provider.Models[1])}
	if !reflect.DeepEqual(channel.Models, wantModels) {
		t.Fatalf("projected model metadata changed: %+v", channel.Models)
	}
	after, err := json.Marshal(r.snapshotCopy.Providers[0])
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(before, after) {
		t.Fatal("route projection changed the saved provider, credential or model metadata")
	}
	return channel, r.App.Settings.Brands
}

func TestPackagePresetCredentialStatus(t *testing.T) {
	pkg := protocol.ConversationPresetsSourcePackagePayload{Name: "fixture"}
	p := providerRouteFixture()
	p.Preset.Source = protocol.ConversationPresetsSource{Kind: "package", Data: &pkg}
	p.Api, p.Credential = protocol.ConversationModelApiPlugin, nil
	entry := protocol.DefaultConversationPresetsEntry()
	entry.Source = protocol.ConversationPresetsSource{Kind: "package", Data: pkg}
	entry.Package = &protocol.PluginReference{Name: pkg.Name}
	entry.Preset.ID = p.Preset.ID
	entry.Preset.Routes = []protocol.ConversationPresetsRoute{{Protocol: protocol.ConversationPresetsProtocolPlugin}}
	for _, tc := range []struct {
		kind   string
		auth   viewmodel.Auth
		status string
	}{
		{"none", viewmodel.AuthNone, ""}, {"api_key", viewmodel.AuthKey, "provider_status_missing_key"}, {"", viewmodel.AuthUnresolved, ""},
	} {
		listing := protocol.DefaultConversationPresetsListing()
		if tc.kind != "" {
			entry.Preset.Auth.Kind = tc.kind
			listing.Entries = []protocol.ConversationPresetsEntry{entry}
		}
		ch, _ := projectProviderRouteFixture(t, p, listing)
		if ch.Auth != tc.auth || settings.ProviderStatus(&ch) != tc.status {
			t.Fatalf("%q: auth %v, status %q", tc.kind, ch.Auth, settings.ProviderStatus(&ch))
		}
	}
}
