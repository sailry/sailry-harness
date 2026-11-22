package live

import (
	"context"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A mount owns one latest publication, not a queue of snapshots. The worker
// never touches mounted rows; taking a prepared transcript transfers them to
// one presentation on the UI thread.
type conversationFeed struct {
	changed  chan struct{}
	taken    chan struct{}
	language atomic.Pointer[string]
	latest   atomic.Pointer[preparedConversation]
	request  atomic.Uint64
	reveal   *conversationReveal // UI thread only
	sources  map[string]attachmentDescriptor
}

type preparedConversation struct {
	state       client.ConversationState
	language    string
	view        viewmodel.Conversation
	clocks      transcriptClocks
	references  map[string][]conversation.Reference
	attachments []attachmentPart
	controls    *conversationControlFacts
	blank       bool
	request     uint64
}

func (r *Runtime) prepareConversation(id protocol.SessionID, state client.ConversationState, language string) *preparedConversation {
	p := &preparedConversation{state: state, language: language, view: viewmodel.Conversation{ID: id.String()}, blank: state.HasBlankHistoryTurn()}
	v := &p.view
	v.Turns, v.Session, v.Waiting, v.Queue, p.clocks = projectTranscript(state, language)
	r.enrichDetails(v, state, language)
	r.enrichQueue(v, state.Snapshot.Page.Queue)
	p.references = r.conversationReferences(v.ID, state.Snapshot.Page)
	p.attachments = attachmentParts(state.Snapshot.Page)
	p.controls = controlFacts(state.Snapshot.Page)
	for i := range v.Turns {
		for j := range v.Turns[i].Reply.Blocks {
			b := &v.Turns[i].Reply.Blocks[j]
			if b.Kind == conversation.ErrorBlock {
				b.Label = i18n.In(language, b.Label)
			}
		}
	}
	return p
}

func (feed *conversationFeed) refresh() {
	select {
	case feed.changed <- struct{}{}:
	default:
	}
}

func (feed *conversationFeed) setLanguage(language string) {
	if current := feed.language.Load(); current == nil || *current != language {
		feed.language.Store(&language)
		feed.refresh()
	}
}

func (r *Runtime) startConversationFeed(id protocol.SessionID, language string) {
	if r.conversationFeeds == nil {
		r.conversationFeeds = map[protocol.SessionID]*conversationFeed{}
	}
	if r.conversationCache == nil {
		r.conversationCache = map[protocol.SessionID]conversationCopy{}
	}
	root := r.localRuntime()
	if root.conversationLane == nil {
		root.conversationLane = make(chan struct{}, 1)
	}
	lane := root.conversationLane
	watch := r.controller.WatchRecentConversation
	if r.App.Messages.HasTranscriptAnchor(id.String()) {
		watch = r.controller.WatchConversation
	}
	ctx, cancel := context.WithCancel(r.ctx)
	feed := &conversationFeed{changed: make(chan struct{}, 1), taken: make(chan struct{}, 1)}
	if language != "" {
		feed.setLanguage(language)
	}
	feed.refresh()
	r.conversationFeeds[id] = feed
	r.conversationCache[id] = conversationCopy{ok: true, state: client.ConversationState{Loading: true}}
	r.conversationWatches[id] = func() {
		cancel()
		if feed.reveal != nil {
			feed.reveal.finish(nil, client.ConversationState{})
		}
		r.App.Messages.SetReferences(id.String(), nil)
		r.replaceTranscriptAttachmentSources(id)
		r.pruneConversationCards(id, nil)
		delete(r.conversationFeeds, id)
		delete(r.conversationCache, id)
	}
	r.observe(func() {
		// Watch and release can contend with the Client reducer during paging;
		// neither belongs on the frame that selects or leaves a conversation.
		if ctx.Err() != nil {
			return
		}
		release, err := watch(ctx, id, feed.refresh)
		if err != nil {
			if ctx.Err() == nil {
				feed.latest.Store(r.prepareConversation(id, client.ConversationState{Err: err}, language))
				root.wake()
			}
			return
		}
		defer release()
		var previous client.ConversationState
		var previousLocale *string
		var previousRequest uint64
		for ctx.Err() == nil {
			select {
			case <-ctx.Done():
				return
			case <-feed.changed:
			}
			select {
			case <-ctx.Done():
				return
			case lane <- struct{}{}:
			}
			if ctx.Err() != nil {
				<-lane
				return
			}
			request, locale := feed.request.Load(), feed.language.Load()
			state, ok := r.controller.ConversationView(id, previous)
			if !ok || ctx.Err() != nil {
				<-lane
				return
			}
			same := sameTranscriptSource(state, previous) && locale == previousLocale && request == previousRequest
			p := &preparedConversation{state: state}
			if !same && locale != nil {
				p = r.prepareConversation(id, state, *locale)
			}
			p.request = request
			<-lane
			if ctx.Err() != nil {
				return
			}
			previous, previousLocale, previousRequest = state, locale, request
			feed.latest.Store(p)
			root.wake()
			// While the UI has not taken this result, a burst of Client frames
			// costs one dirty bit, not another full projection per notification.
			select {
			case <-ctx.Done():
				return
			case <-feed.taken:
			}
		}
	})
}

func sameTranscriptSource(a, b client.ConversationState) bool {
	return a.SameViewTranscript(b) && a.Connected == b.Connected && a.Loading == b.Loading && a.Removed == b.Removed && a.Err == nil && b.Err == nil
}

func (r *Runtime) takeConversationFeed(id protocol.SessionID, feed *conversationFeed) {
	p := feed.latest.Swap(nil)
	if p == nil {
		return
	}
	select {
	case feed.taken <- struct{}{}:
	default:
	}
	copied := r.conversationCache[id]
	state, previous := p.state, copied.state
	if !state.SameViewSnapshot(previous) {
		copied.composer = nil
	}
	if !sameTranscriptSource(state, previous) {
		r.conversationVersion++
		copied.generation = r.conversationVersion
		copied.prepared, copied.controls = nil, nil
	}
	copied.state, copied.ok = state, true
	if p.view.ID != "" {
		copied.prepared, copied.controls = p, p.controls
	}
	copied.request = p.request
	r.conversationCache[id] = copied
}
