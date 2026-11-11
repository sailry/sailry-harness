package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// collections are the selected host's knowledge collections on the
// Knowledge page: each one's state, and pausing, resuming and updating it.
// A host without collections shows no group.
func collections(c *ui.Context, m *Model) {
	if len(m.Collections) == 0 {
		return
	}
	kit.Group(c, "collections", i18n.T(c, "knowledge_collections"), "", nil, func() {
		kit.Card(c, func() {
			for _, x := range m.Collections {
				kit.Entry(c, x.ID, func() { kit.Record(c, x.Name, collectionState(c, x)) }, nil, func() {
					paused := x.State == viewmodel.KnowledgePaused
					icon, label := icons.Pause, "knowledge_pause"
					if paused {
						icon, label = icons.Play, "knowledge_resume"
					}
					kit.Action(c.Key("pause"), icon, i18n.T(c, label), x.Busy).OnClick(func() {
						m.modelAction(c, func(a *ModelActions) { a.CollectionPause(x.ID, !paused) })
					})
					kit.Action(c.Key("update"), icons.Reload, i18n.T(c, "knowledge_update"), x.Busy || paused).OnClick(func() {
						m.modelAction(c, func(a *ModelActions) { a.CollectionUpdate(x.ID) })
					})
				})
			}
		})
	})
}

// collectionState is a collection's state as its row says it: what it is
// doing with the sources indexed, or what could not be indexed and why.
func collectionState(c *ui.Context, x viewmodel.KnowledgeCollection) string {
	switch x.State {
	case viewmodel.KnowledgePaused:
		return i18n.T(c, "knowledge_collection_paused")
	case viewmodel.KnowledgeFailed:
		return i18n.T(c, "knowledge_collection_failed", "reason", x.Reason)
	case viewmodel.KnowledgeIndexing:
		return i18n.T(c, "knowledge_collection_indexing", "count", x.Sources)
	case viewmodel.KnowledgeReady:
		if x.Failed > 0 {
			return i18n.T(c, "knowledge_collection_failures", "count", x.Failed, "reason", x.Reason)
		}
		return i18n.T(c, "knowledge_collection_ready", "count", x.Sources)
	}
	return i18n.T(c, "knowledge_collection_pending")
}
