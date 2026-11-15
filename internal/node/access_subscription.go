package node

import (
	"context"
	"database/sql"
	"reflect"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var topicPolicies = map[string]accessPolicy{
	"node": accessCollection, "conversation": accessResource, "terminal": accessResource, "files": accessResource, "repository": accessResource, "commands": accessResource,
	"status": accessPersonal, "installations": accessDevice, "scoped_installations": accessPersonal, "browser": accessDevice, "connector_login": accessDevice, "provider_login": accessDevice, "connections": accessPersonal, "python": accessMachine,
}

func (node *Node) authenticate(ctx context.Context, caller protocol.NodeID) (context.Context, error) {
	var user protocol.User
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		if caller == node.ID() {
			user, err = store.CurrentUser(ctx, db)
		} else {
			user, err = store.CallerUser(ctx, db, node.ID(), caller)
		}
		return err
	})
	if err != nil {
		return nil, nodeFault(err)
	}
	return store.WithUser(ctx, user), nil
}
func (node *Node) authorizeTopic(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (context.Context, error) {
	ctx, err := node.authenticate(ctx, caller)
	if err != nil {
		return nil, err
	}
	policy, ok := topicPolicies[topic.Kind]
	if !ok {
		return nil, permissionDenied()
	}
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		if user.State != "active" {
			return permissionDenied()
		}
		if policy == accessMachine && !user.Administrator {
			return permissionDenied()
		}
		return authorizeResourceValue(ctx, db, reflect.ValueOf(topic.Data))
	})
	return ctx, nodeFault(err)
}

type accessObservation struct {
	node    *Node
	wake    chan struct{}
	closed  chan struct{}
	once    sync.Once
	release func() error
	check   func(context.Context) error
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
		select {
		case observer.wake <- struct{}{}:
		default:
		}
	}
}

type authorizedSubscription struct {
	*accessObservation
	source link.Subscription
	next   sync.Mutex
}

func (node *Node) Subscribe(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	verified, err := node.authorizeTopic(ctx, caller, topic)
	if err != nil {
		return nil, err
	}
	source, err := node.subscribe(verified, caller, topic)
	if err != nil {
		return nil, err
	}
	user := userFromContext(verified)
	check := func(next context.Context) error {
		_, err := node.authorizeTopic(store.WithUser(next, user), caller, topic)
		return err
	}
	return &authorizedSubscription{accessObservation: node.observeAccess(check, source.Close), source: source}, nil
}
func (value *authorizedSubscription) Next(ctx context.Context) (protocol.Update, error) {
	value.next.Lock()
	defer value.next.Unlock()
	if err := value.check(ctx); err != nil {
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
			if err := value.check(ctx); err != nil {
				_ = value.Close()
				return protocol.Update{}, err
			}
		case outcome := <-done:
			if err := value.check(ctx); err != nil {
				_ = value.Close()
				return protocol.Update{}, err
			}
			return outcome.update, outcome.err
		}
	}
}
