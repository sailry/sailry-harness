// Package feed owns bounded ephemeral conversation frames, never durable history.
package feed

import (
	"context"
	"encoding/json"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxFinalized  = 4096
	maxDraftBytes = 2 * 1024 * 1024
	partOverhead  = 34
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
	replay         replay
	background     map[string]uint64
	observers      map[*Subscription]struct{}
}

func New(node protocol.NodeID) *Hub {
	return &Hub{node: node, sessions: make(map[protocol.SessionID]*sessionFeed)}
}
func newSession() *sessionFeed {
	return &sessionFeed{finalized: map[string]bool{}, observers: map[*Subscription]struct{}{}}
}

// accept lets the active turn stream again and forgets its committed entries;
// seal drops every later delta.
func (f *sessionFeed) accept() { f.finalized, f.finalizedOrder = map[string]bool{}, nil }
func (f *sessionFeed) seal()   { f.finalized, f.finalizedOrder = nil, nil }
func (h *Hub) releaseIdle(id protocol.SessionID, f *sessionFeed) {
	if f.active == nil && len(f.observers) == 0 {
		delete(h.sessions, id)
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

// Observed includes reservations made before the initial history read completes.
func (h *Hub) Observed(id protocol.SessionID) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	f := h.sessions[id]
	return f != nil && len(f.observers) != 0
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
		f.replay = replay{}
		clear(f.background)
		f.notify()
	}
}

func (h *Hub) Seal(id protocol.SessionID, turn protocol.TurnID) {
	h.mu.Lock()
	defer h.mu.Unlock()
	f := h.sessions[id]
	if f == nil || f.active == nil || *f.active != turn {
		return
	}
	f.seal()
}

