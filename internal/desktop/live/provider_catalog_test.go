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
	nodeproviders "github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderModelLookupUsesChannelCapabilities(t *testing.T) {
	t.Parallel()
	// Inputs come only from a catalog entry of the channel's own namespace; an
	// unlisted gateway keeps the unknown list with its Vision tag.
	for _, test := range []struct {
		endpoint string
		inputs   []string
	}{
		{endpoint: "https://fixture-gateway.invalid/v1"},
		{endpoint: "https://api.deepseek.com", inputs: []string{"text", "image"}},
	} {
		t.Run(test.endpoint, func(t *testing.T) {
			t.Parallel()
			found := lookupProviderModelFixture(t, test.endpoint)
			if found == nil || found.ID != "fixture-vision" || !found.Vision || !found.Tools || !found.Reasoning || len(found.Generates) != 0 || found.CustomCapabilities ||
				(found.Inputs == nil) != (test.inputs == nil) || !slices.Equal(found.Inputs, test.inputs) {
				t.Fatalf("catalog tags = %+v", found)
			}
		})
	}
}

func lookupProviderModelFixture(t *testing.T, endpoint string) *viewmodel.ChannelModel {
	t.Helper()
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	transport := &conversationTestTransport{snapshot: snapshot}
	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	provider.Endpoint = endpoint
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
	return found
}

// The editor's inputs reach the draft: an unknown list stays unknown, an
// explicit empty list stays explicit, the user's list is sent with Vision
// as its image entry, and a model that follows the catalog again goes
// without the user's mark, for the Node to complete.
func TestProviderModelInputsDraft(t *testing.T) {
	t.Parallel()
	provider := providerRouteFixture()
	legacy, explicit, custom := protocol.DefaultConversationModel(), protocol.DefaultConversationModel(), protocol.DefaultConversationModel()
	legacy.ID, legacy.Context, legacy.Output, legacy.Vision = "legacy", 128000, 16384, true
	explicit.ID, explicit.Context, explicit.Output, explicit.Inputs = "explicit", 128000, 16384, []string{}
	custom.ID, custom.Context, custom.Output, custom.Inputs, custom.Vision, custom.CustomCapabilities = "custom", 128000, 16384, []string{"text", "image", "pdf"}, true, true
	custom.Generates = []protocol.MediaGeneration{protocol.MediaGenerationSpeech}
	provider.Models, provider.DefaultModel = []protocol.ConversationModel{legacy, explicit, custom}, custom.ID
	projected := []viewmodel.ChannelModel{channelModel(legacy), channelModel(explicit), channelModel(custom)}
	if projected[0].Inputs != nil || !projected[0].AcceptsInput("image") || projected[1].Inputs == nil || len(projected[1].Inputs) != 0 ||
		projected[1].Vision || !slices.Equal(projected[2].Inputs, custom.Inputs) || !projected[2].Vision || !projected[2].CustomCapabilities {
		t.Fatalf("projected inputs = %+v", projected)
	}
	key := provider.ID.String()
	r := &Runtime{providerDraftBases: map[string]protocol.ConversationProvider{key + "/8": provider}}
	channel := viewmodel.Channel{ID: key, API: string(provider.Api), Name: provider.Name, Endpoint: provider.Endpoint, Default: 2, Models: projected}
	draft := r.providerDraft(settings.ProviderDraft{Channel: channel, ExpectedRevision: 8})
	if draft.Models[0].Inputs != nil || !draft.Models[0].Vision || draft.Models[1].Inputs == nil || len(draft.Models[1].Inputs) != 0 ||
		!slices.Equal(draft.Models[2].Inputs, custom.Inputs) || !draft.Models[2].CustomCapabilities || draft.DefaultModel != custom.ID {
		t.Fatalf("unchanged inputs changed in the draft: %+v", draft.Models)
	}
	channel.Models = slices.Clone(projected)
	channel.Models[0].CustomCapabilities, channel.Models[0].Inputs = true, []string{"text", "video"}
	channel.Models[2].CustomCapabilities = false
	draft = r.providerDraft(settings.ProviderDraft{Channel: channel, ExpectedRevision: 8})
	if got := draft.Models[0]; !got.CustomCapabilities || !slices.Equal(got.Inputs, []string{"text", "video"}) || got.Vision {
		t.Fatalf("the user's inputs = %+v", got)
	}
	if got := draft.Models[2]; got.CustomCapabilities || got.ID != custom.ID || draft.DefaultModel != custom.ID {
		t.Fatalf("a model following the catalog kept the user's mark: %+v", got)
	}
}

// A draft on another route keeps none of the previous route's options.
func TestProviderDraftRouteOptions(t *testing.T) {
	provider := providerRouteFixture()
	provider.Api, provider.Endpoint = protocol.ConversationModelApiBedrock, "https://bedrock-runtime.us-east-1.amazonaws.com"
	provider.Options = &protocol.ConversationConnectionOptions{Kind: "bedrock", Data: protocol.ConversationConnectionOptionsBedrockPayload{Region: "us-east-1"}}
	key := provider.ID.String()
	r := &Runtime{providerDraftBases: map[string]protocol.ConversationProvider{key + "/8": provider}}
	channel := viewmodel.Channel{ID: key, API: string(protocol.ConversationModelApiAnthropic), Name: provider.Name, Endpoint: "https://api.example.test", Region: "us-east-1"}
	draft := r.providerDraft(settings.ProviderDraft{Channel: channel, ExpectedRevision: 8})
	if draft.Options != nil {
		t.Errorf("an Anthropic draft kept options %+v", draft.Options)
	}
	if err := nodeproviders.ValidateConfiguration(draft); err != nil {
		t.Errorf("the Node refuses the switched provider: %v", err)
	}
	channel.API, channel.Endpoint, channel.Project, channel.Location = string(protocol.ConversationModelApiVertexAi), "", "proj-123", "global"
	draft = r.providerDraft(settings.ProviderDraft{Channel: channel, ExpectedRevision: 8})
	if options, ok := draft.Options.VertexAi(); !ok || options.Project != "proj-123" || draft.Endpoint != "https://aiplatform.googleapis.com" {
		t.Errorf("a Vertex AI draft has options %+v at %q", draft.Options, draft.Endpoint)
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
