package client

import (
	"context"
	"errors"
	"io"
	"slices"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ConversationState struct {
	Snapshot                                  protocol.ConversationSnapshot
	Connected, Loading, LoadingOlder, Removed bool
	Err, OlderErr                             error
	// StatisticsErr is the fault of the Node's last statistics refresh, until
	// statistics or a snapshot arrive.
	StatisticsErr error
	// InventoryVersion changes when the Node's canonical asset inventory may
	// have changed. It is a local cache key, not the structural page revision.
	InventoryVersion uint64 `json:"-"`
	entryEpoch       *byte
	viewVersion      uint64
	transcript       uint64
}

type conversationWatch struct {
	session         protocol.SessionID
	projection      *ConversationProjection
	viewVersion     uint64
	state           ConversationState
	ctx             context.Context
	cancel          context.CancelFunc
	done            chan struct{}
	refresh, signal chan struct{}
	callbacks       map[uint64]func()
	next            uint64
	extent          conversationExtent
	known           bool
}

const conversationBookmarkLimit = 64

type conversationBookmark struct {
	session protocol.SessionID
	extent  conversationExtent
}

func (w *conversationWatch) discardHistory() {
	w.projection.snapshot = nil
	w.projection.draftText.Reset()
	w.projection.entryEpoch = nil
	w.projection.liveStart = nil
	w.projection.statisticsErr = nil
	w.projection.hidden, w.projection.revealed = nil, nil
	w.extent = conversationExtent{}
	w.state.OlderErr = nil
	w.state.InventoryVersion = 0
}

// A second reconnect during automatic paging must recover the original range,
// not just the prefix the interrupted generation had time to load.
func (w *conversationWatch) rememberExtent() {
	if snapshot := w.projection.snapshot; snapshot != nil && !w.extent.needsMore(*snapshot) {
		w.extent = conversationExtentOf(*snapshot)
	}
}

func (c *Client) Conversation(session protocol.SessionID) (ConversationState, bool) {
	return c.readConversationState(session, nil)
}

// LatestLocalTurnStatus reports the status when turn is the latest run in the
// watched session and originated there. It reads only run metadata.
func (c *Client) LatestLocalTurnStatus(session protocol.SessionID, turn protocol.TurnID) (protocol.ConversationStatus, bool) {
	c.conversationMu.Lock()
	defer c.conversationMu.Unlock()
	w := c.conversations[session]
	if w == nil || w.state.Removed || w.projection.snapshot == nil {
		return "", false
	}
	runs := w.projection.snapshot.Page.Runs
	var latest *protocol.ConversationRun
	for i := range runs {
		if latest == nil || runs[i].Sequence > latest.Sequence {
			latest = &runs[i]
		}
	}
	if latest == nil || latest.Turn != turn || latest.Origin != nil {
		return "", false
	}
	return latest.Status, true
}

// ConversationView returns a detached snapshot for a read-only presentation.
// Passing its previous unmodified result reuses immutable canonical entries.
// Ordinary Conversation callers continue to receive independently mutable copies.
func (c *Client) ConversationView(session protocol.SessionID, previous ConversationState) (ConversationState, bool) {
	return c.readConversationState(session, &previous)
}

func (c *Client) readConversationState(session protocol.SessionID, previous *ConversationState) (ConversationState, bool) {
	c.conversationMu.Lock()
	defer c.conversationMu.Unlock()
	w := c.conversations[session]
	if w == nil {
		return ConversationState{}, false
	}
	return w.readState(previous), true
}

// readState runs under conversationMu, allowing observers to capture their
// delivery cursors and read-only view from one projection state.
func (w *conversationWatch) readState(previous *ConversationState) ConversationState {
	s := w.state
	s.StatisticsErr = w.projection.statisticsErr
	snapshot := w.projection.snapshot
	if snapshot == nil {
		return s
	}
	if previous != nil {
		if w.projection.entryEpoch == nil {
			w.projection.entryEpoch = new(byte)
		}
		s.entryEpoch, s.viewVersion, s.transcript = w.projection.entryEpoch, w.viewVersion, w.projection.transcript
		if s.SameViewSnapshot(*previous) {
			s.Snapshot = previous.Snapshot
			return s
		}
		if s.SameViewTranscript(*previous) {
			s.Snapshot = previous.Snapshot
			s.Snapshot.Sequence = snapshot.Sequence
			s.Snapshot.Statistics = copyValue(snapshot.Statistics)
			return s
		}
		if previous.entryEpoch == s.entryEpoch {
			s.Snapshot = copySnapshotWithEntries(*snapshot, previous.Snapshot.Page.Entries)
			return s
		}
	}
	s.Snapshot = copyValue(*snapshot)
	return s
}

// SameViewSnapshot reports whether two read-only views share the complete
// detached snapshot, independently of watch status such as LoadingOlder.
func (s ConversationState) SameViewSnapshot(other ConversationState) bool {
	return s.entryEpoch != nil && s.entryEpoch == other.entryEpoch && s.viewVersion == other.viewVersion
}

// SameViewTranscript reports whether read-only views differ only in the
// stream sequence, statistics or watch state, not their transcript data.
func (s ConversationState) SameViewTranscript(other ConversationState) bool {
	return s.entryEpoch != nil && s.entryEpoch == other.entryEpoch && s.transcript == other.transcript
}

// Within an entry epoch, streaming and pagination only insert records in
// canonical sequence order. Mutable snapshot fields still need detached copies.
func copySnapshotWithEntries(snapshot protocol.ConversationSnapshot, previous []protocol.ConversationEntry) protocol.ConversationSnapshot {
	entries := snapshot.Page.Entries
	snapshot.Page.Entries = nil
	result := copyValue(snapshot)
	if len(entries) == 0 {
		result.Page.Entries = slices.Clone(entries)
		return result
	}
	if len(previous) == len(entries) {
		result.Page.Entries = previous
		return result
	}
	result.Page.Entries = make([]protocol.ConversationEntry, len(entries))
	for i, entry := range entries {
		if len(previous) > 0 && previous[0].Sequence == entry.Sequence {
			result.Page.Entries[i], previous = previous[0], previous[1:]
		} else {
			result.Page.Entries[i] = copyValue(entry)
		}
	}
	return result
}

// WatchConversation shares a single subscription and reduction owner per session.
func (c *Client) WatchConversation(ctx context.Context, session protocol.SessionID, changed func()) (func(), error) {
	return c.watchConversation(ctx, session, changed, true)
}

// WatchRecentConversation starts at the newest page when reopening without a
// saved reading position. It does not change the range of a shared active watch
// or interfere with pagination and reconnects after opening.
func (c *Client) WatchRecentConversation(ctx context.Context, session protocol.SessionID, changed func()) (func(), error) {
	return c.watchConversation(ctx, session, changed, false)
}

func (c *Client) watchConversation(ctx context.Context, session protocol.SessionID, changed func(), restoreRange bool) (func(), error) {
	c.lifecycle.Lock()
	defer c.lifecycle.Unlock()
	if c.closed {
		return nil, fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	c.conversationMu.Lock()
	if c.conversations == nil {
		c.conversations = map[protocol.SessionID]*conversationWatch{}
	}
	w := c.conversations[session]
	if w == nil || w.ctx.Err() != nil {
		parent := c.context
		if parent == nil {
			parent = ctx
		}
		owner, cancel := context.WithCancel(parent)
		next := &conversationWatch{session: session, projection: NewConversationProjection(c.Target(), session, 0), ctx: owner, cancel: cancel, done: make(chan struct{}), refresh: make(chan struct{}, 1), signal: make(chan struct{}), callbacks: map[uint64]func(){}, state: ConversationState{Loading: true}}
		if w != nil {
			if restoreRange {
				next.extent = w.extent
			}
			w.discardHistory()
		}
		// A bookmark describes how far to page after authorization; it is not
		// a display snapshot that can be delivered by a new subscription.
		bookmarks := c.conversationBookmarks
		if at := slices.IndexFunc(bookmarks, func(b conversationBookmark) bool { return b.session == session }); at >= 0 {
			if restoreRange {
				next.extent = bookmarks[at].extent
			}
			c.conversationBookmarks = slices.Delete(bookmarks, at, at+1)
		}
		next.known = c.sessionKnown(session)
		c.conversations[session] = next
		if w != nil {
			c.pruneConversationDetails()
		}
		w = next
		c.conversationOwners.Add(1)
		go c.observeConversation(w)
	}
	w.next++
	id := w.next
	w.callbacks[id] = changed
	c.conversationMu.Unlock()
	var once sync.Once
	release := func() {
		once.Do(func() {
			c.conversationMu.Lock()
			delete(w.callbacks, id)
			if len(w.callbacks) == 0 {
				w.cancel()
				if c.conversations[session] == w {
					if w.extent != (conversationExtent{}) {
						if len(c.conversationBookmarks) == conversationBookmarkLimit {
							c.conversationBookmarks = slices.Delete(c.conversationBookmarks, 0, 1)
						}
						c.conversationBookmarks = append(c.conversationBookmarks, conversationBookmark{session, w.extent})
					}
					delete(c.conversations, session)
					w.discardHistory()
					c.pruneConversationDetails()
				}
			}
			c.conversationMu.Unlock()
		})
	}
	stop := context.AfterFunc(ctx, release)
	return func() { stop(); release() }, nil
}

func (c *Client) RefreshConversation(ctx context.Context, session protocol.SessionID) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	c.conversationMu.Lock()
	w := c.conversations[session]
	c.conversationMu.Unlock()
	if w == nil || w.ctx.Err() != nil {
		return fault(protocol.ErrorCodeUnavailable, "conversation observation is unavailable")
	}
	select {
	case w.refresh <- struct{}{}:
	default:
	}
	return nil
}

func (c *Client) conversationChanged(w *conversationWatch) {
	c.conversationMu.Lock()
	if c.conversations[w.session] != w {
		c.conversationMu.Unlock()
		return
	}
	callbacks := make([]func(), 0, len(w.callbacks))
	for _, f := range w.callbacks {
		if f != nil {
			callbacks = append(callbacks, f)
		}
	}
	close(w.signal)
	w.signal = make(chan struct{})
	c.conversationMu.Unlock()
	for _, f := range callbacks {
		f()
	}
}

func (c *Client) closeConversations() {
	c.conversationMu.Lock()
	var owners []*conversationWatch
	for _, w := range c.conversations {
		w.cancel()
		w.discardHistory()
		owners = append(owners, w)
	}
	c.conversations, c.conversationBookmarks = nil, nil
	c.pruneConversationDetails()
	c.conversationMu.Unlock()
	for _, w := range owners {
		<-w.done
	}
	c.conversationOwners.Wait()
}

// A resource read miss is not removal; the authoritative Node snapshot confirms it.
func (c *Client) reconcileConversations() {
	c.conversationMu.Lock()
	var removed []*conversationWatch
	for id, w := range c.conversations {
		if c.sessionKnown(id) {
			w.known = true
		} else if w.known && !w.state.Removed {
			w.discardHistory()
			w.state.Removed = true
			w.state.Connected = false
			w.state.Loading = false
			w.state.Err = fault(protocol.ErrorCodeNotFound, "conversation was removed")
			w.cancel()
			if len(w.callbacks) == 0 {
				delete(c.conversations, id)
				continue
			}
			removed = append(removed, w)
		}
	}
	c.pruneConversationDetails()
	c.conversationMu.Unlock()
	for _, w := range removed {
		c.conversationChanged(w)
	}
}

type conversationIncoming struct {
	update protocol.Update
	err    error
}

func (c *Client) observeConversation(w *conversationWatch) {
	defer c.conversationOwners.Done()
	defer close(w.done)
	defer func() {
		c.conversationMu.Lock()
		w.state.Connected = false
		w.state.Loading = false
		w.state.LoadingOlder = false
		c.conversationMu.Unlock()
		c.conversationChanged(w)
	}()
	delay := 250 * time.Millisecond
	var outage time.Time
	for w.ctx.Err() == nil {
		c.conversationMu.Lock()
		w.projection.generation++
		generation := w.projection.generation
		w.projection.recovering = true
		desired := w.extent
		c.conversationMu.Unlock()
		wake := c.retried()
		sub, err := c.transport.Subscribe(w.ctx, protocol.Topic{Kind: "conversation", Data: w.session})
		if err == nil {
			err = c.consumeConversation(w, sub, generation, desired)
			if err == nil {
				delay = 250 * time.Millisecond
				outage = time.Time{}
				continue
			}
		}
		if w.ctx.Err() != nil {
			return
		}
		if !retryConversation(err) {
			c.conversationMu.Lock()
			switch faultCode(err) {
			case protocol.ErrorCodePermissionDenied, protocol.ErrorCodeNotFound:
				w.discardHistory()
				c.pruneConversationDetails()
			}
			w.state.Err = err
			w.state.Connected = false
			w.state.Loading = false
			c.conversationMu.Unlock()
			c.conversationChanged(w)
			select {
			case <-w.ctx.Done():
				return
			case <-w.refresh:
				continue
			}
		}
		if outage.IsZero() {
			outage = time.Now()
		}
		if time.Since(outage) >= 5*time.Second {
			c.conversationMu.Lock()
			w.state.Err = err
			w.state.Connected = faultCode(err) == protocol.ErrorCodeBusy && w.state.Connected
			c.conversationMu.Unlock()
			c.conversationChanged(w)
		}
		timer := time.NewTimer(delay)
		select {
		case <-w.ctx.Done():
			timer.Stop()
			return
		case <-w.refresh:
			timer.Stop()
		case <-wake:
			timer.Stop()
			delay = 250 * time.Millisecond
			continue
		case <-timer.C:
		}
		delay = min(delay*2, 5*time.Second)
	}
}

func retryConversation(err error) bool {
	return transient(err) || errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) || errors.Is(err, context.Canceled)
}

