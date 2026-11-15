package live

import (
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A presentation belongs to one view model and one observed source copy.
// Completed turns keep their UI enrichment between frames; controls, images,
// publications and outgoing input are still refreshed by the ordinary pipeline.
type conversationPresentation struct {
	generation uint64
	language   string
	view       *viewmodel.Conversation
	connection conversation.Connection
}

func (r *Runtime) beginConversationFrame() {
	r.conversationSeen = map[protocol.SessionID]bool{}
	if r.conversationCache == nil {
		r.conversationCache = map[protocol.SessionID]conversationCopy{}
	}
	r.conversationCopies = r.conversationCache
}

func (r *Runtime) finishConversationFrame() {
	for id := range r.conversationCache {
		if !r.conversationSeen[id] {
			delete(r.conversationCache, id)
		}
	}
	for id := range r.conversationPresentations {
		if !r.conversationSeen[id] {
			delete(r.conversationPresentations, id)
		}
	}
	r.conversationCopies = nil
}

func (r *Runtime) projectConversation(cv *viewmodel.Conversation, id protocol.SessionID, state client.ConversationState, now time.Time, language string) {
	copy := r.conversationCopies[id]
	pending := r.composerOutgoing[cv.ID] != nil
	if saved, ok := r.conversationPresentations[id]; ok && !pending && copy.generation != 0 &&
		saved.generation == copy.generation && saved.language == language && saved.view == cv {
		// The Node's connection override is applied later in conversation.
		// Restore the observed connection when that Node becomes available.
		cv.Session.Connection = saved.connection
		return
	}
	delete(r.conversationPresentations, id)
	cv.Turns, cv.Session, cv.Waiting, cv.Queue = conversationAt(state, now, language)
	// Active clocks and streaming drafts are time-dependent; keep the existing
	// projection/timer path. Injected copies without a generation are uncached.
	if pending || copy.generation == 0 || len(state.Snapshot.Drafts) != 0 || slices.ContainsFunc(state.Snapshot.Page.Runs, func(run protocol.ConversationRun) bool {
		return run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping
	}) {
		return
	}
	if r.conversationPresentations == nil {
		r.conversationPresentations = map[protocol.SessionID]conversationPresentation{}
	}
	r.conversationPresentations[id] = conversationPresentation{generation: copy.generation, language: language, view: cv, connection: cv.Session.Connection}
}
