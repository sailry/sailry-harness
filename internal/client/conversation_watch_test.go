package client

import (
	"context"
	"encoding/json"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type conversationTransport struct {
	mu       sync.Mutex
	requests []protocol.Request
	opened   chan *clientSubscription
	node     *clientSubscription
	sequence uint64
	count    int
	initial  func() protocol.ConversationSnapshot
}

func (t *conversationTransport) Target() protocol.NodeID { return protocol.NodeID{1} }
func (t *conversationTransport) WatchConnection(ctx context.Context, changed func(link.ConnectionInfo)) error {
	return link.NewLocal(t.Target(), t.Target(), nil).WatchConnection(ctx, changed)
}
func (t *conversationTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "node" {
		return t.node, nil
	}
	t.mu.Lock()
	t.count++
	t.sequence = uint64(8 + 2*t.count)
	sequence := t.sequence
	t.mu.Unlock()
	s := conversationFixture()
	s.Sequence = sequence
	s.Page.Runs[0].Turn = protocol.TurnID{3}
	s.Page.Runs[0].Sequence = 3
	s.Page.Runs[0].Status = "completed"
	s.Page.Entries[0].Turn = protocol.TurnID{3}
	s.Page.Entries[0].Sequence = 3
	s.Page.NextBefore = new(protocol.TurnID{3})
	if t.initial != nil {
		s = t.initial()
	}
	sub := &clientSubscription{updates: make(chan protocol.Update, 8), failures: make(chan error, 1), closed: make(chan struct{})}
	sub.updates <- protocol.Update{Kind: "conversation_snapshot", Data: s}
	t.opened <- sub
	return sub, nil
}
func (t *conversationTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	t.mu.Lock()
	t.requests = append(t.requests, request)
	sequence := t.sequence
	t.mu.Unlock()
	p := protocol.DefaultConversationPage()
	p.Session = protocol.SessionID{1}
	p.Revision = 1
	for i := byte(1); i <= 2; i++ {
		p.Runs = append(p.Runs, protocol.ConversationRun{Session: p.Session, Turn: protocol.TurnID{i}, Sequence: uint64(i), Revision: 1, Kind: "task", Status: "completed"})
		p.Entries = append(p.Entries, protocol.ConversationEntry{ID: string([]byte{'a' + i}), Turn: protocol.TurnID{i}, Sequence: uint64(i), Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Older"}}})
	}
	done := make(chan link.Response, 1)
	done <- link.Response{Output: protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{Sequence: sequence, Page: p}}}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: done}, nil
}

func testConversationWatchRecovery(t *testing.T) {
	snapshot := projectionSnapshot(0)
	session := projectionSession(1, 1)
	snapshot.Sessions = []protocol.Session{session}
	snapshot.Worktrees = []protocol.Worktree{{ID: session.Worktree}}
	transport := &conversationTransport{node: testSubscription(snapshot), opened: make(chan *clientSubscription, 8)}
	c := New(transport)
	if err := c.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	changes := make(chan struct{}, 32)
	changed := func() {
		select {
		case changes <- struct{}{}:
		default:
		}
	}
	wait := func(predicate func(ConversationState) bool) {
		t.Helper()
		for {
			state, ok := c.Conversation(session.ID)
			if ok && predicate(state) {
				return
			}
			select {
			case <-changes:
			case <-t.Context().Done():
				t.Fatal(t.Context().Err())
			}
		}
	}
	release, err := c.WatchConversation(t.Context(), session.ID, changed)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	other, err := c.WatchConversation(t.Context(), session.ID, changed)
	if err != nil {
		t.Fatal(err)
	}
	defer other()
	first := <-transport.opened
	wait(func(s ConversationState) bool { return s.Connected })
	transport.mu.Lock()
	count := transport.count
	transport.mu.Unlock()
	if count != 1 {
		t.Fatal("observers did not share a subscription")
	}
	if err := c.LoadOlderConversation(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	wait(func(s ConversationState) bool {
		return len(s.Snapshot.Page.Runs) == 3 && s.Snapshot.Page.NextBefore == nil
	})
	if err := c.RefreshConversation(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	second := <-transport.opened
	wait(func(s ConversationState) bool {
		return s.Connected && s.Snapshot.Sequence == 12 && len(s.Snapshot.Page.Runs) == 3 && s.Snapshot.Page.NextBefore == nil
	})
	select {
	case <-first.closed:
	default:
		t.Fatal("refresh retained its old stream")
	}
	transport.mu.Lock()
	requests := append([]protocol.Request(nil), transport.requests...)
	transport.mu.Unlock()
	if len(requests) != 2 {
		t.Fatalf("history reads=%d", len(requests))
	}
	for _, request := range requests {
		query, ok := request.Command.Data.(protocol.CommandReadConversationPayload)
		if request.Command.Kind != "read_conversation" || !ok || query.Limit != 20 || query.Before == nil || *query.Before != (protocol.TurnID{3}) {
			t.Fatal("reconnect replayed business input or lost pagination scope")
		}
	}
	release()
	select {
	case <-second.closed:
		t.Fatal("releasing one observer stopped another")
	default:
	}
	other()
	if _, kept := c.Conversation(session.ID); kept {
		t.Fatal("last release retained a readable conversation")
	}
	reopened, err := c.WatchConversation(t.Context(), session.ID, changed)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened()
	third := <-transport.opened
	wait(func(s ConversationState) bool {
		return s.Connected && s.Snapshot.Sequence == 14 && len(s.Snapshot.Page.Runs) == 3 && s.Snapshot.Page.NextBefore == nil
	})
	transport.node.updates <- projectionEvent(1, "sessions_removed", []protocol.SessionID{session.ID})
	wait(func(s ConversationState) bool { return s.Removed })
	select {
	case <-third.closed:
	case <-t.Context().Done():
		t.Fatal(t.Context().Err())
	}
	if state, kept := c.Conversation(session.ID); !kept || len(state.Snapshot.Page.Entries) != 0 {
		t.Fatalf("a removed conversation did not leave just its removal state: %+v", state)
	}
	reopened()
	if _, kept := c.Conversation(session.ID); kept {
		t.Fatal("a removed conversation without observers kept its watch")
	}
	if err := c.Close(); err != nil {
		t.Fatal(err)
	}
}

func (t *conversationTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "fixture stream is unavailable"}
}

type gatedConversationTransport struct {
	*conversationTransport
	snapshot <-chan struct{}
}

func (t *gatedConversationTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "conversation" {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-t.snapshot:
		}
	}
	return t.conversationTransport.Subscribe(ctx, topic)
}

func TestConversationWatchReopensWhilePreviousPageIsClosing(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	gate := make(chan struct{})
	transport := &conversationTransport{opened: make(chan *clientSubscription, 1)}
	c := New(&gatedConversationTransport{conversationTransport: transport, snapshot: gate})
	t.Cleanup(func() { _ = c.Close() })

	old := conversationFixture()
	old.Page.Entries[0].Parts[0].Data = "Previous result"
	projection := NewConversationProjection(old.Node, old.Page.Session, 1)
	if _, err := projection.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: old}); err != nil {
		t.Fatal(err)
	}
	closed, stop := context.WithCancel(ctx)
	stop()
	// The prior observer was released while its read still waited for the
	// transport. Its cleanup has not yet cleared the owner's busy state.
	c.conversations = map[protocol.SessionID]*conversationWatch{old.Page.Session: {
		session: old.Page.Session, ctx: closed, cancel: stop, projection: projection,
		state: ConversationState{LoadingOlder: true}, extent: conversationExtentOf(old),
	}}
	changed := make(chan struct{}, 1)
	release, err := c.WatchConversation(ctx, old.Page.Session, func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	state, _ := c.Conversation(old.Page.Session)
	if !state.Loading || state.Connected || state.LoadingOlder || len(state.Snapshot.Page.Entries) != 0 {
		t.Fatalf("reopened conversation inherited an unverified view or busy read: %+v", state)
	}
	close(gate)
	for {
		state, _ = c.Conversation(old.Page.Session)
		if state.Connected && state.Snapshot.Page.NextBefore == nil && len(state.Snapshot.Page.Entries) == 3 {
			if got := state.Snapshot.Page.Entries[0].Parts[0].Data; got != "Older" {
				t.Fatalf("same revision, but the latest page was not used: %v", got)
			}
			return
		}
		select {
		case <-changed:
		case <-ctx.Done():
			t.Fatalf("reopened conversation did not restore its range: %+v", state)
		}
	}
}

