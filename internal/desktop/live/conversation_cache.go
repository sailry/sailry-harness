package live

import (
	"encoding/json"
	"reflect"
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
	statistics *protocol.ConversationStatistics
	mounted    bool
	overlay    *conversationOutgoing
	requested  uint64
	offline    bool
}

// Composer bindings share the usage wire representation and the run flags
// of their observed history. Local draft, model and permission state are read
// independently so plugins still see changes before the Node observes them.
type composerHistory struct {
	statistics  json.RawMessage
	compacting  bool
	hasMessages bool
	err         error
	// source is the read-only statistics the wire representation was made
	// from, compared by value when a later view carries a new copy.
	source *protocol.ConversationStatistics
}

func (r *Runtime) composerHistory(id protocol.SessionID) *composerHistory {
	state, ok := r.conversationState(id)
	if !ok || state.Snapshot.Statistics == nil && len(state.Snapshot.Page.Runs) == 0 {
		return nil
	}
	copies := r.conversationCopies
	if r.conversationFeeds[id] != nil {
		copies = r.conversationCache
	}
	copied, cached := copies[id]
	if copied.composer != nil {
		return copied.composer
	}
	hasMessages := len(state.Snapshot.Page.Runs) != 0
	compacting := slices.ContainsFunc(state.Snapshot.Page.Runs, func(run protocol.ConversationRun) bool {
		return run.Kind == protocol.ConversationRunKindCompaction &&
			(run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping)
	})
	// A streamed view copies unchanged statistics. Keeping the previous
	// projection keeps composer bindings from encoding the usage again.
	history := copied.usage
	if history == nil || history.err != nil || history.hasMessages != hasMessages || history.compacting != compacting ||
		!sameComposerStatistics(history.source, state.Snapshot.Statistics) {
		history = &composerHistory{statistics: json.RawMessage("null"), hasMessages: hasMessages, compacting: compacting,
			source: state.Snapshot.Statistics}
		if state.Snapshot.Statistics != nil {
			history.statistics, history.err = plugins.ComposerStatistics(state.Snapshot.Statistics)
		}
	}
	if cached {
		copied.composer, copied.usage = history, history
		copies[id] = copied
	}
	return history
}

func sameComposerStatistics(previous, current *protocol.ConversationStatistics) bool {
	if previous == nil || current == nil {
		return previous == current
	}
	return previous == current || equalProtocolValue(reflect.ValueOf(previous).Elem(), reflect.ValueOf(current).Elem())
}

// equalProtocolValue compares decoded protocol values by every field.
// reflect.DeepEqual records each pointer and slice it visits against cycles,
// which allocates on every streamed view; decoded values have no cycles.
// Values it cannot compare report a difference.
func equalProtocolValue(a, b reflect.Value) bool {
	switch a.Kind() {
	case reflect.Pointer, reflect.Interface:
		if a.IsNil() || b.IsNil() {
			return a.IsNil() == b.IsNil()
		}
		if a.Kind() == reflect.Interface && a.Elem().Type() != b.Elem().Type() {
			return false
		}
		return equalProtocolValue(a.Elem(), b.Elem())
	case reflect.Slice:
		if a.IsNil() != b.IsNil() || a.Len() != b.Len() {
			return false
		}
		fallthrough
	case reflect.Array:
		for index := range a.Len() {
			if !equalProtocolValue(a.Index(index), b.Index(index)) {
				return false
			}
		}
		return true
	case reflect.Struct:
		for index := range a.NumField() {
			if !equalProtocolValue(a.Field(index), b.Field(index)) {
				return false
			}
		}
		return true
	case reflect.Map:
		if a.IsNil() != b.IsNil() || a.Len() != b.Len() {
			return false
		}
		for entries := a.MapRange(); entries.Next(); {
			other := b.MapIndex(entries.Key())
			if !other.IsValid() || !equalProtocolValue(entries.Value(), other) {
				return false
			}
		}
		return true
	case reflect.Bool:
		return a.Bool() == b.Bool()
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return a.Int() == b.Int()
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64, reflect.Uintptr:
		return a.Uint() == b.Uint()
	case reflect.Float32, reflect.Float64:
		return a.Float() == b.Float()
	case reflect.String:
		return a.String() == b.String()
	default:
		return false
	}
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
	for id, feed := range r.conversationFeeds {
		r.takeConversationFeed(id, feed)
	}
	r.conversationCopies = r.conversationCache
}