func (c *Client) consumeConversation(w *conversationWatch, sub link.Subscription, generation uint64, desired conversationExtent) error {
	ctx, cancel := context.WithCancel(w.ctx)
	stream := make(chan conversationIncoming, 1)
	joined := make(chan struct{})
	go func() {
		defer close(joined)
		for {
			update, err := sub.Next(ctx)
			select {
			case stream <- conversationIncoming{update, err}:
			case <-ctx.Done():
				return
			}
			if err != nil {
				return
			}
		}
	}()
	var restoreDone chan struct{}
	defer func() {
		cancel()
		sub.Close()
		<-joined
		if restoreDone != nil {
			<-restoreDone
		}
	}()
	restored := false
	for {
		select {
		case <-w.ctx.Done():
			return w.ctx.Err()
		case <-w.refresh:
			return nil
		case incoming := <-stream:
			if incoming.err != nil {
				return incoming.err
			}
			applied, err := c.applyConversationUpdate(w, generation, incoming.update)
			if err != nil {
				return err
			}
			if applied == Recover {
				return nil
			}
			if applied == Ignored {
				continue
			}
			c.conversationChanged(w)
			if !restored {
				restored = true
				restoreDone = make(chan struct{})
				go func() { defer close(restoreDone); c.restoreConversation(ctx, w, generation, desired) }()
			}
		}
	}
}