func TestConversationObserverDropsRevokedHistory(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	snapshot := conversationFixture()
	snapshot.Page.Runs[0].Worktree = protocol.WorktreeID{1}
	transport := &conversationTransport{opened: make(chan *clientSubscription, 1), initial: func() protocol.ConversationSnapshot { return snapshot }}
	c := New(transport)
	defer c.Close()
	observer, err := c.watchConversationView(ctx, snapshot.Page.Session)
	if err != nil {
		t.Fatal(err)
	}
	defer observer.Close()
	sub := <-transport.opened
	read := func(ready func(bool, *protocol.Fault) bool) *protocol.ConversationSnapshot {
		t.Helper()
		for {
			packet, err := observer.Next(ctx)
			if err != nil {
				t.Fatal(err)
			}
			var view struct {
				Snapshot  *protocol.ConversationSnapshot `json:"snapshot"`
				Connected bool                           `json:"connected"`
				Fault     *protocol.Fault                `json:"error"`
			}
			if err := json.Unmarshal(packet, &view); err != nil {
				t.Fatal(err)
			}
			if ready(view.Connected, view.Fault) {
				return view.Snapshot
			}
		}
	}
	if view := read(func(connected bool, _ *protocol.Fault) bool { return connected }); view == nil || len(view.Page.Entries) == 0 {
		t.Fatalf("authorized observer did not receive the transcript: %+v", view)
	}
	sub.failures <- fault(protocol.ErrorCodePermissionDenied, "conversation access was revoked")
	if view := read(func(_ bool, err *protocol.Fault) bool {
		return err != nil && err.Code == protocol.ErrorCodePermissionDenied
	}); view != nil {
		t.Fatal("the bridge emitted a stale transcript after the Node denied access")
	}
	if state, _ := c.Conversation(snapshot.Page.Session); len(state.Snapshot.Page.Entries) != 0 || state.Connected {
		t.Fatal("the Go view retained an authorized transcript after access was revoked")
	}
}

