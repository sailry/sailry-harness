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

// refreshInitial checks the pinned scope at delivery. A normal write leaves
// the original snapshot and its first unread delta alone. A lost project
// requires reprojection, including its descendants, at a new cursor.
func (stream *events) refreshInitial(ctx context.Context, caller protocol.NodeID, snapshot protocol.Snapshot) (protocol.Snapshot, error) {
	stream.next.Lock()
	defer stream.next.Unlock()
	node := stream.node
	if err := node.begin(); err != nil {
		return protocol.Snapshot{}, err
	}
	defer node.work.Done()
	ctx = store.WithUser(ctx, stream.user)
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		verified, err := node.authorizeTopicDB(ctx, db, caller, protocol.Topic{Kind: "node"}, true)
		if err != nil {
			return err
		}
		if snapshot.User != nil && snapshot.User.ID == userFromContext(verified).ID {
			visible, err := snapshotProjectsVisible(verified, db, snapshot.Projects)
			if err != nil || visible {
				return err
			}
		}
		verified = store.WithNativeSkills(node.pluginContext(verified), node.modules.NativeSkills)
		current, err := store.Snapshot(verified, db, node.ID())
		if err != nil {
			return err
		}
		current.ProjectSources, current.HostTools = snapshot.ProjectSources, snapshot.HostTools
		snapshot = current
		return nil
	})
	if err != nil {
		return protocol.Snapshot{}, nodeFault(err)
	}
	stream.user, stream.cursor = *snapshot.User, snapshot.Cursor
	return snapshot, nil
}

func snapshotProjectsVisible(ctx context.Context, db *sql.Conn, cached []protocol.Project) (bool, error) {
	if len(cached) == 0 {
		return true, nil
	}
	current, err := store.ReadProjects(ctx, db)
	if err != nil {
		return false, err
	}
	visible := make(map[protocol.ProjectID]struct{}, len(current))
	for _, project := range current {
		visible[project.ID] = struct{}{}
	}
	for _, project := range cached {
		if _, ok := visible[project.ID]; !ok {
			return false, nil
		}
	}
	return true, nil
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
	ctx = store.WithUser(ctx, stream.user)
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
