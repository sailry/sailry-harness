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

func TestProviderPresetCache(t *testing.T) {
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
	// Updating the installed packages while an old directory request is in
	// flight must schedule another read, even after the cache was populated.
	install := func(packages []protocol.PluginSummary) {
		t.Helper()
		transport.snapshot.Cursor++
		transport.snapshot.Plugins = packages
		if err := controller.Refresh(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
	installed := protocol.DefaultPluginSummary()
	installed.Name, installed.Enabled, installed.Digest = "agy", true, "version-a"
	install([]protocol.PluginSummary{installed})
	r.readProviderPresets()
	r.workers.Wait()
	installed.Digest = "version-b"
	install([]protocol.PluginSummary{installed})
	settle()
	if reads != 5 || !r.providerPresetsLoaded || r.providerPresetPackages[0].Digest != "version-b" {
		t.Fatalf("stale preset directory after update: reads=%d packages=%v", reads, r.providerPresetPackages)
	}
	r.readProviderPresets()
	settle()
	if reads != 5 {
		t.Fatalf("unchanged package directory triggered another read: %d", reads)
	}
	install(nil)
	r.readProviderPresets()
	settle()
	if reads != 6 || len(r.providerPresetPackages) != 0 {
		t.Fatalf("stale preset directory after removal: reads=%d packages=%v", reads, r.providerPresetPackages)
	}

}
