package live

import (
	"bytes"
	"context"
	"encoding/json"
	"reflect"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderKeyRead(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		provider := providerRouteFixture()
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Providers = protocol.NodeID{1}, []protocol.ConversationProvider{provider}
		key := protocol.NewSecret("stored-fixture-key")
		transport := &conversationTestTransport{snapshot: snapshot}
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "read_provider_key" {
				return link.Response{}, false
			}
			input, _ := detail[protocol.CommandReadProviderKeyPayload](request.Command.Data)
			if input.Provider != provider.ID || input.ExpectedRevision != provider.Revision {
				t.Error("key read lost its captured provider or revision")
			}
			return link.Response{Output: protocol.Output{Kind: "provider_key", Data: &key}}, true
		}
		ctx, cancel := context.WithCancel(t.Context())
		controller := client.New(transport)
		if err := controller.Start(ctx); err != nil {
			cancel()
			t.Fatal(err)
		}
		r := &Runtime{ctx: ctx, controller: controller, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}, providerDraftBases: map[string]protocol.ConversationProvider{}}
		t.Cleanup(func() { cancel(); r.workers.Wait(); _ = controller.Close() })
		synctest.Wait()
		got := ""
		r.readProviderKey(provider.ID.String(), func(value string) { got = value })
		synctest.Wait()
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queue {
			apply(nil)
		}
		if got != key.Expose() {
			t.Fatal("saved API key was reduced to a presence flag")
		}
		channel := viewmodel.Channel{ID: provider.ID.String(), Name: provider.Name, Endpoint: provider.Endpoint, API: "responses", Default: 1}
		for _, model := range provider.Models {
			channel.Models = append(channel.Models, channelModel(model))
		}
		draft := r.providerDraft(settings.ProviderDraft{Channel: channel, ExpectedRevision: provider.Revision})
		originalModels, _ := json.Marshal(provider.Models)
		draftModels, _ := json.Marshal(draft.Models)
		if draft.ID != provider.ID || !reflect.DeepEqual(draft.Credential, provider.Credential) || draft.Api != protocol.ConversationModelApiResponses ||
			draft.DefaultModel != provider.DefaultModel || !bytes.Equal(draftModels, originalModels) {
			t.Fatal("protocol draft changed the provider's identity, stored credential or model metadata")
		}
	})
}
