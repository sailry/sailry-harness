package live

import (
	"cmp"
	"fmt"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › Knowledge: the selected host's knowledge collections with
// their indexing state, read when the page shows and again whenever the
// Node reports knowledge_collections_changed (its progress, a failure, a
// removal), and paused, resumed or updated at the revision read.

// knowledgeState is a Node's collections as the page shows them. acting
// are the collections with a pause or an update under way.
type knowledgeState struct {
	collections []protocol.KnowledgeCollection
	acting      map[protocol.KnowledgeID]bool
	known       bool
	reading     bool
	stale       bool
	failed      bool
}

// knowledgeFrame keeps the page's collections on this Node: it reads them
// on entering the page and after a reported change. A failed read is a
// toast and is not tried again until the page is entered anew.
func (r *Runtime) knowledgeFrame(m *settings.Model, entered bool) {
	s := &r.knowledge
	if entered {
		s.failed, s.stale = false, true
	}
	if !s.reading && (!s.known || s.stale) && !s.failed {
		r.readCollections()
	}
	m.Collections = knowledgeViews(s.collections, s.acting)
}

func (r *Runtime) readCollections() {
	s := &r.knowledge
	if s.reading || r.ctx != nil && r.ctx.Err() != nil {
		return
	}
	s.reading, s.stale = true, false
	s.reading = r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "list_knowledge_collections"})
		collections, ok := detail[[]protocol.KnowledgeCollection](output.Data)
		if err == nil && (output.Kind != "knowledge_collections" || !ok) {
			err = fmt.Errorf("unexpected knowledge collections response")
		}
		r.enqueue(func(c *ui.Context) {
			s.reading = false
			if err != nil {
				s.failed = true
				settings.Report(c, kit.Failed, settings.CollectionMessages)
				return
			}
			s.collections, s.known = collections, true
		})
	})
}

// pauseCollection sets or clears a collection's pause.
func (r *Runtime) pauseCollection(id string, paused bool) {
	r.changeCollection(id, func(x protocol.KnowledgeCollection) protocol.Command {
		return protocol.Command{Kind: "pause_knowledge_collection",
			Data: protocol.CommandPauseKnowledgeCollectionPayload{ID: x.ID, ExpectedRevision: x.Revision, Paused: paused}}
	})
}

// updateCollection saves a collection's definition as it is, which has the
// Node read its sources again: changed ones are indexed, failed ones are
// tried again and unchanged ones are kept.
func (r *Runtime) updateCollection(id string) {
	r.changeCollection(id, func(x protocol.KnowledgeCollection) protocol.Command {
		return protocol.Command{Kind: "update_knowledge_collection",
			Data: protocol.CommandUpdateKnowledgeCollectionPayload{ID: x.ID, ExpectedRevision: x.Revision, Definition: x.Definition}}
	})
}

// changeCollection sends one change of a collection at the revision read.
// Failure, conflict and an unknown outcome are the runtime_* toasts; the
// collections are read again either way, so that the next change is sent
// at the Node's revision.
func (r *Runtime) changeCollection(id string, command func(protocol.KnowledgeCollection) protocol.Command) {
	s := &r.knowledge
	index := slices.IndexFunc(s.collections, func(x protocol.KnowledgeCollection) bool { return x.ID.String() == id })
	if index < 0 || s.acting[s.collections[index].ID] {
		return
	}
	x := s.collections[index]
	if s.acting == nil {
		s.acting = map[protocol.KnowledgeID]bool{}
	}
	s.acting[x.ID] = true
	r.saveSetting(command(x), settings.CollectionMessages, func(output protocol.Output, ok bool) {
		delete(s.acting, x.ID)
		s.stale = true
		if changed, valid := detail[protocol.KnowledgeCollection](output.Data); ok && valid && output.Kind == "knowledge_collection" {
			s.accept(changed)
		}
	})
}

// accept takes a collection a change answered with, unless a read since
// brought a newer one.
func (s *knowledgeState) accept(changed protocol.KnowledgeCollection) {
	for i := range s.collections {
		if s.collections[i].ID == changed.ID && s.collections[i].Revision <= changed.Revision {
			s.collections[i] = changed
		}
	}
}

// knowledgeViews are the collections as the page's rows, by name.
func knowledgeViews(collections []protocol.KnowledgeCollection, acting map[protocol.KnowledgeID]bool) []viewmodel.KnowledgeCollection {
	views := make([]viewmodel.KnowledgeCollection, 0, len(collections))
	for _, x := range collections {
		view := viewmodel.KnowledgeCollection{ID: x.ID.String(), Name: x.Definition.Name, Busy: acting[x.ID],
			Sources: x.Progress.Sources, Failed: x.Progress.Failed}
		if len(x.Progress.Failures) > 0 {
			first := x.Progress.Failures[0]
			view.Reason = first.Reason
			if first.Path != "" {
				view.Reason = first.Path + ": " + first.Reason
			}
		}
		switch {
		case x.Paused:
			view.State = viewmodel.KnowledgePaused
		case x.Progress.Status == protocol.KnowledgeStatusPaused:
			// The Node pauses a collection it could not index at all.
			view.State = viewmodel.KnowledgeFailed
		case x.Progress.Status == protocol.KnowledgeStatusIndexing:
			view.State, view.Sources = viewmodel.KnowledgeIndexing, x.Progress.Completed
		case x.Progress.Status == protocol.KnowledgeStatusReady:
			view.State = viewmodel.KnowledgeReady
		}
		views = append(views, view)
	}
	slices.SortStableFunc(views, func(a, b viewmodel.KnowledgeCollection) int {
		return cmp.Or(cmp.Compare(a.Name, b.Name), cmp.Compare(a.ID, b.ID))
	})
	return views
}
