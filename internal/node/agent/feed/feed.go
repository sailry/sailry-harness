// Package feed owns bounded ephemeral conversation frames, never durable history.
package feed

import (
	"context"
	"encoding/json"
	"reflect"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxFinalized     = 4096
	maxRetained      = 512
	maxRetainedBytes = 4 * 1024 * 1024
	maxDraftBytes    = 2 * 1024 * 1024
	partOverhead     = 34
	liveCapacity     = 64
)

type Hub struct {
	node     protocol.NodeID
	mu       sync.Mutex
	sessions map[protocol.SessionID]*sessionFeed
}

type sessionFeed struct {
	sequence uint64
	active   *protocol.TurnID
	drafts   []protocol.ConversationDraft
	// finalized holds the active turn's most recently committed entries, whose
	// late deltas are dropped; nil seals the turn against every delta.
	// finalizedOrder lists them oldest first so the oldest leaves at the bound.
	finalized      map[string]bool
	finalizedOrder []string
	bytes          int
	retained       []retainedFrame
	retainedBytes  int
	background     map[string]protocol.ConversationBackgroundTask
	observers      map[*Subscription]struct{}
}

type retainedFrame struct {
	frame protocol.ConversationFrame
	bytes int
}

func New(node protocol.NodeID) *Hub {
	return &Hub{node: node, sessions: make(map[protocol.SessionID]*sessionFeed)}
}
func newSession() *sessionFeed {
	return &sessionFeed{finalized: map[string]bool{}, background: map[string]protocol.ConversationBackgroundTask{}, observers: map[*Subscription]struct{}{}}
}

// accept lets the active turn stream again and forgets its committed entries;
// seal drops every later delta.
func (f *sessionFeed) accept() { f.finalized, f.finalizedOrder = map[string]bool{}, nil }
func (f *sessionFeed) seal()   { f.finalized, f.finalizedOrder = nil, nil }
func (h *Hub) prune() {
	for id, f := range h.sessions {
		if f.active == nil && len(f.observers) == 0 {
			delete(h.sessions, id)
		}
	}
}

func (h *Hub) Sequence(id protocol.SessionID) uint64 {
	h.mu.Lock()
	defer h.mu.Unlock()
	if f := h.sessions[id]; f != nil {
		return f.sequence
	}
	return 0
}

// Sessions returns only currently retained or observed conversation feeds.
func (h *Hub) Sessions() []protocol.SessionID {
	h.mu.Lock()
	defer h.mu.Unlock()
	result := make([]protocol.SessionID, 0, len(h.sessions))
	for id := range h.sessions {
		result = append(result, id)
	}
	return result
}

func (h *Hub) Reset(id protocol.SessionID) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if f := h.sessions[id]; f != nil {
		f.sequence++
		f.drafts = nil
		f.bytes = 0
		f.retained = nil
		f.retainedBytes = 0
		clear(f.background)
		f.broadcast(protocol.Update{Kind: "reset_required"})
	}
}

func (h *Hub) Seal(id protocol.SessionID, turn protocol.TurnID) []protocol.ConversationDraft {
	h.mu.Lock()
	defer h.mu.Unlock()
	f := h.sessions[id]
	if f == nil || f.active == nil || *f.active != turn {
		return nil
	}
	f.seal()
	var drafts []protocol.ConversationDraft
	for _, d := range f.drafts {
		if d.Turn == turn {
			drafts = append(drafts, copyValue(d))
		}
	}
	return drafts
}

