package live

import (
	"encoding/json"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A presentation belongs to one view model and one observed source copy.
// Turns retain their publication metadata between frames. Visible image rows,
// controls and outgoing input still update independently.
type conversationPresentation struct {
	generation uint64
	language   string
	connection conversation.Connection
	clocks     transcriptClocks
	mounted    bool
}

// Composer bindings share the usage wire representation and the run flags
// of their observed history. Local draft, model and permission state are read
// independently so plugins still see changes before the Node observes them.
type composerHistory struct {
	statistics  json.RawMessage
	compacting  bool
	hasMessages bool
	err         error
}

func (r *Runtime) composerHistory(id protocol.SessionID) composerHistory {
	history := composerHistory{statistics: json.RawMessage("null")}
	state, ok := r.conversationState(id)
	if !ok {
		return history
	}
	copied, cached := r.conversationCopies[id]
	if copied.composer != nil {
		return *copied.composer
	}
	if state.Snapshot.Statistics != nil {
		history.statistics, history.err = plugins.ComposerStatistics(state.Snapshot.Statistics)
	}
	history.hasMessages = len(state.Snapshot.Page.Runs) != 0
	history.compacting = slices.ContainsFunc(state.Snapshot.Page.Runs, func(run protocol.ConversationRun) bool {
		return run.Kind == protocol.ConversationRunKindCompaction &&
			(run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping)
	})
	if cached {
		copied.composer = &history
		r.conversationCopies[id] = copied
	}
	return history
}

func (r *Runtime) beginConversationFrame() {
	r.conversationSeen = map[protocol.SessionID]bool{}
	if r.conversationCache == nil {
		r.conversationCache = map[protocol.SessionID]conversationCopy{}
	}
	r.mu.Lock()
	for id := range r.conversationChanges {
		if cached, ok := r.conversationCache[id]; ok {
			cached.stale = true
			r.conversationCache[id] = cached
		}
	}
	clear(r.conversationChanges)
	r.mu.Unlock()
	r.conversationCopies = r.conversationCache
}

func (r *Runtime) finishConversationFrame() {
	for id := range r.conversationCache {
		if !r.conversationSeen[id] {
			delete(r.conversationCache, id)
		}
	}
	for cv, presentation := range r.conversationPresentations {
		if presentation.mounted {
			presentation.mounted = false
			r.conversationPresentations[cv] = presentation
			continue
		}
		cv.Turns, cv.Queue = nil, nil
		cv.Waiting, cv.Animating = composer.Interactions{}, false
		delete(r.conversationPresentations, cv)
	}
	for id, read := range r.overviews {
		if !r.conversationSeen[id] {
			read.cancelRead()
			delete(r.overviews, id)
		}
	}
	for id := range r.childConversations {
		if !r.conversationSeen[id] {
			delete(r.childConversations, id)
		}
	}
	r.conversationCopies = nil
}

func (r *Runtime) saveConversationPresentation(cv *viewmodel.Conversation, presentation conversationPresentation) {
	if r.conversationPresentations == nil {
		r.conversationPresentations = map[*viewmodel.Conversation]conversationPresentation{}
	}
	r.conversationPresentations[cv] = presentation
}

// carryConversationPresentations moves the mounted rows onto the views a
// Node snapshot rebuilt. The localized frame projection consumes new
// observations when the conversation renders.
func (r *Runtime) carryConversationPresentations(previous *viewmodel.Data) {
	if previous == nil {
		return
	}
	for old, saved := range r.conversationPresentations {
		if previous.Find(old.ID) != old || old.Offline || saved.generation == 0 || r.composerOutgoing[old.ID] != nil {
			continue
		}
		cv := r.App.Data.Find(old.ID)
		if cv == nil || cv == old || cv.Node != old.Node || cv.Offline {
			continue
		}
		cv.Turns, cv.Session, cv.Waiting, cv.Queue, cv.Animating = old.Turns, old.Session, old.Waiting, old.Queue, old.Animating
		old.Turns, old.Queue = nil, nil
		old.Waiting, old.Animating = composer.Interactions{}, false
		delete(r.conversationPresentations, old)
		r.conversationPresentations[cv] = saved
	}
}

// An optimistic edit invalidates reuse while retaining ownership of the view
// until the frame can release a detached presentation.
func (r *Runtime) invalidateConversationPresentation(cv *viewmodel.Conversation) {
	r.saveConversationPresentation(cv, conversationPresentation{mounted: r.conversationPresentations[cv].mounted})
}

func (r *Runtime) projectWatchedConversation(c *ui.Context, cv *viewmodel.Conversation, id protocol.SessionID, state client.ConversationState) {
	if r.projectConversation(cv, id, state, c.Now(), c.Locale()) {
		page := state.Snapshot.Page
		r.bindReferences(cv.ID, page)
		r.enrichDetails(cv, state, c.Locale())
		r.enrichQueue(cv, page.Queue)
		r.bindAttachmentSources(cv.ID, page)
		r.projectConversationAssets(cv, state)
	}
	cv.Animating = state.Connected && len(state.Snapshot.Drafts) > 0
}

func (r *Runtime) projectConversation(cv *viewmodel.Conversation, id protocol.SessionID, state client.ConversationState, now time.Time, language string) bool {
	copy := r.conversationCopies[id]
	cacheable := copy.generation != 0 && r.composerOutgoing[cv.ID] == nil
	saved := r.conversationPresentations[cv]
	if cacheable && saved.generation == copy.generation && saved.language == language {
		// The Node's connection override is applied later in conversation.
		// Restore the observed connection when that Node becomes available.
		cv.Session.Connection = saved.connection
		cv.Session.LoadingOlder, cv.Session.OlderFailed = state.LoadingOlder, state.OlderErr != nil
		saved.clocks.advance(cv.Turns, now)
		saved.mounted = true
		r.conversationPresentations[cv] = saved
		return false
	}
	var clocks transcriptClocks
	cv.Turns, cv.Session, cv.Waiting, cv.Queue, clocks = projectTranscript(state, language)
	clocks.advance(cv.Turns, now)
	// Recover a blank turn on the fitted page without loading unrelated
	// history or the prefix of a turn already shown.
	if state.HasBlankHistoryTurn() && !state.LoadingOlder && state.OlderErr == nil {
		r.async(func() { _ = r.controller.LoadBlankConversationTurn(r.ctx, id) })
	}
	presentation := conversationPresentation{mounted: true}
	if cacheable {
		presentation.generation, presentation.language = copy.generation, language
		presentation.connection, presentation.clocks = cv.Session.Connection, clocks
	}
	r.saveConversationPresentation(cv, presentation)
	return true
}
