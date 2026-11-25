package live

import (
	"bytes"
	"encoding/json"
	"maps"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSettingsDiscoveryCapturesDraftHeaders(t *testing.T) {
	for name, headers := range map[string]map[string]string{
		"absent": nil,
		"empty":  {},
		"custom": {"X-Tenant-Id": "private-discovery-fixture", "User-Agent": "fixture-client"},
	} {
		t.Run(name, func(t *testing.T) {
			expected := maps.Clone(headers)
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			source := &conversationTestTransport{snapshot: snapshot}
			discoveries, catalogs := 0, 0
			source.handle = func(request protocol.Request) (link.Response, bool) {
				switch request.Command.Kind {
				case "discover_models":
					discoveries++
					selection, ok := detail[protocol.ConversationDiscoverySource](request.Command.Data)
					if !ok || selection.Kind != "draft" {
						t.Error("settings did not issue a draft discovery")
					}
					value, ok := detail[protocol.ConversationDiscoveryDraft](selection.Data)
					if !ok || !maps.Equal(value.Headers, expected) {
						t.Error("settings discovery lost its captured custom headers")
					}
					return link.Response{Output: protocol.Output{Kind: "discovered_models", Data: protocol.ConversationDiscoveryCatalog{
						Endpoint: "https://gateway.example/v1", Models: []protocol.ConversationDiscoveryModel{{ID: "fixture-model"}},
					}}}, true
				case "read_model_catalog":
					catalogs++
					encoded, err := json.Marshal(request.Command)
					if err != nil || bytes.Contains(encoded, []byte(`"headers"`)) || bytes.Contains(encoded, []byte("private-discovery-fixture")) {
						t.Error("a catalog lookup received private discovery headers")
					}
					return link.Response{Output: protocol.Output{Kind: "model_catalog", Data: protocol.ConversationCatalogPage{
						Revision: 1, Models: []protocol.ConversationCatalogModel{},
					}}}, true
				default:
					t.Error("settings discovery issued an unexpected command")
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest}}, true
				}
			}
			controller := client.New(source)
			if controller.Start(t.Context()) != nil {
				t.Fatal("starting the discovery fixture failed")
			}
			r := &Runtime{ctx: t.Context(), controller: controller, providerDraftBases: map[string]protocol.ConversationProvider{},
				providerModels: map[string]providerDiscovery{}, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
			t.Cleanup(func() { r.workers.Wait(); _ = controller.Close() })
			draft := settings.ProviderDraft{Channel: viewmodel.Channel{ID: protocol.ProviderID{1}.String(), Name: "Fixture",
				API: string(protocol.ConversationModelApiResponses), Endpoint: "https://gateway.example/v1", Headers: headers}, EditorGeneration: 1}
			completed := false
			r.discoverModels(r, draft, func(models []viewmodel.ChannelModel) {
				completed = true
				if len(models) != 1 {
					t.Error("settings discovery did not complete its model projection")
				}
			})
			if len(headers) != 0 {
				draft.Channel.Headers["X-Tenant-Id"] = "later-editor-change"
			}
			for range 2 {
				r.workers.Wait()
				r.mu.Lock()
				queue := r.queue
				r.queue = nil
				r.mu.Unlock()
				for _, post := range queue {
					post(nil)
				}
			}
			if !completed || discoveries != 1 || catalogs != 1 || len(r.busy) != 0 {
				t.Fatal("settings discovery did not settle after its isolated catalog lookup")
			}
		})
	}
}
