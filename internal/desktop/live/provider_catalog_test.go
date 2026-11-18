package live

import (
	"encoding/json"
	"reflect"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderModelLookupUsesChannelCapabilities(t *testing.T) {
	t.Parallel()
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	transport := &conversationTestTransport{snapshot: snapshot}
	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	provider.Endpoint = "https://fixture-gateway.invalid/v1"
	const id = "fixture-vision"
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "read_model_catalog" {
			return link.Response{}, false
		}
		query, ok := detail[protocol.ConversationCatalogQuery](request.Command.Data)
		if !ok || query.Channel == nil || query.Channel.Endpoint != provider.Endpoint || query.Channel.Authentication != provider.Authentication ||
			query.Provider != nil || !slices.Equal(query.IDs, []string{id}) {
			t.Errorf("looked up a model outside the channel: %+v", query)
		}
		page := protocol.DefaultConversationCatalogPage()
		page.Models = []protocol.ConversationCatalogModel{{
			ID: id, Name: id, Inputs: []string{"text", "image"}, Outputs: []string{"text"}, Tools: new(true), Reasoning: new(true),
		}}
		return link.Response{Output: protocol.Output{Kind: "model_catalog", Data: page}}, true
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r := &Runtime{ctx: t.Context(), controller: controller}
	var found *viewmodel.ChannelModel
	r.lookupProviderModel(provider, id, func(model *viewmodel.ChannelModel) { found = model })
	r.workers.Wait()
	for _, post := range r.queue {
		post(nil)
	}
	if found == nil || found.ID != id || !found.Vision || !found.Tools || !found.Reasoning || len(found.Generates) != 0 || found.CustomCapabilities {
		t.Fatalf("catalog tags = %+v", found)
	}
}

func TestProviderDiscoveryCatalogFailureRetainsModels(t *testing.T) {
	for _, test := range []struct {
		name string
		code protocol.ErrorCode
		key  string
	}{
		{"metadata unavailable", protocol.ErrorCodeNotConfigured, "provider_catalog_required"},
		{"catalog read failed", protocol.ErrorCodeUnavailable, "provider_catalog_read_failed"},
	} {
		t.Run(test.name, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Api = protocol.ProviderID{1}, protocol.ConversationModelApiResponses
			provider.Endpoint, provider.Authentication = "https://gateway.example/v1", protocol.AuthenticationApiKey
			transport := &conversationTestTransport{snapshot: snapshot}
			reads := 0
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind == "read_model_catalog" {
					reads++
					query, ok := detail[protocol.ConversationCatalogQuery](request.Command.Data)
					if !ok || query.Provider != nil || query.Channel == nil || query.Channel.Endpoint != provider.Endpoint || query.Channel.Authentication != provider.Authentication {
						t.Errorf("discovery queried a protocol namespace instead of its channel: %+v", query)
					}
					return link.Response{Fault: &protocol.Fault{Code: test.code, Message: "fixture catalog unavailable"}}, true
				}
				return link.Response{}, false
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			previous := map[string]protocol.ConversationModel{"existing": {ID: "existing", Context: 200000, Output: 32000}}
			r := &Runtime{ctx: t.Context(), controller: controller, providerModels: map[string]providerDiscovery{provider.ID.String(): {models: previous}}}
			var shown string
			tester := ui.NewTester(func(c *ui.Context) {
				r.mu.Lock()
				queue := r.queue
				r.queue = nil
				r.mu.Unlock()
				for _, post := range queue {
					post(c)
				}
				ui.ToastViewportBase(c, func(_ ui.Element, toasts []ui.Toast) {
					if len(toasts) > 0 {
						shown = toasts[len(toasts)-1].Title
					}
				})
			}, 800, 500)
			tester.SetLocale("en")
			pending, completed := true, false
			known := map[string]protocol.ConversationModel{"new": {ID: "new", Context: 128000, Output: 16384}}
			r.finishDiscovery(provider, 1, []protocol.ConversationDiscoveryModel{{ID: "new"}}, known, r.providerModels, func(models []viewmodel.ChannelModel) {
				pending, completed = false, true
				if models != nil {
					t.Error("failed metadata lookup exposed incomplete discovered models")
				}
			})
			r.workers.Wait()
			tester.Frame()
			if pending || !completed || reads != 1 || shown != i18n.In("en", test.key) {
				t.Fatalf("catalog failure did not settle discovery with the expected prompt: pending=%v completed=%v reads=%d toast=%q", pending, completed, reads, shown)
			}
			if !reflect.DeepEqual(r.providerModels[provider.ID.String()].models, previous) {
				t.Fatal("failed discovery replaced the provider's existing model metadata")
			}
		})
	}
}

