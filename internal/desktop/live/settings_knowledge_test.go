package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// checkKnowledgeViews covers the pure half of the Knowledge page's
// collections: the Node's progress as the rows' states, and which answer
// of a change is kept.
func checkKnowledgeViews(t *testing.T) {
	collection := func(id byte, name string, paused bool, progress protocol.KnowledgeProgress) protocol.KnowledgeCollection {
		value := protocol.DefaultKnowledgeCollection()
		value.ID, value.Revision, value.Paused, value.Progress = protocol.KnowledgeID{id}, 3, paused, progress
		value.Definition.Name = name
		return value
	}
	collections := []protocol.KnowledgeCollection{
		collection(1, "e pending", false, protocol.KnowledgeProgress{Status: protocol.KnowledgeStatusPending}),
		collection(2, "d indexing", false, protocol.KnowledgeProgress{Status: protocol.KnowledgeStatusIndexing, Sources: 9, Completed: 7}),
		collection(3, "c ready", false, protocol.KnowledgeProgress{Status: protocol.KnowledgeStatusReady, Sources: 40, Completed: 40, Failed: 2,
			Failures: []protocol.KnowledgeFailure{{Path: "scan.png", Reason: "OCR failed"}, {Path: "b.pdf", Reason: "unreadable"}}}),
		collection(4, "b paused", true, protocol.KnowledgeProgress{Status: protocol.KnowledgeStatusPaused, Sources: 5, Completed: 5}),
		collection(5, "a failed", false, protocol.KnowledgeProgress{Status: protocol.KnowledgeStatusPaused,
			Failures: []protocol.KnowledgeFailure{{Reason: "worktree is unavailable"}}}),
	}
	views := knowledgeViews(collections, map[protocol.KnowledgeID]bool{{3}: true})
	want := []viewmodel.KnowledgeCollection{
		{ID: collections[4].ID.String(), Name: "a failed", State: viewmodel.KnowledgeFailed, Reason: "worktree is unavailable"},
		{ID: collections[3].ID.String(), Name: "b paused", State: viewmodel.KnowledgePaused, Sources: 5},
		{ID: collections[2].ID.String(), Name: "c ready", State: viewmodel.KnowledgeReady, Sources: 40, Failed: 2, Reason: "scan.png: OCR failed", Busy: true},
		{ID: collections[1].ID.String(), Name: "d indexing", State: viewmodel.KnowledgeIndexing, Sources: 7},
		{ID: collections[0].ID.String(), Name: "e pending", State: viewmodel.KnowledgePending},
	}
	if len(views) != len(want) {
		t.Fatalf("views = %+v", views)
	}
	for i := range want {
		if views[i] != want[i] {
			t.Fatalf("view %d = %+v, want %+v", i, views[i], want[i])
		}
	}

	// A change's answer replaces the collection read, unless a read since
	// brought a newer revision.
	state := knowledgeState{collections: []protocol.KnowledgeCollection{collections[3]}}
	resumed := collections[3]
	resumed.Revision, resumed.Paused = 4, false
	state.accept(resumed)
	if state.collections[0].Paused || state.collections[0].Revision != 4 {
		t.Fatalf("accepted = %+v", state.collections[0])
	}
	state.accept(collections[3])
	if state.collections[0].Revision != 4 {
		t.Fatalf("an older answer replaced revision 4: %+v", state.collections[0])
	}
}