func (r *Runtime) finishConversationFrame() {
	for id := range r.conversationCache {
		if !r.conversationSeen[id] {
			delete(r.conversationCache, id)
		}
	}
	for _, outgoing := range []map[string]*conversationOutgoing{r.composerOutgoing, r.composerTrayOutgoing} {
		for id, pending := range outgoing {
			if pending.turn == (protocol.TurnID{}) {
				continue
			}
			if session, err := protocol.ParseSessionID(id); err == nil && !r.conversationSeen[session] {
				r.discardCapturedOutgoing(id, pending)
			}
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
		if previous.Find(old.ID) != old || old.Offline || saved.generation == 0 {
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
	r.projectConversation(cv, id, state, c.Now(), c.Locale())
	if feed := r.conversationFeeds[id]; feed != nil && feed.reveal != nil && r.conversationPresentations[cv].generation == r.conversationCache[id].generation &&
		r.conversationCache[id].request >= feed.reveal.request {
		reveal := feed.reveal
		feed.reveal = nil
		reveal.finish(cv, state)
	}
	cv.Animating = state.Connected && len(state.Snapshot.Drafts) > 0
}

func (r *Runtime) projectConversation(cv *viewmodel.Conversation, id protocol.SessionID, state client.ConversationState, now time.Time, language string) bool {
	copied := r.conversationCache[id]
	if r.conversationCopies != nil {
		copied = r.conversationCopies[id]
	}
	saved := r.conversationPresentations[cv]
	statistics := state.Snapshot.Statistics
	if copied.generation != 0 && saved.generation == copied.generation && saved.language == language &&
		(saved.overlay == nil || saved.overlay == r.composerOutgoing[cv.ID]) && (!saved.offline || !r.controller.Connected()) {
		// The Node's connection override is applied later in conversation.
		// Restore the observed connection when that Node becomes available.
		cv.Session.Connection = saved.connection
		cv.Session.Loading = state.Loading
		cv.Session.LoadingOlder, cv.Session.OlderFailed = state.LoadingOlder, state.OlderErr != nil
		if saved.statistics != statistics {
			cv.Session.Usage, saved.statistics = conversationUsage(statistics), statistics
		}
		if !saved.offline {
			saved.clocks.advance(cv.Turns, now)
		}
		if overlay := r.projectOutgoing(cv, state.Snapshot.Page); overlay != nil || saved.overlay != r.composerOutgoing[cv.ID] {
			saved.overlay = overlay
		}
		saved.mounted = true
		r.conversationPresentations[cv] = saved
		return false
	}
	p := copied.prepared
	if feed := r.conversationFeeds[id]; feed != nil {
		if p == nil || p.language != language {
			// A second mounted view needs its own mutable rows. Never rebuild a
			// consumed presentation or a locale change in the calling frame.
			feed.setLanguage(language)
			if saved.requested <= copied.request {
				saved.requested = feed.request.Add(1)
				feed.refresh()
			}
			if saved.generation == 0 && len(cv.Turns) == 0 {
				cv.Session.Loading = true
				r.projectOutgoing(cv, protocol.ConversationPage{})
			}
			saved.mounted = true
			r.saveConversationPresentation(cv, saved)
			return false
		}
		copied.prepared = nil
		r.conversationCache[id] = copied
	} else {
		p = r.prepareConversation(id, state, language)
	}
	v := &p.view
	cv.Turns, cv.Session, cv.Waiting, cv.Queue = v.Turns, v.Session, v.Waiting, v.Queue
	cv.Session.LoadingOlder, cv.Session.OlderFailed = state.LoadingOlder, state.OlderErr != nil
	if p.state.Snapshot.Statistics != statistics {
		cv.Session.Usage = conversationUsage(statistics)
	}
	p.clocks.advance(cv.Turns, now)
	r.App.Messages.SetReferences(cv.ID, p.references)
	r.replaceTranscriptAttachmentSources(id)
	r.bindAttachmentSources(cv.ID, state.Snapshot.Page, p.attachments)
	r.projectConversationAssets(cv, state, p.attachments)
	r.pruneConversationCards(id, cv.Turns)
	// Recover a blank turn on the fitted page without loading unrelated
	// history or the prefix of a turn already shown.
	if p.blank && !state.LoadingOlder && state.OlderErr == nil {
		r.async(func() { _ = r.controller.LoadBlankConversationTurn(r.ctx, id) })
	}
	presentation := conversationPresentation{mounted: true, generation: copied.generation, language: language, overlay: r.projectOutgoing(cv, p.state.Snapshot.Page),
		connection: cv.Session.Connection, clocks: p.clocks, statistics: statistics}
	r.saveConversationPresentation(cv, presentation)
	return true
}