func TestConversationWatchDropsQueuedSnapshotAfterRemoval(t *testing.T) {
	const generation = 1
	snapshot := conversationFixture()
	session := projectionSession(1, 1)
	node := projectionSnapshot(0)
	node.Sessions = []protocol.Session{session}
	node.Worktrees = []protocol.Worktree{{ID: session.Worktree}}
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	w := &conversationWatch{
		session:    session.ID,
		projection: NewConversationProjection(snapshot.Node, session.ID, generation),
		ctx:        ctx,
		cancel:     cancel,
		signal:     make(chan struct{}),
		callbacks:  map[uint64]func(){1: nil},
	}
	c := &Client{project: NewProjection(node.Node, generation), conversations: map[protocol.SessionID]*conversationWatch{session.ID: w}}
	if applied, err := c.apply(t.Context(), generation, protocol.Update{Kind: "snapshot", Data: node}); err != nil || applied != Applied {
		t.Fatalf("initial Node snapshot = %v, %v", applied, err)
	}
	if applied, err := c.applyConversationUpdate(w, generation, protocol.Update{Kind: "conversation_snapshot", Data: snapshot}); err != nil || applied != Applied {
		t.Fatalf("authorized conversation snapshot = %v, %v", applied, err)
	}
	snapshot.Sequence++
	stream := make(chan conversationIncoming, 1)
	stream <- conversationIncoming{update: protocol.Update{Kind: "conversation_snapshot", Data: snapshot}}
	incoming := <-stream
	if applied, err := c.apply(t.Context(), generation, projectionEvent(1, "sessions_removed", []protocol.SessionID{session.ID})); err != nil || applied != Applied {
		t.Fatalf("session removal = %v, %v", applied, err)
	}
	if c.conversations[session.ID] != w || !w.state.Removed || ctx.Err() != context.Canceled {
		t.Fatal("removal did not cancel and retain the watch for its observer")
	}
	if applied, err := c.applyConversationUpdate(w, generation, incoming.update); applied != Ignored || err != context.Canceled {
		t.Fatalf("queued snapshot after removal = %v, %v", applied, err)
	}
	if state, ok := c.Conversation(session.ID); !ok || !state.Removed || state.Connected || faultCode(state.Err) != protocol.ErrorCodeNotFound || state.Snapshot.Sequence != 0 || len(state.Snapshot.Page.Entries) != 0 {
		t.Fatalf("queued snapshot changed the removal state: %+v", state)
	}
}