func TestOAuthDiscoveryAcceptsNativeModelsWithoutPublicMetadata(t *testing.T) {
	t.Parallel()
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Api = protocol.ProviderID{1}, protocol.ConversationModelApiResponses
	provider.Endpoint, provider.Authentication = "https://chatgpt.com/backend-api/codex", protocol.AuthenticationChatGptOAuth
	transport := &conversationTestTransport{snapshot: snapshot, handle: func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind == "read_model_catalog" {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture catalog unavailable"}}, true
		}
		return link.Response{}, false
	}}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r := &Runtime{ctx: t.Context(), controller: controller, providerModels: map[string]providerDiscovery{}}
	const modelID = "native-only-model"
	known := map[string]protocol.ConversationModel{modelID: {ID: modelID, Context: 500000, Output: 16384, Vision: true}}
	var received []viewmodel.ChannelModel
	r.finishDiscovery(provider, 1, []protocol.ConversationDiscoveryModel{{ID: modelID}}, known, r.providerModels, func(models []viewmodel.ChannelModel) {
		received = models
	})
	r.workers.Wait()
	for _, post := range r.queue {
		post(nil)
	}
	if len(received) != 1 || received[0].ID != modelID || !received[0].Vision || received[0].Context != 500000 {
		t.Fatalf("account catalog model missing: %+v", received)
	}
}

func TestProviderDraftRefreshesAccountSpeed(t *testing.T) {
	provider := providerRouteFixture()
	provider.Revision = 2
	provider.Authentication, provider.Api = protocol.AuthenticationChatGptOAuth, protocol.ConversationModelApiResponses
	provider.Endpoint = "https://chatgpt.com/backend-api/codex"
	model := &provider.Models[1]
	model.Capabilities.Features = []protocol.ConversationFeaturesFeature{{ID: "speed", Options: []protocol.ConversationFeaturesFeatureOption{{ID: "fast"}}}}
	replacement := []protocol.ConversationFeaturesFeature{{ID: "speed", Options: []protocol.ConversationFeaturesFeatureOption{{ID: "ultrafast"}}}}
	newOptions := map[string]json.RawMessage{"adapter": json.RawMessage(`"refreshed"`)}
	fresh := protocol.ConversationModel{ID: model.ID, Capabilities: &protocol.ConversationDiscoveryCapabilities{Features: replacement, Options: newOptions}}
	key := provider.ID.String()
	channel := viewmodel.Channel{ID: key, API: "responses", Name: provider.Name, Endpoint: provider.Endpoint, Default: 1}
	for _, saved := range provider.Models {
		channel.Models = append(channel.Models, channelModel(saved))
	}
	for _, tt := range []struct {
		name                 string
		generation, revision uint64
		endpoint             string
		authentication       protocol.Authentication
		wantFresh            bool
	}{
		{name: "current editor", generation: 2, revision: 2, wantFresh: true},
		{name: "closed editor", generation: 1, revision: 2},
		{name: "old provider revision", generation: 2, revision: 1},
		{name: "different endpoint", generation: 2, revision: 2, endpoint: "https://gateway.example/v1"},
		{name: "different authentication", generation: 2, revision: 2, authentication: protocol.AuthenticationApiKey},
	} {
		t.Run(tt.name, func(t *testing.T) {
			endpoint, authentication := tt.endpoint, tt.authentication
			if endpoint == "" {
				endpoint = provider.Endpoint
			}
			if authentication == "" {
				authentication = provider.Authentication
			}
			r := &Runtime{
				providerDraftBases: map[string]protocol.ConversationProvider{key + "/2": provider},
				providerModels: map[string]providerDiscovery{
					key: {generation: tt.generation, revision: tt.revision, endpoint: endpoint, authentication: authentication,
						api: provider.Api, models: map[string]protocol.ConversationModel{model.ID: fresh}},
				},
			}
			draft := r.providerDraft(settings.ProviderDraft{Channel: channel, ExpectedRevision: 2, EditorGeneration: 2})
			got := draft.Models[1]
			want := *model
			if tt.wantFresh {
				capabilities := *want.Capabilities
				capabilities.Features, capabilities.Options = replacement, newOptions
				want.Capabilities = &capabilities
			}
			if !reflect.DeepEqual(got, want) || draft.DefaultModel != provider.DefaultModel {
				t.Fatalf("model or default changed unexpectedly: %+v", got)
			}
		})
	}
}