// Publish accepts a committed change. Delta uses its separate bounded path.
func (h *Hub) Publish(id protocol.SessionID, change protocol.ConversationChange) {
	change = copyValue(change)
	h.mu.Lock()
	defer h.mu.Unlock()
	h.prune()
	f := h.sessions[id]
	run, starts := payload[protocol.ConversationRun](change.Data)
	starts = change.Kind == "run" && starts && run.Status == protocol.ConversationStatusRunning
	if f == nil {
		if !starts {
			return
		}
		f = newSession()
		h.sessions[id] = f
	}
	switch change.Kind {
	case "run":
		if run.Status == protocol.ConversationStatusRunning {
			if f.active == nil || *f.active != run.Turn {
				f.accept()
			}
			turn := run.Turn
			f.active = &turn
		}
		if run.Status == protocol.ConversationStatusStopping && f.active != nil && *f.active == run.Turn {
			f.seal()
		}
		if run.Status != protocol.ConversationStatusQueued && run.Status != protocol.ConversationStatusRunning && run.Status != protocol.ConversationStatusStopping {
			f.drafts = slices.DeleteFunc(f.drafts, func(d protocol.ConversationDraft) bool { return d.Turn == run.Turn })
			if f.active != nil && *f.active == run.Turn {
				f.active = nil
				f.accept()
			}
		}
	case "entry", "revealed", "hidden":
		var turn protocol.TurnID
		var entryID string
		if change.Kind == "hidden" {
			v, _ := payload[protocol.ConversationChangeHiddenPayload](change.Data)
			turn, entryID = v.Turn, v.ID
		} else {
			v, _ := payload[protocol.ConversationEntry](change.Data)
			turn, entryID = v.Turn, v.ID
		}
		f.drafts = slices.DeleteFunc(f.drafts, func(d protocol.ConversationDraft) bool { return d.Turn == turn && d.ID == entryID })
		if f.active != nil && *f.active == turn && f.finalized != nil && !f.finalized[entryID] {
			// A delta follows its entry's commit only briefly, so the most
			// recent entries are enough and a long turn keeps streaming.
			f.finalized[entryID] = true
			f.finalizedOrder = append(f.finalizedOrder, entryID)
			if len(f.finalizedOrder) > maxFinalized {
				delete(f.finalized, f.finalizedOrder[0])
				f.finalizedOrder = f.finalizedOrder[1:]
			}
		}
	case "delta":
		panic("conversation deltas require the bounded path")
	case "background":
		task, _ := payload[protocol.ConversationBackgroundTask](change.Data)
		if previous, ok := f.background[task.ID]; ok && reflect.DeepEqual(previous, task) {
			return
		}
		f.background[task.ID] = task
	}
	f.bytes = 0
	for _, draft := range f.drafts {
		for _, part := range draft.Parts {
			f.bytes += partBytes(part)
		}
	}
	h.send(id, f, change)
}

func (h *Hub) Delta(id protocol.SessionID, draft protocol.ConversationDraft) error {
	draft = copyValue(draft)
	h.mu.Lock()
	defer h.mu.Unlock()
	f := h.sessions[id]
	if f == nil || f.active == nil || *f.active != draft.Turn || f.finalized == nil || f.finalized[draft.ID] || len(draft.Parts) == 0 {
		return nil
	}
	index := -1
	for i, current := range f.drafts {
		if current.ID == draft.ID && current.Turn == draft.Turn {
			index = i
			break
		}
	}
	var previous *protocol.ConversationPart
	if index >= 0 && len(f.drafts[index].Parts) > 0 {
		previous = &f.drafts[index].Parts[len(f.drafts[index].Parts)-1]
	}
	bytes := 0
	for i := range draft.Parts {
		part := &draft.Parts[i]
		merged := previous != nil && previous.Kind == part.Kind && (part.Kind == "text" || part.Kind == "thinking")
		bytes += partBytes(*part)
		if merged {
			bytes -= partOverhead
		}
		previous = part
	}
	if bytes > maxDraftBytes-f.bytes {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "streaming output exceeds its buffer limit"}
	}
	if index < 0 {
		stored := draft
		stored.Parts = nil
		f.drafts = append(f.drafts, stored)
		index = len(f.drafts) - 1
	}
	current := &f.drafts[index]
	for _, part := range draft.Parts {
		last := len(current.Parts) - 1
		if last >= 0 && current.Parts[last].Kind == part.Kind && (part.Kind == "text" || part.Kind == "thinking") {
			a, _ := payload[string](current.Parts[last].Data)
			b, _ := payload[string](part.Data)
			current.Parts[last].Data = a + b
		} else {
			current.Parts = append(current.Parts, part)
		}
	}
	f.bytes += bytes
	h.send(id, f, protocol.ConversationChange{Kind: "delta", Data: draft})
	return nil
}

func partBytes(part protocol.ConversationPart) int {
	if part.Kind != "text" && part.Kind != "thinking" {
		return 0
	}
	text, _ := payload[string](part.Data)
	body, _ := json.Marshal(text)
	return len(body) + partOverhead - 2
}

func (h *Hub) send(id protocol.SessionID, f *sessionFeed, change protocol.ConversationChange) {
	f.sequence++
	frame := protocol.ConversationFrame{Node: h.node, Session: id, Sequence: f.sequence, Change: change}
	encoded, err := json.Marshal(frame)
	if err != nil {
		panic(err)
	}
	bytes := len(encoded)
	if bytes <= maxRetainedBytes {
		for len(f.retained) >= maxRetained || f.retainedBytes+bytes > maxRetainedBytes {
			f.retainedBytes -= f.retained[0].bytes
			f.retained = f.retained[1:]
		}
		f.retained = append(f.retained, retainedFrame{frame, bytes})
		f.retainedBytes += bytes
	} else {
		f.retained = nil
		f.retainedBytes = 0
	}
	f.broadcast(protocol.Update{Kind: "conversation_frame", Data: frame})
}

func (f *sessionFeed) broadcast(update protocol.Update) {
	for observer := range f.observers {
		if len(observer.queue) == liveCapacity {
			observer.queue = observer.queue[1:]
			observer.lagged = true
		}
		observer.queue = append(observer.queue, update)
		select {
		case observer.wake <- struct{}{}:
		default:
		}
	}
}