func TestConversationViewReusesUnchangedHistoryDuringStreaming(t *testing.T) {
	const generation = 1
	snapshot := conversationFixture()
	session, turn := snapshot.Page.Session, snapshot.Page.Runs[0].Turn
	w := &conversationWatch{session: session, projection: NewConversationProjection(snapshot.Node, session, generation), ctx: t.Context()}
	c := &Client{conversations: map[protocol.SessionID]*conversationWatch{session: w}}
	apply := func(update protocol.Update) {
		t.Helper()
		if got, err := c.applyConversationUpdate(w, generation, update); got != Applied || err != nil {
			t.Fatalf("update %s = %v, %v", update.Kind, got, err)
		}
	}
	apply(protocol.Update{Kind: "conversation_snapshot", Data: snapshot})
	initial, _ := c.ConversationView(session, ConversationState{})
	apply(conversationFrame(*w.projection.snapshot, "delta", protocol.ConversationDraft{
		ID: "stream", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Working"}},
	}))
	streaming, _ := c.ConversationView(session, initial)
	if streaming.SameViewSnapshot(initial) || len(streaming.Snapshot.Drafts) != 1 || len(initial.Snapshot.Drafts) != 0 {
		t.Fatal("the live draft did not update independently of the previous view")
	}
	if &streaming.Snapshot.Page.Entries[0] != &initial.Snapshot.Page.Entries[0] {
		t.Fatal("a streaming draft copied the unchanged canonical history")
	}
	entry := protocol.ConversationEntry{ID: "stream", Turn: turn, Sequence: 2, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Original"}}}
	apply(conversationFrame(*w.projection.snapshot, "entry", entry))
	finished, _ := c.ConversationView(session, streaming)
	if len(finished.Snapshot.Page.Entries) != 2 || &finished.Snapshot.Page.Entries[0].Parts[0] != &initial.Snapshot.Page.Entries[0].Parts[0] {
		t.Fatal("appending a canonical entry copied unmodified entry bodies")
	}
	apply(conversationFrame(*w.projection.snapshot, "hidden", protocol.ConversationChangeHiddenPayload{ID: entry.ID, Turn: turn}))
	entry.Parts[0].Data = "Revealed"
	apply(conversationFrame(*w.projection.snapshot, "revealed", entry))
	revealed, _ := c.ConversationView(session, finished)
	if len(revealed.Snapshot.Page.Entries) != 2 || revealed.Snapshot.Page.Entries[1].Parts[0].Data != "Revealed" || finished.Snapshot.Page.Entries[1].Parts[0].Data != "Original" {
		t.Fatal("a hide-and-reveal cycle reused the obsolete projection")
	}
	mutable, _ := c.Conversation(session)
	mutable.Snapshot.Page.Entries[0].Parts[0].Data = "Changed"
	if revealed.Snapshot.Page.Entries[0].Parts[0].Data != "Request" {
		t.Fatal("a mutable action read aliased the presentation")
	}
}

type requestedTurnPage struct {
	query    protocol.CommandReadTurnPayload
	complete chan link.Response
}

type turnPagingTransport struct {
	*conversationTransport
	pages chan requestedTurnPage
}

func (t *turnPagingTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	query, ok := request.Command.Data.(protocol.CommandReadTurnPayload)
	if request.Command.Kind != "read_turn" || !ok {
		return link.Admission{}, fault(protocol.ErrorCodeInvalidRequest, "fixture expects a turn page")
	}
	complete := make(chan link.Response, 1)
	select {
	case t.pages <- requestedTurnPage{query, complete}:
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: complete}, nil
	case <-ctx.Done():
		return link.Admission{}, ctx.Err()
	}
}