// Publish accepts a committed change. Delta uses its separate bounded path.
func (h *Hub) Publish(id protocol.SessionID, change protocol.ConversationChange) {
	h.mu.Lock()
	defer h.mu.Unlock()
	f := h.sessions[id]
	run, isRun := payload[protocol.ConversationRun](change.Data)
	starts := change.Kind == "run" && isRun && run.Status == protocol.ConversationStatusRunning
	if f == nil {
		if !starts {
			return
		}
		f = newSession()
		h.sessions[id] = f
	}
	var key metadataKey
	draftCount := len(f.drafts)
	switch change.Kind {
	case "run":
		key = metadataKey{kind: "run", turn: run.Turn}
		if run.Status == protocol.ConversationStatusRunning && (f.active == nil || *f.active != run.Turn) {
			f.accept()
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
	case "queue":
		key = metadataKey{kind: "queue"}
	case "entry", "revealed", "hidden":
		var turn protocol.TurnID
		var entryID string
		var affectsInput bool
		if change.Kind == "hidden" {
			v, _ := payload[protocol.ConversationChangeHiddenPayload](change.Data)
			turn, entryID = v.Turn, v.ID
			affectsInput = true
		} else {
			v, _ := payload[protocol.ConversationEntry](change.Data)
			turn, entryID = v.Turn, v.ID
			affectsInput = v.Author == "user"
		}
		if affectsInput {
			// A canonical user entry can replace the projected sent input.
			delete(f.replay.metadata, metadataKey{kind: "run", turn: turn})
		}
		f.drafts = slices.DeleteFunc(f.drafts, func(d protocol.ConversationDraft) bool { return d.Turn == turn && d.ID == entryID })
		if f.active != nil && *f.active == turn && f.finalized != nil && !f.finalized[entryID] {
			// A delta follows its entry's commit only briefly, so the most
			// recent entries are enough and a long turn keeps streaming.
			f.finalized[entryID] = true
			f.finalizedOrder = append(f.finalizedOrder, entryID)
			if len(f.finalizedOrder) > maxFinalized {
				delete(f.finalized, f.finalizedOrder[0])
				clear(f.finalizedOrder[:1])
				f.finalizedOrder = f.finalizedOrder[1:]
			}
		}
	case "delta":
		panic("conversation deltas require the bounded path")
	}
	if len(f.drafts) != draftCount {
		f.bytes = 0
		for _, draft := range f.drafts {
			for _, part := range draft.Parts {
				f.bytes += partBytes(part)
			}
		}
	}
	f.publish(change, key)
	h.releaseIdle(id, f)
}

// PublishBackground retains deduplication state only for the background task
// snapshot still published by the store. Older completions stay in history.
func (h *Hub) PublishBackground(id protocol.SessionID, tasks []protocol.ConversationBackgroundTask) {
	h.mu.Lock()
	defer h.mu.Unlock()
	f := h.sessions[id]
	if f == nil || len(f.observers) == 0 {
		return
	}
	current := make(map[string]uint64, len(tasks))
	for _, task := range tasks {
		current[task.ID] = task.Revision
		if previous, ok := f.background[task.ID]; ok && previous == task.Revision {
			continue
		}
		f.publish(protocol.ConversationChange{Kind: "background", Data: task}, metadataKey{})
	}
	f.background = current
}

func (h *Hub) Delta(id protocol.SessionID, draft protocol.ConversationDraft) error {
	body := encodeChange(protocol.ConversationChange{Kind: "delta", Data: draft})
	draft = *decodeChange(body).Data.(*protocol.ConversationDraft)
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
	f.advance(body, metadataKey{})
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

func encodeChange(change protocol.ConversationChange) []byte {
	body, err := json.Marshal(change)
	if err != nil {
		panic(err)
	}
	return body
}

func decodeChange(body []byte) protocol.ConversationChange {
	var change protocol.ConversationChange
	if err := json.Unmarshal(body, &change); err != nil {
		panic(err)
	}
	return change
}

func (f *sessionFeed) publish(change protocol.ConversationChange, key metadataKey) {
	var body []byte
	if len(f.observers) != 0 {
		body = encodeChange(change)
		if key.kind != "" && f.replay.duplicate(key, body) {
			return
		}
	}
	f.advance(body, key)
}

func (f *sessionFeed) advance(body []byte, key metadataKey) {
	f.sequence++
	// New observers read a fresh durable snapshot at this sequence.
	if len(f.observers) != 0 {
		f.replay.append(f.sequence, body, key)
		f.notify()
	}
}

func (f *sessionFeed) notify() {
	for observer := range f.observers {
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
	f := h.sessions[id]
	if f == nil {
		f = newSession()
		h.sessions[id] = f
	}
	stream := &Subscription{hub: h, session: id, sequence: f.sequence, wake: make(chan struct{}, 1), closed: make(chan struct{}), nodeClosed: closed}
	f.observers[stream] = struct{}{}
	// A fitted snapshot need not contain every run. Do not hide its first
	// authoritative publication behind a frame older than the reservation.
	clear(f.replay.metadata)
	return &Reserved{Sequence: f.sequence, Drafts: copyValue(f.drafts), subscription: stream}
}
func (r *Reserved) Close() error { return r.subscription.Close() }

// Subscribe receives a history page already fitted with room for these drafts.
func (r *Reserved) Subscribe(snapshot protocol.ConversationSnapshot) link.Subscription {
	s := r.subscription
	snapshot.Node = s.hub.node
	snapshot.Sequence = r.Sequence
	snapshot.Drafts = r.Drafts
	value := protocol.Update{Kind: "conversation_snapshot", Data: snapshot}
	s.hub.mu.Lock()
	defer s.hub.mu.Unlock()
	select {
	case <-s.closed:
	default:
		s.initial = &value
	}
	return s
}

type Subscription struct {
	hub        *Hub
	session    protocol.SessionID
	sequence   uint64
	initial    *protocol.Update
	wake       chan struct{}
	closed     chan struct{}
	nodeClosed <-chan struct{}
	next       sync.Mutex
}

func (s *Subscription) Close() error {
	s.hub.mu.Lock()
	defer s.hub.mu.Unlock()
	s.closeLocked()
	return nil
}

func (s *Subscription) closeLocked() {
	select {
	case <-s.closed:
		return
	default:
	}
	f := s.hub.sessions[s.session]
	delete(f.observers, s)
	if len(f.observers) == 0 {
		f.replay, f.background = replay{}, nil
	}
	s.hub.releaseIdle(s.session, f)
	s.initial = nil
	close(s.closed)
}

func unavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "conversation subscription is unavailable"}
}

func (s *Subscription) Next(ctx context.Context) (protocol.Update, error) {
	s.next.Lock()
	defer s.next.Unlock()
	for {
		s.hub.mu.Lock()
		select {
		case <-s.closed:
			s.hub.mu.Unlock()
			return protocol.Update{}, unavailable()
		case <-s.nodeClosed:
			s.closeLocked()
			s.hub.mu.Unlock()
			return protocol.Update{}, unavailable()
		default:
		}
		if s.initial != nil {
			value := *s.initial
			s.initial = nil
			s.hub.mu.Unlock()
			return value, nil
		}
		if f := s.hub.sessions[s.session]; s.sequence != f.sequence {
			sequence := s.sequence + 1
			body := f.replay.at(sequence)
			if body == nil {
				// This cursor cannot continue. The client must reserve again
				// and read canonical history; do not stream a broken suffix.
				s.closeLocked()
				s.hub.mu.Unlock()
				return protocol.Update{Kind: "reset_required"}, nil
			}
			s.sequence = sequence
			s.hub.mu.Unlock()
			frame := &protocol.ConversationFrame{Node: s.hub.node, Session: s.session, Sequence: sequence, Change: decodeChange(body)}
			return protocol.Update{Kind: "conversation_frame", Data: frame}, nil
		}
		s.hub.mu.Unlock()
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-s.closed:
			return protocol.Update{}, unavailable()
		case <-s.nodeClosed:
			_ = s.Close()
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
