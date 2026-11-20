package live

import (
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The view zoom is a profile preference: it survives a restart, reaches the
// profile's other windows through the store's revision, and an unchanged
// default writes nothing.
func TestViewZoomPreference(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "preferences.json")
	snapshot := protocol.Snapshot{Node: protocol.NodeID{1}}
	window := func(store *preferences.Store) *Runtime {
		r := &Runtime{ctx: t.Context(), App: shell.NewLive(Sidebar(snapshot, store.Data)), Preferences: store}
		r.applyPreferences()
		r.prefsRevision = store.Revision()
		return r
	}
	store := preferences.Open(path)
	first, second := window(store), window(store)
	if first.App.ViewZoom != 100 {
		t.Fatalf("a new profile starts at %d%%", first.App.ViewZoom)
	}
	first.savePreferences()
	first.workers.Wait()
	if store.Data.ViewZoom != nil {
		t.Fatalf("the default zoom was written as %d", *store.Data.ViewZoom)
	}
	first.App.ViewZoom = 125
	first.savePreferences()
	first.workers.Wait()
	if store.Revision() == second.prefsRevision {
		t.Fatal("the zoom did not change the preferences revision")
	}
	// The other window applies the new revision before it saves its own.
	second.applyPreferences()
	second.savePreferences()
	second.workers.Wait()
	if second.App.ViewZoom != 125 {
		t.Fatalf("the other window shows %d%%", second.App.ViewZoom)
	}
	stored := preferences.Open(path).Data
	if stored.ViewZoom == nil || *stored.ViewZoom != 125 || stored.MessageTextSize != nil {
		t.Fatalf("stored zoom %v, text size %v", stored.ViewZoom, stored.MessageTextSize)
	}
	if restarted := window(preferences.Open(path)); restarted.App.ViewZoom != 125 {
		t.Fatalf("after a restart the zoom is %d%%", restarted.App.ViewZoom)
	}
}
