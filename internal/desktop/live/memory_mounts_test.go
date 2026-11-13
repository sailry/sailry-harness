package live

import (
	"runtime"
	"strings"
	"testing"

	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestPluginMemoryMounts keeps card package identity independent of the
// reference that background authorization updates replace under card.mu.
// The race detector exercises those actual updates during mount collection.
func TestPluginMemoryMounts(t *testing.T) {
	shared, cardOnly := &desktopplugins.Mounted{}, &desktopplugins.Mounted{}
	viewPackage := protocol.PluginReference{Name: "shared-memory", Digest: strings.Repeat("a", 64)}
	cardPackage := protocol.PluginReference{Name: "card-memory", Digest: strings.Repeat("b", 64)}
	sharedCard := &pluginCardInstance{summary: protocol.PluginCardSummary{Package: viewPackage}, reference: viewPackage, mounted: shared}
	uniqueCard := &pluginCardInstance{summary: protocol.PluginCardSummary{Package: cardPackage}, reference: cardPackage, mounted: cardOnly}
	r := &Runtime{
		pluginViews: &pluginViewState{mounts: map[pluginMountKey]*pluginMountedView{
			{name: "shared", role: "navigation"}:   {scope: protocol.PluginContext{Package: viewPackage}, mounted: shared},
			{name: "inactive", role: "navigation"}: {scope: protocol.PluginContext{Package: viewPackage}},
		}},
		pluginCards: &pluginCardState{cards: map[pluginCardKey]*pluginCardInstance{
			{receipt: protocol.PluginCardReceipt{Block: 1}}: sharedCard,
			{receipt: protocol.PluginCardReceipt{Block: 2}}: uniqueCard,
		}},
	}
	start, done := make(chan struct{}), make(chan struct{})
	const iterations = 2000
	go func() {
		defer close(done)
		<-start
		sharedUpdate, uniqueUpdate := viewPackage, cardPackage
		for revision := uint64(1); revision <= iterations; revision++ {
			sharedUpdate.SettingsRevision, uniqueUpdate.SettingsRevision = revision, revision
			sharedCard.updateReference(sharedUpdate)
			uniqueCard.updateReference(uniqueUpdate)
			runtime.Gosched()
		}
	}()
	defer func() { <-done }()
	close(start)
	for range iterations {
		mounts := r.memoryMounts()
		if len(mounts) != 2 {
			t.Fatalf("shared view/card mount was duplicated or an inactive view counted: %+v", mounts)
		}
		seen := map[*desktopplugins.Mounted]bool{}
		for _, mount := range mounts {
			if seen[mount.mounted] {
				t.Fatal("a VM was counted twice")
			}
			seen[mount.mounted] = true
			switch mount.mounted {
			case shared:
				if mount.name != viewPackage.Name {
					t.Fatalf("shared mount lost its captured name: %q", mount.name)
				}
			case cardOnly:
				if mount.name != cardPackage.Name {
					t.Fatalf("card mount lost its captured name: %q", mount.name)
				}
			default:
				t.Fatalf("unexpected live VM: %+v", mount)
			}
		}
		runtime.Gosched()
	}
	<-done
}
