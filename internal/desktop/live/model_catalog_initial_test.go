package live

import (
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestModelCatalogInitialPageFetch(t *testing.T) {
	for _, test := range []struct {
		name                             string
		metadata, scores, bundled, stale bool
	}{
		{"empty catalog", false, false, false, false},
		{"metadata already available", true, false, false, false},
		{"scores already available", false, true, false, false},
		{"catalog already available", true, true, false, false},
		{"stale scores refresh on visit", true, true, false, true},
		{"bundled metadata", false, false, true, false},
		{"bundled metadata with scores", false, true, true, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			snapshot.ModelCatalog.Bundled = test.bundled
			fresh := uint64(time.Now().UnixMilli())
			old := uint64(time.Now().Add(-48 * time.Hour).UnixMilli())
			if test.metadata {
				snapshot.ModelCatalog.UpdatedAtMs = &fresh
			}
			if test.scores {
				ts := fresh
				if test.stale {
					ts = old
				}
				snapshot.ModelCatalog.Scores.UpdatedAtMs = &ts
				snapshot.ModelCatalog.Scores.CheckedAtMs = &ts
			}
			transport := &conversationTestTransport{snapshot: snapshot}
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind == "refresh_model_catalog" {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture catalog unavailable"}}, true
				}
				return link.Response{}, false
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			r, err := NewWithAssembly(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"), desktop.Harness())
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			tester := ui.NewTester(func(c *ui.Context) {
				r.drain(c)
				r.nodeSettingsFrame()
				ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
			}, 640, 400)
			settle := func() {
				for range 6 {
					r.workers.Wait()
					tester.Frame()
				}
			}
			count := func() (found int) {
				transport.mu.Lock()
				defer transport.mu.Unlock()
				for _, request := range transport.requests {
					if request.Command.Kind == "refresh_model_catalog" {
						found++
					}
				}
				return
			}
			settle()
			if count() != 0 {
				t.Fatal("catalog fetched before opening model settings")
			}
			r.App.Page = shell.SettingsPage
			r.App.Settings.Open(settings.Providers)
			settle()
			expected := 0
			if !test.metadata || !test.scores || test.stale {
				expected = 1
			}
			if catalog := r.App.Settings.Catalog; catalog.Bundled != test.bundled || !test.metadata && catalog.Last != "" {
				t.Fatalf("catalog projection invented network freshness or lost its source: %+v", catalog)
			}
			if count() != expected || r.App.Settings.Busy[settings.BusyCatalog] {
				t.Fatalf("initial catalog fetches=%d, expected=%d, busy=%v", count(), expected, r.App.Settings.Busy[settings.BusyCatalog])
			}
			settle()
			if count() != expected {
				t.Fatal("a failed catalog fetch repeated on subsequent frames")
			}
			r.App.Page = shell.ConversationPage
			settle()
			r.App.Page = shell.SettingsPage
			settle()
			if count() != expected*2 {
				t.Fatalf("returning to an empty catalog did not make one new attempt: %d", count())
			}
			r.App.Settings.ProviderActions.RefreshCatalog()
			settle()
			if count() != expected*2+1 {
				t.Fatal("the explicit Update action stopped working")
			}
		})
	}
}