func TestConversationHistoryResumesOriginalRangeAfterInterruptedRestore(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	s := conversationFixture()
	run := s.Page.Runs[0]
	run.Status = protocol.ConversationStatusCompleted
	s.Page.Runs[0] = run
	entry := func(n uint64, text string) protocol.ConversationEntry {
		return protocol.ConversationEntry{ID: text, Turn: run.Turn, Sequence: n, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: text}}}
	}
	s.Page.Entries = []protocol.ConversationEntry{entry(4, "Tail")}
	s.Missing = []protocol.TurnID{run.Turn}
	transport := &turnPagingTransport{
		conversationTransport: &conversationTransport{opened: make(chan *clientSubscription, 3), initial: func() protocol.ConversationSnapshot { return s }},
		pages:                 make(chan requestedTurnPage),
	}
	c := New(transport)
	defer c.Close()
	changed := make(chan struct{}, 1)
	release, err := c.WatchConversation(ctx, run.Session, func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	wait := func(ready func(ConversationState) bool) {
		t.Helper()
		for {
			if state, _ := c.Conversation(run.Session); ready(state) {
				return
			}
			select {
			case <-changed:
			case <-ctx.Done():
				t.Fatal("conversation history did not recover", ctx.Err())
			}
		}
	}
	read := func(before uint64, limit uint16) requestedTurnPage {
		t.Helper()
		select {
		case request := <-transport.pages:
			q := request.query
			if q.Session != run.Session || q.Turn != run.Turn || q.ExpectedRevision != s.Page.Revision || q.Limit != limit || q.Before == nil || *q.Before != before {
				t.Fatalf("wrong turn continuation at %d: %+v", before, q)
			}
			return request
		case <-ctx.Done():
			t.Fatal("no turn continuation at", before)
			return requestedTurnPage{}
		}
	}
	respond := func(request requestedTurnPage, entries []protocol.ConversationEntry, more bool) {
		h := protocol.ConversationTurnHistory{Sequence: s.Sequence, Revision: s.Page.Revision, Run: run, Entries: entries}
		if more {
			h.NextBefore = new(entries[0].Sequence)
		}
		request.complete <- link.Response{Output: protocol.Output{Kind: "turn_history", Data: h}}
	}
	refresh := func() {
		t.Helper()
		if err := c.RefreshConversation(ctx, run.Session); err != nil {
			t.Fatal(err)
		}
	}
	wait(func(state ConversationState) bool { return state.Connected })
	manual := make(chan error, 1)
	go func() { manual <- c.LoadOlderConversation(ctx, run.Session) }()
	respond(read(4, 500), []protocol.ConversationEntry{entry(1, "Old prefix"), entry(2, "Second"), entry(3, "Third")}, false)
	if err := <-manual; err != nil {
		t.Fatal(err)
	}

	refresh()
	respond(read(4, 1000), []protocol.ConversationEntry{entry(3, "Third")}, true)
	interrupted := read(3, 1000)
	refresh()
	respond(read(4, 1000), []protocol.ConversationEntry{entry(3, "Third")}, true)
	respond(interrupted, []protocol.ConversationEntry{entry(1, "Obsolete prefix")}, false)
	respond(read(3, 1000), []protocol.ConversationEntry{entry(1, "Current prefix"), entry(2, "Second")}, false)
	wait(func(state ConversationState) bool {
		page := state.Snapshot.Page
		return !state.LoadingOlder && state.OlderErr == nil && len(state.Snapshot.Missing) == 0 && len(page.Entries) == 4 && page.Entries[0].ID == "Current prefix"
	})
}

func TestLatestLocalTurnStatus(t *testing.T) {
	session, turn := protocol.SessionID{1}, protocol.TurnID{2}
	snapshot := conversationFixture()
	snapshot.Page.Runs = []protocol.ConversationRun{
		{Sequence: 2, Turn: turn, Status: protocol.ConversationStatusRunning},
		{Sequence: 1, Turn: protocol.TurnID{1}, Status: protocol.ConversationStatusCompleted},
	}
	watch := &conversationWatch{projection: &ConversationProjection{snapshot: &snapshot}}
	c := &Client{conversations: map[protocol.SessionID]*conversationWatch{session: watch}}
	if n := testing.AllocsPerRun(10, func() {
		if status, ok := c.LatestLocalTurnStatus(session, turn); !ok || status != protocol.ConversationStatusRunning {
			t.Fatal(status, ok)
		}
	}); n != 0 {
		t.Fatalf("reading turn status allocated %v times", n)
	}
	if _, ok := c.LatestLocalTurnStatus(session, protocol.TurnID{1}); ok {
		t.Fatal("older turn returned")
	}
	snapshot.Page.Runs[0].Origin = new(protocol.SessionID{9})
	if _, ok := c.LatestLocalTurnStatus(session, turn); ok {
		t.Fatal("inherited turn returned")
	}
	snapshot.Page.Runs[0].Origin = nil
	watch.state.Removed = true
	if _, ok := c.LatestLocalTurnStatus(session, turn); ok {
		t.Fatal("removed watch returned")
	}
}
