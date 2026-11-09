package node

import (
	"context"
	"database/sql"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type events struct {
	node    *Node
	wake    chan struct{}
	closed  chan struct{}
	close   sync.Once
	next    sync.Mutex
	initial *protocol.Snapshot
	cursor  uint64
	through uint64
	pending []protocol.EventEnvelope
}

func (node *Node) Subscribe(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "node" {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "subscription is unavailable"}
	}
	value := &events{node: node, wake: make(chan struct{}, 1), closed: make(chan struct{})}
	node.mu.Lock()
	if node.stopped {
		node.mu.Unlock()
		return nil, unavailable()
	}
	// Register before the pinned snapshot so every concurrent commit is observed.
	node.subscribersMu.Lock()
	node.subscribers[value] = struct{}{}
	node.subscribersMu.Unlock()
	node.mu.Unlock()
	snapshot, err := node.Snapshot(ctx)
	if err != nil {
		_ = value.Close()
		return nil, err
	}
	value.initial = &snapshot
	value.cursor = snapshot.Cursor
	return value, nil
}

func (stream *events) Close() error {
	stream.close.Do(func() {
		close(stream.closed)
		stream.node.subscribersMu.Lock()
		delete(stream.node.subscribers, stream)
		stream.node.subscribersMu.Unlock()
	})
	return nil
}

func (stream *events) Next(ctx context.Context) (protocol.Update, error) {
	stream.next.Lock()
	defer stream.next.Unlock()
	select {
	case <-stream.closed:
		return protocol.Update{}, unavailable()
	case <-stream.node.closed:
		return protocol.Update{}, unavailable()
	default:
	}
	if stream.initial != nil {
		value := *stream.initial
		stream.initial = nil
		return protocol.Update{Kind: "snapshot", Data: value}, nil
	}
	for {
		if len(stream.pending) > 0 {
			event := stream.pending[0]
			stream.pending = stream.pending[1:]
			if event.Cursor != stream.cursor+1 {
				stream.pending = nil
				stream.cursor = stream.through
				return protocol.Update{Kind: "reset_required"}, nil
			}
			stream.cursor = event.Cursor
			return protocol.Update{Kind: "event", Data: event}, nil
		}
		if err := stream.node.begin(); err != nil {
			return protocol.Update{}, err
		}
		var through uint64
		var pending []protocol.EventEnvelope
		err := stream.node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			through, pending, err = store.EventPage(ctx, db, stream.node.ID(), stream.cursor, stream.through)
			return err
		})
		stream.node.work.Done()
		if err != nil {
			if ctx.Err() != nil {
				return protocol.Update{}, ctx.Err()
			}
			stream.pending = nil
			stream.cursor = stream.through
			stream.through = 0
			return protocol.Update{Kind: "reset_required"}, nil
		}
		stream.through = through
		stream.pending = pending
		if len(pending) > 0 {
			continue
		}
		if stream.cursor < through {
			stream.cursor = through
			stream.through = 0
			return protocol.Update{Kind: "reset_required"}, nil
		}
		stream.through = 0
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-stream.closed:
			return protocol.Update{}, unavailable()
		case <-stream.node.closed:
			return protocol.Update{}, unavailable()
		case <-stream.wake:
		}
	}
}
