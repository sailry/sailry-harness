package client

import (
	"context"
	"errors"
	"io"
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
}

type conversationWatch struct {
	session         protocol.SessionID
	projection      *ConversationProjection
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

func (c *Client) Conversation(session protocol.SessionID) (ConversationState, bool) {
	c.conversationMu.Lock()
	defer c.conversationMu.Unlock()
	w := c.conversations[session]
	if w == nil {
		return ConversationState{}, false
	}
	s := w.state
	s.StatisticsErr = w.projection.statisticsErr
	if w.projection.snapshot != nil {
		s.Snapshot = copyValue(*w.projection.snapshot)
	}
	return s, true
}

// WatchConversation shares a single subscription and reduction owner per session.
func (c *Client) WatchConversation(ctx context.Context, session protocol.SessionID, changed func()) (func(), error) {
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
			next.state = w.state
			next.projection.snapshot = w.projection.Snapshot()
			next.projection.liveStart = w.projection.liveStart
			next.projection.artifactCursor = w.projection.artifactCursor
			next.projection.statisticsErr = w.projection.statisticsErr
			next.extent = w.extent
		}
		next.known = c.sessionKnown(session)
		w = next
		c.conversations[session] = w
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
				// A live session keeps its state for the next observer; a
				// removed one has none to come.
				if w.state.Removed && c.conversations[session] == w {
					delete(c.conversations, session)
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
		owners = append(owners, w)
	}
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
			w.state.Removed = true
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
		if faultCode(err) == protocol.ErrorCodeNotFound {
			c.conversationMu.Lock()
			w.state.Err = err
			w.state.Loading = false
			w.state.Connected = false
			c.conversationMu.Unlock()
			c.conversationChanged(w)
			// Only Node removal prunes this owner. Explicit reconnect can retry.
			select {
			case <-w.ctx.Done():
				return
			case <-w.refresh:
				continue
			}
		}
		if !retryConversation(err) {
			c.conversationMu.Lock()
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
			c.conversationMu.Lock()
			if c.conversations[w.session] != w {
				c.conversationMu.Unlock()
				return context.Canceled
			}
			applied, err := w.projection.Apply(generation, incoming.update)
			if err == nil && applied == Applied {
				w.state.Loading = false
				w.state.Connected = true
				w.state.Err = nil
				if !desired.needsMore(*w.projection.snapshot) {
					w.extent = conversationExtentOf(*w.projection.snapshot)
				}
				c.pruneConversationDetails()
			}
			c.conversationMu.Unlock()
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

func (c *Client) restoreConversation(ctx context.Context, w *conversationWatch, generation uint64, desired conversationExtent) {
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
		if err := c.LoadOlderConversation(ctx, w.session); err != nil {
			return
		}
	}
}
