package live

import (
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The interface size is a profile preference beside the conversation's text
// size: both survive a restart and reach the profile's other windows through
// the store's revision, and unchanged defaults write nothing.
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
	if first.App.Settings.ViewZoom != 0 {
		t.Fatalf("a new profile starts at level %d", first.App.Settings.ViewZoom)
	}
	first.savePreferences()
	first.workers.Wait()
	if store.Data.ViewZoom != nil || store.Data.MessageTextSize != nil {
		t.Fatalf("the defaults were written as %v, %v", store.Data.ViewZoom, store.Data.MessageTextSize)
	}
	first.App.Settings.ViewZoom, first.App.Settings.TextSize = 2, 16
	first.savePreferences()
	first.workers.Wait()
	if store.Revision() == second.prefsRevision {
		t.Fatal("the zoom did not change the preferences revision")
	}
	// The other window applies the new revision before it saves its own.
	second.applyPreferences()
	second.savePreferences()
	second.workers.Wait()
	if second.App.Settings.ViewZoom != 2 || second.App.Settings.TextSize != 16 {
		t.Fatalf("the other window shows level %d, text %v", second.App.Settings.ViewZoom, second.App.Settings.TextSize)
	}
	stored := preferences.Open(path).Data
	if stored.ViewZoom == nil || *stored.ViewZoom != 125 || stored.MessageTextSize == nil || *stored.MessageTextSize != 16 {
		t.Fatalf("stored zoom %v, text size %v", stored.ViewZoom, stored.MessageTextSize)
	}
	restarted := window(preferences.Open(path))
	if restarted.App.Settings.ViewZoom != 2 || restarted.App.Settings.TextSize != 16 {
		t.Fatalf("after a restart level %d, text %v", restarted.App.Settings.ViewZoom, restarted.App.Settings.TextSize)
	}
}