// Reserved registers before the history read, so concurrent committed frames
// cannot fall between the initial snapshot and live observation.
type Reserved struct {
	Sequence     uint64
	Drafts       []protocol.ConversationDraft
	subscription *Subscription
}

func (h *Hub) Reserve(id protocol.SessionID, closed <-chan struct{}) *Reserved {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.prune()
	f := h.sessions[id]
	if f == nil {
		f = newSession()
		h.sessions[id] = f
	}
	stream := &Subscription{hub: h, session: id, sequence: f.sequence, wake: make(chan struct{}, 1), closed: make(chan struct{}), nodeClosed: closed}
	f.observers[stream] = struct{}{}
	return &Reserved{Sequence: f.sequence, Drafts: copyValue(f.drafts), subscription: stream}
}
func (r *Reserved) Close() error { return r.subscription.Close() }

// Subscribe receives a history page already fitted with room for these drafts.
func (r *Reserved) Subscribe(snapshot protocol.ConversationSnapshot) link.Subscription {
	snapshot.Node = r.subscription.hub.node
	snapshot.Sequence = r.Sequence
	snapshot.Drafts = r.Drafts
	value := protocol.Update{Kind: "conversation_snapshot", Data: snapshot}
	r.subscription.initial = &value
	return r.subscription
}

type Subscription struct {
	hub        *Hub
	session    protocol.SessionID
	sequence   uint64
	initial    *protocol.Update
	queue      []protocol.Update
	pending    []protocol.ConversationFrame
	lagged     bool
	wake       chan struct{}
	closed     chan struct{}
	nodeClosed <-chan struct{}
	close      sync.Once
	next       sync.Mutex
}

func (s *Subscription) Close() error {
	s.close.Do(func() {
		s.hub.mu.Lock()
		defer s.hub.mu.Unlock()
		if f := s.hub.sessions[s.session]; f != nil {
			delete(f.observers, s)
		}
		close(s.closed)
	})
	return nil
}
func unavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "conversation subscription is unavailable"}
}
func (s *Subscription) resume() bool {
	f := s.hub.sessions[s.session]
	if f == nil {
		return false
	}
	var pending []protocol.ConversationFrame
	for _, item := range f.retained {
		if item.frame.Sequence > s.sequence {
			pending = append(pending, item.frame)
		}
	}
	if s.sequence != f.sequence && (len(pending) == 0 || pending[0].Sequence != s.sequence+1 || pending[len(pending)-1].Sequence != f.sequence) {
		return false
	}
	s.pending = pending
	s.queue = nil
	s.lagged = false
	return true
}
func (s *Subscription) Next(ctx context.Context) (protocol.Update, error) {
	s.next.Lock()
	defer s.next.Unlock()
	select {
	case <-s.closed:
		return protocol.Update{}, unavailable()
	case <-s.nodeClosed:
		return protocol.Update{}, unavailable()
	default:
	}
	if s.initial != nil {
		value := *s.initial
		s.initial = nil
		return value, nil
	}
	for {
		s.hub.mu.Lock()
		if len(s.pending) > 0 {
			frame := s.pending[0]
			s.pending = s.pending[1:]
			s.sequence = frame.Sequence
			s.hub.mu.Unlock()
			return protocol.Update{Kind: "conversation_frame", Data: copyValue(frame)}, nil
		}
		if s.lagged {
			ok := s.resume()
			s.hub.mu.Unlock()
			if !ok {
				return protocol.Update{Kind: "reset_required"}, nil
			}
			continue
		}
		if len(s.queue) > 0 {
			update := s.queue[0]
			s.queue = s.queue[1:]
			if update.Kind != "conversation_frame" {
				s.hub.mu.Unlock()
				return copyValue(update), nil
			}
			frame, _ := payload[protocol.ConversationFrame](update.Data)
			if frame.Sequence <= s.sequence {
				s.hub.mu.Unlock()
				continue
			}
			if frame.Sequence == s.sequence+1 {
				s.sequence = frame.Sequence
				s.hub.mu.Unlock()
				return copyValue(update), nil
			}
			ok := s.resume()
			s.hub.mu.Unlock()
			if !ok {
				return protocol.Update{Kind: "reset_required"}, nil
			}
			continue
		}
		s.hub.mu.Unlock()
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-s.closed:
			return protocol.Update{}, unavailable()
		case <-s.nodeClosed:
			return protocol.Update{}, unavailable()
		case <-s.wake:
		}
	}
}
func payload[T any](data any) (T, bool) {
	switch value := data.(type) {
	case T:
		return value, true
	case *T:
		if value != nil {
			return *value, true
		}
	}
	var zero T
	return zero, false
}
func copyValue[T any](value T) T {
	body, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	var result T
	if err := json.Unmarshal(body, &result); err != nil {
		panic(err)
	}
	return result
}
