package live

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSavedProviderRefreshesOpenDraft(t *testing.T) {
	for _, removed := range []bool{false, true} {
		name := "saved channel appears without a subscription event"
		if removed {
			name = "a later removal stays authoritative"
		}
		t.Run(name, func(t *testing.T) {
			provider := providerRouteFixture()
			provider.Enabled = true
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Providers = protocol.NodeID{1}, []protocol.ConversationProvider{provider}
			transport := &conversationTestTransport{snapshot: snapshot}
			var saved protocol.ConversationProvider
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind != "save_provider" {
					return link.Response{}, false
				}
				input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
				saved = input.Provider
				saved.Revision = 1
				// No event is sent to the existing subscription. A fresh
				// subscription reads the actual post-save state instead.
				transport.snapshot.Cursor++
				if !removed {
					transport.snapshot.Providers = append(slices.Clone(transport.snapshot.Providers), saved)
				}
				return link.Response{Output: protocol.Output{Kind: "provider", Data: saved}}, true
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			id := r.App.FocusedConversation()
			cv := r.App.Data.Find(id)
			if cv == nil {
				t.Fatal("new conversation draft is unavailable")
			}
			comp := r.App.Composer(id)
			var offered *composer.Context
			tester := ui.NewTester(func(c *ui.Context) {
				r.drain(c)
				offered, _ = r.conversation(c, cv, comp)
				r.finishFrame()
				ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
			}, 640, 400)
			settle := func() {
				for range 6 {
					r.workers.Wait()
					tester.Frame()
				}
			}
			settle()
			comp.Draft = "Keep the existing conversation draft"
			selected := comp.Model
			newID := protocol.ProviderID{9}.String()
			result := settings.ProviderSaveResult{}
			r.saveManagedProvider(settings.ProviderDraft{Channel: viewmodel.Channel{
				ID: newID, Name: "New channel", API: "responses", Endpoint: "https://models.example.test/v1", Auth: viewmodel.AuthKey, Enabled: true,
				Models: []viewmodel.ChannelModel{{ID: "new-model", Context: 128000, Output: 16384}},
			}}, func(saved settings.ProviderSaveResult) { result = saved })
			settle()
			if len(result.Succeeded) != 1 {
				t.Fatal("confirmed channel save was not reported")
			}
			found := slices.ContainsFunc(offered.Models, func(model composer.Model) bool { return model.ID == modelKey(saved.ID, "new-model") })
			if found == removed {
				t.Fatalf("open draft model choices do not match the fresh Node snapshot: %+v", offered.Models)
			}
			if comp.Model != selected || comp.Draft != "Keep the existing conversation draft" {
				t.Fatal("refreshing channel choices replaced the draft or its model selection")
			}
			transport.mu.Lock()
			defer transport.mu.Unlock()
			if count := len(slices.DeleteFunc(slices.Clone(transport.requests), func(request protocol.Request) bool { return request.Command.Kind != "save_provider" })); count != 1 {
				t.Fatalf("refresh repeated the channel save: %d requests", count)
			}
		})
	}
}
