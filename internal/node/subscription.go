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
	user    protocol.User
	node    *Node
	wake    chan struct{}
	closed  chan struct{}
	close   sync.Once
	next    sync.Mutex
	initial *protocol.Snapshot
	cursor  uint64
	through uint64
}

func (node *Node) subscribe(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "status" {
		return node.subscribePluginStatus(ctx, caller, topic)
	}
	if topic.Kind == "terminal" {
		return node.subscribeTerminal(topic)
	}
	if topic.Kind == "provider_login" || topic.Kind == "connector_login" || topic.Kind == "connections" {
		return node.subscribeAuth(ctx, caller, topic)
	}
	if topic.Kind == "installations" {
		return node.plugins.installations.Subscribe(caller), nil
	}
	if topic.Kind == "scoped_installations" {
		return node.subscribeScopedInstallations(ctx, caller, topic)
	}
	if topic.Kind == "browser" {
		return node.browsers.subscribe(caller)
	}
	if topic.Kind == "python" {
		return &pythonEvents{node: node, initial: true, closed: make(chan struct{})}, nil
	}
	if topic.Kind == "files" || topic.Kind == "repository" || topic.Kind == "commands" {
		return node.subscribeWorkspace(ctx, topic)
	}
	if topic.Kind == "conversation" {
		switch session := topic.Data.(type) {
		case protocol.SessionID:
			return node.subscribeConversation(ctx, session)
		case *protocol.SessionID:
			return node.subscribeConversation(ctx, *session)
		}
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "conversation session is required"}
	}

	if topic.Kind != "node" {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "subscription is unavailable"}
	}
	return node.subscribeNodeEvents(ctx, true)
}

func (node *Node) subscribeNodeEvents(ctx context.Context, withSnapshot bool) (link.Subscription, error) {
	value := &events{user: userFromContext(ctx), node: node, wake: make(chan struct{}, 1), closed: make(chan struct{})}
	node.mu.Lock()
	if node.stopped {
		node.mu.Unlock()
		return nil, unavailable()
	}
	// Register before pinning the starting read so every concurrent commit is observed.
	node.subscribersMu.Lock()
	node.subscribers[value] = struct{}{}
	node.subscribersMu.Unlock()
	node.mu.Unlock()
	if err := value.pinStart(ctx, withSnapshot); err != nil {
		_ = value.Close()
		return nil, err
	}
	return value, nil
}

func (stream *events) pinStart(ctx context.Context, withSnapshot bool) error {
	if withSnapshot {
		snapshot, err := stream.node.Snapshot(ctx)
		if err != nil {
			return err
		}
		stream.initial = &snapshot
		stream.cursor = snapshot.Cursor
		return nil
	}
	if err := stream.node.begin(); err != nil {
		return err
	}
	defer stream.node.work.Done()
	return nodeFault(stream.node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		stream.cursor, err = store.EventCursor(ctx, db)
		return err
	}))
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
	ctx = store.WithUser(ctx, stream.user)
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
		if err := stream.node.begin(); err != nil {
			return protocol.Update{}, err
		}
		through := stream.through
		var event *protocol.EventEnvelope
		err := stream.node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			through, event, err = store.NextEvent(ctx, db, stream.node.ID(), stream.cursor, through)
			return err
		})
		stream.node.work.Done()
		if err != nil {
			if ctx.Err() != nil {
				return protocol.Update{}, ctx.Err()
			}
			stream.cursor = max(stream.cursor, through)
			stream.through = 0
			return protocol.Update{Kind: "reset_required"}, nil
		}
		stream.through = through
		if event != nil {
			stream.cursor = event.Cursor
			if event.Event.Kind == "projection_invalidated" {
				stream.through = 0
				return protocol.Update{Kind: "reset_required"}, nil
			}
			return protocol.Update{Kind: "event", Data: *event}, nil
		}
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
