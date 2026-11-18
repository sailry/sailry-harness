package live

import (
	"reflect"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

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
			transport := &conversationTestTransport{snapshot: snapshot}
			reads := 0
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind == "read_model_catalog" {
					reads++
					return link.Response{Fault: &protocol.Fault{Code: test.code, Message: "fixture catalog unavailable"}}, true
				}
				return link.Response{}, false
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Api = protocol.ProviderID{1}, protocol.ConversationModelApiResponses
			previous := map[string]protocol.ConversationModel{"existing": {ID: "existing", Context: 200000, Output: 32000}}
			r := &Runtime{ctx: t.Context(), controller: controller, providerModels: map[string]map[string]protocol.ConversationModel{provider.ID.String(): previous}}
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
			r.finishDiscovery(provider, []protocol.ConversationDiscoveryModel{{ID: "new"}}, known, r.providerModels, func(models []viewmodel.ChannelModel) {
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
			if !reflect.DeepEqual(r.providerModels[provider.ID.String()], previous) {
				t.Fatal("failed discovery replaced the provider's existing model metadata")
			}
		})
	}
}