func (c *Client) applyConversationUpdate(w *conversationWatch, generation uint64, update protocol.Update) (Apply, error) {
	c.conversationMu.Lock()
	defer c.conversationMu.Unlock()
	if c.conversations[w.session] != w || w.state.Removed || w.ctx.Err() != nil {
		return Ignored, context.Canceled
	}
	applied, err := w.projection.Apply(generation, update)
	if err == nil && applied == Applied {
		w.viewVersion++
		if changesConversationInventory(update) && !w.projection.toolActivityHidden {
			c.conversationInventory++
			w.state.InventoryVersion = c.conversationInventory
		}
		w.state.Loading = false
		w.state.Connected = true
		w.state.Err = nil
		w.rememberExtent()
		c.pruneConversationDetails()
	}
	return applied, err
}

// The inventory reads canonical records, including hidden ones outside the
// fitted page. Only known non-inventory frames can reuse a previous read.
func changesConversationInventory(update protocol.Update) bool {
	if update.Kind == "conversation_frame" {
		frame, _ := payload[protocol.ConversationFrame](update.Data) // already accepted by Apply
		switch frame.Change.Kind {
		case "delta", "statistics", "statistics_failed", "run", "queue", "approval", "question", "child", "background":
			return false
		}
	}
	return true
}

func (c *Client) restoreConversation(ctx context.Context, w *conversationWatch, generation uint64, desired conversationExtent) {
	turnLimit := turnHistoryPageLimit
	if desired.all {
		// The Node also bounds each page by bytes. Leave interactive reads
		// and recovery to a partial anchor at the smaller entry limit.
		turnLimit = fullHistoryRestoreLimit
	}
	for ctx.Err() == nil {
		c.conversationMu.Lock()
		valid := c.conversations[w.session] == w && w.projection.generation == generation
		needed := valid && w.projection.snapshot != nil && desired.needsMore(*w.projection.snapshot)
		loading, signal := w.state.LoadingOlder, w.signal
		c.conversationMu.Unlock()
		if !needed {
			return
		}
		if loading {
			select {
			case <-ctx.Done():
				return
			case <-signal:
				continue
			}
		}
		if err := c.loadOlderConversation(ctx, w.session, turnLimit, false); err != nil {
			return
		}
	}
}
