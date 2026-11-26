package node

import (
	"context"
	"database/sql"
	"reflect"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var topicPolicies = map[string]accessPolicy{
	"node": accessCollection, "conversation": accessResource, "terminal": accessResource, "files": accessResource, "repository": accessResource, "commands": accessResource,
	"status": accessPersonal, "installations": accessDevice, "scoped_installations": accessPersonal, "browser": accessDevice, "connector_login": accessDevice, "provider_login": accessDevice, "connections": accessPersonal, "python": accessMachine,
}

func (node *Node) authenticatedUser(ctx context.Context, db *sql.Conn, caller protocol.NodeID) (protocol.User, error) {
	captured, _ := ctx.Value(capturedDispatchKey{}).(bool)
	if caller == node.ID() || captured {
		return store.CurrentUser(ctx, db)
	}
	return store.CallerUser(ctx, db, node.ID(), caller)
}

func (node *Node) authenticate(ctx context.Context, caller protocol.NodeID) (context.Context, error) {
	var user protocol.User
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		user, err = node.authenticatedUser(ctx, db, caller)
		return err
	})
	if err != nil {
		return nil, nodeFault(err)
	}
	return store.WithUser(ctx, user), nil
}
func (node *Node) authorizeTopic(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (context.Context, error) {
	return node.authorizeSubscriptionTopic(ctx, caller, topic, false)
}

func (node *Node) authorizeSubscriptionTopic(ctx context.Context, caller protocol.NodeID, topic protocol.Topic, attached bool) (context.Context, error) {
	var verified context.Context
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		verified, err = node.authorizeTopicDB(ctx, db, caller, topic, attached)
		return err
	})
	return verified, nodeFault(err)
}

func (node *Node) authorizeTopicDB(ctx context.Context, db *sql.Conn, caller protocol.NodeID, topic protocol.Topic, attached bool) (context.Context, error) {
	user, err := node.authenticatedUser(ctx, db, caller)
	if err != nil {
		return nil, err
	}
	policy, ok := topicPolicies[topic.Kind]
	if !ok || user.State != "active" || policy == accessMachine && !user.Administrator {
		return nil, permissionDenied()
	}
	verified := store.WithUser(ctx, user)
	if topic.Kind == "terminal" {
		id, err := terminalTopicID(topic)
		if err != nil {
			return nil, err
		}
		return verified, store.CheckTerminalSubscriptionAccess(verified, db, id, attached)
	}
	return verified, authorizeResourceValue(verified, db, reflect.ValueOf(topic.Data))
}

func (node *Node) accessSequence() uint64 {
	if node.profile == nil || node.profile.Store == nil || node.profile.Store.Node == nil {
		return 0
	}
	return node.profile.Store.Node.WriteSequence()
}

type accessObservation struct {
	node        *Node
	wake        chan struct{}
	closed      chan struct{}
	once        sync.Once
	release     func() error
	check       func(context.Context) error
	verifiedSeq atomic.Uint64
}

func (node *Node) observeAccess(check func(context.Context) error, release func() error) *accessObservation {
	value := &accessObservation{node: node, wake: make(chan struct{}, 1), closed: make(chan struct{}), release: release, check: check}
	node.accessMu.Lock()
	if node.accessObservers == nil {
		node.accessObservers = map[*accessObservation]struct{}{}
	}
	node.accessObservers[value] = struct{}{}
	node.accessMu.Unlock()
	return value
}
func (value *accessObservation) verify(ctx context.Context) error {
	seq := value.node.accessSequence()
	if seq != 0 && value.verifiedSeq.Load() == seq {
		return nil
	}
	if err := value.check(ctx); err != nil {
		value.verifiedSeq.Store(0)
		return err
	}
	if seq != 0 {
		value.verifiedSeq.Store(seq)
	}
	return nil
}
func (value *accessObservation) Close() error {
	var err error
	value.once.Do(func() {
		close(value.closed)
		value.node.accessMu.Lock()
		delete(value.node.accessObservers, value)
		value.node.accessMu.Unlock()
		err = value.release()
	})
	return err
}
func (node *Node) publishAccess() {
	node.accessMu.Lock()
	defer node.accessMu.Unlock()
	for observer := range node.accessObservers {
		observer.verifiedSeq.Store(0)
		select {
		case observer.wake <- struct{}{}:
		default:
		}
	}
}

type authorizedSubscription struct {
	*accessObservation
	source link.Subscription
	caller protocol.NodeID
	next   sync.Mutex
}

func (node *Node) Subscribe(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "terminal" {
		// Admission, attachment and rechecks must all use the same terminal ID.
		id, err := terminalTopicID(topic)
		if err != nil {
			return nil, err
		}
		topic.Data = id
	}
	seq := node.accessSequence()
	verified, err := node.authorizeTopic(ctx, caller, topic)
	if err != nil {
		return nil, err
	}
	source, err := node.subscribe(verified, caller, topic)
	if err != nil {
		return nil, err
	}
	tracked := node.trackSubscription(verified, caller, topic, source)
	if seq != 0 {
		if guarded, ok := tracked.(*authorizedSubscription); ok {
			guarded.verifiedSeq.Store(seq)
		}
	}
	return tracked, nil
}

func (node *Node) trackSubscription(verified context.Context, caller protocol.NodeID, topic protocol.Topic, source link.Subscription) link.Subscription {
	user := userFromContext(verified)
	check := func(next context.Context) error {
		_, err := node.authorizeSubscriptionTopic(store.WithUser(next, user), caller, topic, true)
		return err
	}
	return &authorizedSubscription{accessObservation: node.observeAccess(check, source.Close), source: source, caller: caller}
}
func (value *authorizedSubscription) Next(ctx context.Context) (protocol.Update, error) {
	value.next.Lock()
	defer value.next.Unlock()
	if err := value.verify(ctx); err != nil {
		_ = value.Close()
		return protocol.Update{}, err
	}
	pending, cancel := context.WithCancel(ctx)
	defer cancel()
	type result struct {
		update protocol.Update
		err    error
	}
	done := make(chan result, 1)
	go func() { update, err := value.source.Next(pending); done <- result{update, err} }()
	for {
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-value.closed:
			return protocol.Update{}, unavailable()
		case <-value.node.closed:
			_ = value.Close()
			return protocol.Update{}, unavailable()
		case <-value.wake:
			value.verifiedSeq.Store(0)
			if err := value.verify(ctx); err != nil {
				_ = value.Close()
				return protocol.Update{}, err
			}
		case outcome := <-done:
			if stream, ok := value.source.(*events); ok && outcome.err == nil && outcome.update.Kind == "snapshot" {
				snapshot, err := stream.refreshInitial(ctx, value.caller, outcome.update.Data.(protocol.Snapshot))
				if err != nil {
					_ = value.Close()
					return protocol.Update{}, err
				}
				return protocol.Update{Kind: "snapshot", Data: snapshot}, nil
			}
			if err := value.verify(ctx); err != nil {
				_ = value.Close()
				return protocol.Update{}, err
			}
			return outcome.update, outcome.err
		}
	}
}
