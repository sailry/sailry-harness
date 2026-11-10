package client

import (
	"context"
	"encoding/json"
	"strconv"
	"strings"
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
	reference protocol.ToolDetailReference
	cancel    context.CancelFunc
}

type conversationObserver struct {
	client   *Client
	observer *Observer
	session  protocol.SessionID
	history  chan historyRequest
	mu       sync.Mutex
	selected map[toolReference]bool
	reads    map[string]detailRead
	failures map[string]*protocol.Fault
	sent     map[string]bool
}

func (client *Client) watchConversationView(ctx context.Context, session protocol.SessionID) (*Observer, error) {
	observer := newObserver(ctx)
	owner := &conversationObserver{
		client: client, observer: observer, session: session, history: make(chan historyRequest, 8),
		selected: map[toolReference]bool{}, reads: map[string]detailRead{}, failures: map[string]*protocol.Fault{}, sent: map[string]bool{},
	}
	release, err := client.WatchConversation(observer.ctx, session, observer.notify)
	if err != nil {
		_ = observer.Close()
		return nil, err
	}
	observer.release = release
	observer.read = owner.view
	observer.control = owner.control
	observer.run(owner.readHistory)
	observer.notify()
	return observer, nil
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
		owner.selected = map[toolReference]bool{}
		for _, reference := range selected.Items {
			owner.selected[reference] = true
		}
		state, _ := owner.client.Conversation(owner.session)
		calls := collectToolCalls(state.Snapshot.Page)
		for _, call := range calls {
			for _, retry := range selected.Retry {
				if call.Source == retry && call.Reference != nil {
					delete(owner.failures, detailKey(*call.Reference))
				}
			}
		}
		owner.mu.Unlock()
		owner.syncDetails(calls)
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
			snapshot := watch.projection.Snapshot()
			signal := watch.signal
			owner.client.conversationMu.Unlock()
			if snapshot == nil {
				select {
				case <-owner.observer.ctx.Done():
					return
				case <-signal:
					continue
				}
			}
			if request.through != 0 && conversationContainsThrough(*snapshot, request.through) {
				break
			}
			if err := owner.client.LoadOlderConversation(owner.observer.ctx, owner.session); err != nil {
				break
			}
			if request.through == 0 || snapshot.Page.NextBefore == nil && len(snapshot.Missing) == 0 {
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

func (owner *conversationObserver) view() (any, error) {
	owner.client.conversationMu.Lock()
	watch := owner.client.conversations[owner.session]
	if watch == nil {
		owner.client.conversationMu.Unlock()
		return nil, fault(protocol.ErrorCodeUnavailable, "conversation observation is closed")
	}
	state := watch.state
	snapshot := watch.projection.Snapshot()
	generation := watch.projection.generation
	liveStart := watch.projection.liveStart
	artifactCursor := watch.projection.artifactCursor
	statisticsError := watch.projection.statisticsErr
	owner.client.conversationMu.Unlock()
	value := map[string]any{
		"generation": generation, "live_start": liveStart, "snapshot": snapshot,
		"calls": []any{}, "children": map[string]any{"sessions": map[string]any{}},
		"guidance": map[string]string{}, "timeline": []protocol.TurnID{},
		"removed": state.Removed, "connected": state.Connected, "error": observerFault(state.Err),
		"statistics_error": observerFault(statisticsError), "loading_older": state.LoadingOlder, "older_error": observerFault(state.OlderErr),
		"artifact_cursor": artifactCursor, "detail_errors": []*protocol.Fault{},
		"owners": map[string]protocol.TurnID{}, "segments": map[string]any{}, "images": []any{},
	}
	if snapshot == nil {
		return value, nil
	}
	calls := collectToolCalls(snapshot.Page)
	owner.syncDetails(calls)
	owner.mu.Lock()
	projected := make([]map[string]any, 0, len(calls))
	ready := map[string]bool{}
	for _, call := range calls {
		display := projectToolCall(snapshot.Page, call)
		if call.Reference != nil {
			key := detailKey(*call.Reference)
			if detail, ok := owner.client.ConversationDetail(*call.Reference); ok {
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
				if strings.Contains(failure.Message, "exceeds the transfer limit") {
					display["detail_state"] = map[string]any{"kind": "too_large", "limit": "transfer"}
				} else if strings.Contains(failure.Message, "memory budget") {
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
	value["children"] = map[string]any{"sessions": projectChildren(snapshot.Page)}
	guidance := projectGuidance(snapshot.Page)
	value["guidance"] = guidance
	timeline, owners, segments := projectTranscript(*snapshot, calls, guidance)
	value["timeline"], value["owners"], value["segments"] = timeline, owners, segments
	value["images"] = referencedImages(*snapshot)
	return value, nil
}

func (owner *conversationObserver) syncDetails(calls []projectedToolCall) {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	wanted := map[string]protocol.ToolDetailReference{}
	for _, call := range calls {
		if !owner.selected[call.Source] || call.Reference == nil || call.Summary == nil ||
			!(call.Approval != nil && call.Approval.State == "pending" || call.Summary.Published) {
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
		if _, loaded := owner.client.ConversationDetail(reference); loaded {
			continue
		}
		if _, pending := owner.reads[key]; pending || owner.failures[key] != nil {
			continue
		}
		ctx, cancel := context.WithCancel(owner.observer.ctx)
		owner.reads[key] = detailRead{reference: reference, cancel: cancel}
		if !owner.observer.run(func() {
			_, err := owner.client.ReadConversationDetail(ctx, reference)
			owner.mu.Lock()
			if active, exists := owner.reads[key]; exists && active.reference == reference {
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
