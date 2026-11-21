package client

import (
	"context"
	"encoding/json"
	"strconv"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type toolReference struct {
	Entry string `json:"entry"`
	Index uint64 `json:"index"`
}

type historyRequest struct {
	through uint64
}

type detailRead struct {
	cancel context.CancelFunc
}

type observationTranscript struct {
	calls  []ToolCall     // Initialized under conversationObserver.mu.
	fields map[string]any // Only the serialized Next path builds these read-only fields.
}

type observationView struct {
	ConversationState
	derived *observationTranscript
}

func (view *observationView) toolCallsLocked() []ToolCall {
	if view.derived.calls == nil {
		view.derived.calls = ToolCalls(view.Snapshot.Page)
	}
	return view.derived.calls
}

type conversationObserver struct {
	client        *Client
	observer      *Observer
	session       protocol.SessionID
	referenceRows bool
	history       chan historyRequest
	mu            sync.Mutex
	selected      map[toolReference]bool
	reads         map[string]*detailRead
	failures      map[string]*protocol.Fault
	sent          map[string]bool
	previous      *observationView // Guarded by Client.conversationMu.
}

func (client *Client) watchConversationView(ctx context.Context, session protocol.SessionID) (*Observer, error) {
	return client.WatchConversationView(ctx, session, ConversationViewOptions{})
}

type ConversationViewOptions struct {
	ReferenceRows bool // Omit recoverable parts; the default keeps plugin rows inline.
	Recent        bool // Skip the released bookmark when opening a new watch.
}

func (client *Client) WatchConversationView(ctx context.Context, session protocol.SessionID, options ConversationViewOptions) (*Observer, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	observer := newObserver(ctx)
	owner := &conversationObserver{
		client: client, observer: observer, session: session, referenceRows: options.ReferenceRows, history: make(chan historyRequest, 8),
		selected: map[toolReference]bool{}, reads: map[string]*detailRead{}, failures: map[string]*protocol.Fault{}, sent: map[string]bool{},
	}
	watch := client.WatchConversation
	if options.Recent {
		watch = client.WatchRecentConversation
	}
	release, err := watch(observer.ctx, session, owner.changed)
	if err != nil {
		_ = observer.Close()
		return nil, err
	}
	dropView := func() {
		client.conversationMu.Lock()
		owner.previous = nil
		client.conversationMu.Unlock()
	}
	stop := context.AfterFunc(observer.ctx, dropView)
	observer.release = func() {
		stop()
		release()
		dropView()
	}
	observer.read = owner.view
	observer.control = owner.control
	observer.run(owner.readHistory)
	observer.notify()
	return observer, nil
}

func (owner *conversationObserver) changed() {
	owner.client.conversationMu.Lock()
	if previous := owner.previous; previous != nil {
		watch := owner.client.conversations[owner.session]
		if watch == nil || watch.projection.entryEpoch != previous.entryEpoch {
			owner.previous = nil
		}
	}
	owner.client.conversationMu.Unlock()
	owner.observer.notify()
}

func (owner *conversationObserver) control(_ context.Context, method string, argument json.RawMessage) error {
	switch method {
	case "load_older", "load_through":
		request := historyRequest{}
		if method == "load_through" {
			var value string
			if json.Unmarshal(argument, &value) != nil {
				return fault(protocol.ErrorCodeInvalidRequest, "invalid conversation turn sequence")
			}
			sequence, err := strconv.ParseUint(value, 10, 64)
			if err != nil || sequence == 0 || strconv.FormatUint(sequence, 10) != value {
				return fault(protocol.ErrorCodeInvalidRequest, "conversation turn sequence must be positive")
			}
			request.through = sequence
		}
		select {
		case owner.history <- request:
			return nil
		default:
			if method == "load_older" {
				return nil
			}
			return fault(protocol.ErrorCodeBusy, "conversation history request queue is full")
		}
	case "track_tools":
		var selected struct {
			Items []toolReference `json:"items"`
			Retry []toolReference `json:"retry"`
		}
		if json.Unmarshal(argument, &selected) != nil {
			return fault(protocol.ErrorCodeInvalidRequest, "invalid tool selection")
		}
		owner.mu.Lock()
		var calls []ToolCall
		if len(selected.Items) > 0 {
			owner.client.conversationMu.Lock()
			if err := owner.observer.ctx.Err(); err != nil {
				owner.client.conversationMu.Unlock()
				owner.mu.Unlock()
				return err
			}
			view := owner.detachedViewLocked(owner.client.conversations[owner.session])
			owner.client.conversationMu.Unlock()
			if view != nil {
				calls = view.toolCallsLocked()
			}
		}
		owner.selected = map[toolReference]bool{}
		for _, reference := range selected.Items {
			owner.selected[reference] = true
		}
		for _, call := range calls {
			for _, retry := range selected.Retry {
				if call.Source == retry && call.Reference != nil {
					delete(owner.failures, detailKey(*call.Reference))
				}
			}
		}
		owner.syncDetailsLocked(calls)
		owner.mu.Unlock()
		owner.observer.notify()
		return nil
	}
	return fault(protocol.ErrorCodeInvalidRequest, "unknown conversation observation control")
}

func (owner *conversationObserver) readHistory() {
	for {
		var request historyRequest
		select {
		case <-owner.observer.ctx.Done():
			return
		case request = <-owner.history:
		}
		for owner.observer.ctx.Err() == nil {
			owner.client.conversationMu.Lock()
			watch := owner.client.conversations[owner.session]
			if watch == nil || watch.ctx.Err() != nil {
				owner.client.conversationMu.Unlock()
				break
			}
			snapshot := watch.projection.snapshot
			loaded := snapshot != nil
			if !loaded && watch.state.Err != nil && !retryConversation(watch.state.Err) {
				owner.client.conversationMu.Unlock()
				break
			}
			atTarget := loaded && request.through != 0 && conversationContainsThrough(*snapshot, request.through)
			hasOlder := loaded && (snapshot.Page.NextBefore != nil || len(snapshot.Missing) != 0)
			waitForPage := request.through != 0 && !atTarget && hasOlder && watch.state.LoadingOlder
			signal, closed := watch.signal, watch.ctx.Done()
			owner.client.conversationMu.Unlock()
			if !loaded || waitForPage {
				select {
				case <-owner.observer.ctx.Done():
					return
				case <-closed:
				case <-signal:
					continue
				}
				break
			}
			if atTarget {
				break
			}
			if err := owner.client.LoadOlderConversation(owner.observer.ctx, owner.session); err != nil {
				break
			}
			if request.through == 0 || !hasOlder {
				break
			}
		}
	}
}

func conversationContainsThrough(snapshot protocol.ConversationSnapshot, sequence uint64) bool {
	for _, run := range snapshot.Page.Runs {
		if run.Sequence <= sequence {
			missing := false
			for _, turn := range snapshot.Missing {
				missing = missing || turn == run.Turn
			}
			if !missing {
				return true
			}
		}
	}
	return snapshot.Page.NextBefore == nil && len(snapshot.Missing) == 0
}

// The observer uses the ConversationView epoch under conversationMu so the
// detached snapshot and the packet cursors come from the same watch state.
func (owner *conversationObserver) detachedViewLocked(watch *conversationWatch) *observationView {
	if watch == nil || watch.projection.snapshot == nil {
		owner.previous = nil
		return nil
	}
	previous := owner.previous
	var prior ConversationState
	if previous != nil {
		prior = previous.ConversationState
	}
	current := watch.readState(&prior)
	var transcript *observationTranscript
	if previous != nil && current.SameViewSnapshot(prior) {
		return previous
	}
	if previous != nil && current.SameViewTranscript(prior) {
		transcript = previous.derived
	}
	if transcript == nil {
		transcript = &observationTranscript{}
	}
	owner.previous = &observationView{ConversationState: current, derived: transcript}
	return owner.previous
}

func (owner *conversationObserver) view() (any, error) {
	owner.mu.Lock()
	owner.client.conversationMu.Lock()
	if err := owner.observer.ctx.Err(); err != nil {
		owner.client.conversationMu.Unlock()
		owner.mu.Unlock()
		return nil, err
	}
	watch := owner.client.conversations[owner.session]
	view := owner.detachedViewLocked(watch)
	if watch == nil {
		owner.client.conversationMu.Unlock()
		owner.mu.Unlock()
		return nil, fault(protocol.ErrorCodeUnavailable, "conversation observation is closed")
	}
	state := watch.state
	generation := watch.projection.generation
	var liveStart any
	if start := watch.projection.liveStart; start != nil {
		liveStart = *start
	}
	artifactCursor := watch.projection.artifactCursor
	statisticsError := watch.projection.statisticsErr
	owner.client.conversationMu.Unlock()
	var snapshot *protocol.ConversationSnapshot
	if view != nil {
		snapshot = &view.Snapshot
	}
	value := map[string]any{
		"generation": generation, "live_start": liveStart, "snapshot": snapshot,
		"removed": state.Removed, "connected": state.Connected, "error": observerFault(state.Err),
		"statistics_error": observerFault(statisticsError), "loading_older": state.LoadingOlder, "older_error": observerFault(state.OlderErr),
		"artifact_cursor": artifactCursor, "detail_errors": []*protocol.Fault{},
	}
	if view == nil {
		owner.syncDetailsLocked(nil)
		clear(owner.sent)
		owner.mu.Unlock()
		value["calls"], value["children"] = []any{}, map[string]any{"sessions": map[string]any{}}
		value["guidance"], value["timeline"] = map[string]string{}, []protocol.TurnID{}
		value["owners"], value["segments"], value["images"] = map[string]protocol.TurnID{}, map[string]any{}, []any{}
		return value, nil
	}
	calls := view.toolCallsLocked()
	owner.syncDetailsLocked(calls)
	projected := make([]map[string]any, 0, len(calls))
	ready := map[string]bool{}
	for _, call := range calls {
		display := projectToolCall(snapshot.Page, call)
		if call.Reference != nil {
			key := detailKey(*call.Reference)
			if detail, ok := owner.client.cachedConversationDetail(key, *call.Reference); ok {
				ready[key] = true
				display["detail_state"] = map[string]any{"kind": "ready"}
				if !owner.sent[key] {
					display["detail"] = toolDetailPacket(detail, call)
					owner.sent[key] = true
				} else {
					delete(display, "detail")
				}
				display["approval_view"] = toolApprovalView(call, detail.Resolved)
			} else if failure := owner.failures[key]; failure != nil {
				if failure.Message == detailTransferLimit {
					display["detail_state"] = map[string]any{"kind": "too_large", "limit": "transfer"}
				} else if failure.Message == detailMemoryLimit {
					display["detail_state"] = map[string]any{"kind": "too_large", "limit": "memory"}
				} else {
					display["detail_state"] = map[string]any{"kind": "failed", "fault": failure, "retrying": false}
				}
			} else if _, pending := owner.reads[key]; pending {
				display["detail_state"] = map[string]any{"kind": "loading"}
			}
		}
		projected = append(projected, display)
	}
	for key := range owner.sent {
		if !ready[key] {
			delete(owner.sent, key)
		}
	}
	for _, failure := range owner.failures {
		value["detail_errors"] = append(value["detail_errors"].([]*protocol.Fault), failure)
	}
	owner.mu.Unlock()
	value["calls"] = projected
	transcript := view.derived
	if transcript.fields == nil {
		guidance := projectGuidance(snapshot.Page)
		timeline, owners, segments := projectTranscript(*snapshot, calls, guidance, owner.referenceRows)
		transcript.fields = map[string]any{
			"children": map[string]any{"sessions": projectChildren(snapshot.Page)},
			"guidance": guidance, "timeline": timeline, "owners": owners, "segments": segments,
			"images": referencedImages(*snapshot),
		}
	}
	for key, field := range transcript.fields {
		value[key] = field
	}
	return value, nil
}

func (owner *conversationObserver) syncDetailsLocked(calls []ToolCall) {
	wanted := map[string]protocol.ToolDetailReference{}
	for _, call := range calls {
		if !owner.selected[call.Source] || call.Reference == nil || call.Summary == nil ||
			!(call.Approval != nil && call.Approval.State == "pending" || call.Summary.Published || toolNeedsErrorDetail(call)) {
			continue
		}
		key := detailKey(*call.Reference)
		wanted[key] = *call.Reference
	}
	for key, read := range owner.reads {
		if _, keep := wanted[key]; !keep {
			read.cancel()
			delete(owner.reads, key)
		}
	}
	for key := range owner.failures {
		if _, keep := wanted[key]; !keep {
			delete(owner.failures, key)
		}
	}
	for key, reference := range wanted {
		if _, loaded := owner.client.cachedConversationDetail(key, reference); loaded {
			continue
		}
		if _, pending := owner.reads[key]; pending || owner.failures[key] != nil {
			continue
		}
		ctx, cancel := context.WithCancel(owner.observer.ctx)
		read := &detailRead{cancel: cancel}
		owner.reads[key] = read
		if !owner.observer.run(func() {
			_, err := owner.client.ReadConversationDetail(ctx, reference)
			owner.mu.Lock()
			if owner.reads[key] == read {
				delete(owner.reads, key)
				if err != nil && ctx.Err() == nil {
					owner.failures[key] = observerFault(err)
				}
			}
			owner.mu.Unlock()
			cancel()
			owner.observer.notify()
		}) {
			cancel()
			delete(owner.reads, key)
		}
	}
}
