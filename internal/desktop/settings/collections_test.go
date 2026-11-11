package settings

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The Knowledge page's collections say their state and send their pause,
// resume and update through ModelActions.
func TestKnowledgeCollections(t *testing.T) {
	en := func(key string, args ...any) string { return i18n.In("en", key, args...) }
	m := &Model{Live: true}
	type call struct {
		id     string
		paused bool
		update bool
	}
	var calls []call
	m.ModelActions = &ModelActions{
		CollectionPause:  func(id string, paused bool) { calls = append(calls, call{id: id, paused: paused}) },
		CollectionUpdate: func(id string) { calls = append(calls, call{id: id, update: true}) },
	}
	tt := ui.NewTester(func(c *ui.Context) { collections(c, m) }, 900, 700)
	tt.SetLocale("en")

	t.Run("no collections show no group", func(t *testing.T) {
		tt.Frame()
		if tt.HasText(en("knowledge_collections")) {
			t.Fatalf("an empty host shows the group: %q", tt.Texts())
		}
	})

	t.Run("rows say what each collection is doing", func(t *testing.T) {
		m.Collections = []viewmodel.KnowledgeCollection{
			{ID: "a", Name: "Waiting", State: viewmodel.KnowledgePending},
			{ID: "b", Name: "Reading", State: viewmodel.KnowledgeIndexing, Sources: 12},
			{ID: "c", Name: "Done", State: viewmodel.KnowledgeReady, Sources: 40},
			{ID: "d", Name: "Partly", State: viewmodel.KnowledgeReady, Sources: 40, Failed: 2, Reason: "scan.png: OCR failed"},
			{ID: "e", Name: "Broken", State: viewmodel.KnowledgeFailed, Reason: "worktree is unavailable"},
		}
		tt.Frame()
		for _, want := range []string{
			en("knowledge_collections"), "Waiting", en("knowledge_collection_pending"),
			en("knowledge_collection_indexing", "count", 12),
			en("knowledge_collection_ready", "count", 40),
			en("knowledge_collection_failures", "count", 2, "reason", "scan.png: OCR failed"),
			en("knowledge_collection_failed", "reason", "worktree is unavailable"),
		} {
			if !tt.HasText(want) {
				t.Fatalf("no %q in %q", want, tt.Texts())
			}
		}
	})

	t.Run("pause, resume and update name their collection", func(t *testing.T) {
		m.Collections = []viewmodel.KnowledgeCollection{{ID: "c", Name: "Done", State: viewmodel.KnowledgeReady, Sources: 40}}
		tt.Frame()
		clickButton(t, tt, en("knowledge_pause"))
		clickButton(t, tt, en("knowledge_update"))
		m.Collections[0].State = viewmodel.KnowledgePaused
		tt.Frame()
		if !tt.HasText(en("knowledge_collection_paused")) || !findButton(t, tt, en("knowledge_update")).Disabled {
			t.Fatalf("a paused row: %q", tt.Texts())
		}
		clickButton(t, tt, en("knowledge_resume"))
		want := []call{{id: "c", paused: true}, {id: "c", update: true}, {id: "c"}}
		if len(calls) != len(want) {
			t.Fatalf("calls = %+v", calls)
		}
		for i := range want {
			if calls[i] != want[i] {
				t.Fatalf("calls = %+v", calls)
			}
		}
	})

	t.Run("a row with an action under way is locked", func(t *testing.T) {
		calls = nil
		m.Collections = []viewmodel.KnowledgeCollection{{ID: "c", Name: "Done", State: viewmodel.KnowledgeReady, Busy: true}}
		tt.Frame()
		if !findButton(t, tt, en("knowledge_pause")).Disabled || !findButton(t, tt, en("knowledge_update")).Disabled {
			t.Fatal("a busy row offers its actions")
		}
		clickButton(t, tt, en("knowledge_pause"))
		if len(calls) != 0 {
			t.Fatalf("a busy row sent %+v", calls)
		}
	})
}
