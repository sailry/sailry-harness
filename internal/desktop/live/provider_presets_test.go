package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderPresetsRemainRetryableAfterFailedOrInvalidReads(t *testing.T) {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	transport := &conversationTestTransport{snapshot: snapshot}
	reads := 0
	listing := protocol.DefaultConversationPresetsListing()
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "list_channel_presets" {
			return link.Response{}, false
		}
		reads++
		switch reads {
		case 1:
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture offline"}}, true
		case 2:
			return link.Response{Output: protocol.Output{Kind: "wrong_type", Data: "invalid"}}, true
		default:
			return link.Response{Output: protocol.Output{Kind: "channel_presets", Data: listing}}, true
		}
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	app := shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	r := &Runtime{ctx: t.Context(), controller: controller, App: app, Preferences: &preferences.Store{Data: preferences.Default()}}
	settle := func() {
		t.Helper()
		for range 4 {
			r.workers.Wait()
			r.mu.Lock()
			queue := r.queue
			r.queue = nil
			r.mu.Unlock()
			for _, apply := range queue {
				apply(nil)
			}
		}
	}
	for expected := 1; expected <= 3; expected++ {
		r.readProviderPresets()
		settle()
		if reads != expected {
			t.Fatalf("preset read %d was not attempted, requests=%d", expected, reads)
		}
		if r.providerPresetsLoading {
			t.Fatalf("preset read %d remained marked in flight", expected)
		}
		if r.providerPresetsLoaded != (expected == 3) {
			t.Fatalf("preset catalog loaded after attempt %d = %t", expected, r.providerPresetsLoaded)
		}
	}
}
