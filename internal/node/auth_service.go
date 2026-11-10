package node

import (
	"context"
	"database/sql"
	"net/http"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type authControls struct {
	mu             sync.Mutex
	entries        map[protocol.RequestID]*authEntry
	effects        map[protocol.RequestID]func(bool)
	renewals       map[protocol.CredentialID]*authRenewal
	changed        chan struct{}
	cursor         uint64
	host           string
	closed         bool
	connectorLocks sync.Map
	cli            map[protocol.PluginConnectorID]connectorCachedCLI
	resources      map[[32]byte]*providerDocument
	consents       map[protocol.RequestID]store.ConnectorLoginCapture
	client         *http.Client
}

type authEntry struct {
	owner      protocol.NodeID
	kind       string
	provider   protocol.ConversationLoginUpdate
	connector  protocol.PluginConnectorLoginUpdate
	revision   uint64
	finished   bool
	stopped    bool
	callback   chan string
	cancel     context.CancelFunc
	capture    *store.ProviderLoginCapture
	connection *store.ConnectorLoginCapture
}

type authRenewal struct {
	done chan struct{}
	err  error
}

func (node *Node) initAuth(ctx context.Context) error {
	control := &authControls{
		entries: map[protocol.RequestID]*authEntry{}, effects: map[protocol.RequestID]func(bool){},
		renewals: map[protocol.CredentialID]*authRenewal{}, changed: make(chan struct{}),
		cli: map[protocol.PluginConnectorID]connectorCachedCLI{}, resources: map[[32]byte]*providerDocument{},
		consents: map[protocol.RequestID]store.ConnectorLoginCapture{},
		client:   authorization.HTTPClient(),
	}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		control.host, err = store.OAuthHost(ctx, db)
		if err == nil {
			err = store.RecoverConnectorLogins(ctx, db)
		}
		return err
	}); err != nil {
		return err
	}
	node.auth = control
	return nil
}

func (node *Node) closeAuth() {
	if node.auth == nil {
		return
	}
	node.auth.mu.Lock()
	defer node.auth.mu.Unlock()
	if node.auth.closed {
		return
	}
	node.auth.closed = true
	for _, entry := range node.auth.entries {
		if entry.cancel != nil {
			entry.cancel()
		}
	}
	close(node.auth.changed)
}

func (control *authControls) notify() {
	close(control.changed)
	control.changed = make(chan struct{})
}

func (node *Node) authChanged() {
	if node.auth == nil {
		return
	}
	node.auth.mu.Lock()
	defer node.auth.mu.Unlock()
	if !node.auth.closed {
		node.auth.cursor++
		node.auth.notify()
	}
}

func (node *Node) authCommitted(_ protocol.NodeID, request protocol.Request, err error) {
	if node.auth == nil {
		return
	}
	node.auth.mu.Lock()
	effect := node.auth.effects[request.ID]
	delete(node.auth.effects, request.ID)
	delete(node.auth.consents, request.ID)
	node.auth.mu.Unlock()
	if effect != nil {
		effect(err == nil)
	}
	if err == nil {
		switch request.Command.Kind {
		case "start_connector_login", "update_connection", "set_connection_enabled", "disconnect_connection", "remove_connection", "confirm_connector_consent":
			node.authChanged()
		}
	}
}

func (node *Node) authEntry(caller protocol.NodeID, attempt protocol.RequestID, kind string) (*authEntry, error) {
	entry := node.auth.entries[attempt]
	if entry == nil || entry.owner != caller || entry.kind != kind {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "authorization attempt does not exist"}
	}
	return entry, nil
}

func (node *Node) addAuthEntry(attempt protocol.RequestID, entry *authEntry) error {
	node.auth.mu.Lock()
	defer node.auth.mu.Unlock()
	if node.auth.closed {
		return unavailable()
	}
	if len(node.auth.entries) >= 32 {
		for id, previous := range node.auth.entries {
			if previous.finished {
				delete(node.auth.entries, id)
			}
		}
	}
	if len(node.auth.entries) >= 32 {
		return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "authorization attempt capacity is exhausted"}
	}
	node.auth.entries[attempt] = entry
	node.auth.notify()
	return nil
}

func (node *Node) subscribeAuth(_ context.Context, caller protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	stream := &authEvents{node: node, topic: topic.Kind, initial: true, closed: make(chan struct{})}
	if topic.Kind != "connections" {
		attempt, ok := dispatchPayload[protocol.RequestID](topic.Data)
		if !ok {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "authorization attempt is required"}
		}
		node.auth.mu.Lock()
		entry, err := node.authEntry(caller, attempt, topic.Kind)
		node.auth.mu.Unlock()
		if err != nil {
			return nil, err
		}
		stream.entry = entry
	}
	return stream, nil
}

type authEvents struct {
	node    *Node
	topic   string
	entry   *authEntry
	initial bool
	cursor  uint64
	closed  chan struct{}
	once    sync.Once
	next    sync.Mutex
}

func (stream *authEvents) Close() error {
	stream.once.Do(func() { close(stream.closed) })
	return nil
}

func (stream *authEvents) Next(ctx context.Context) (protocol.Update, error) {
	stream.next.Lock()
	defer stream.next.Unlock()
	for {
		stream.node.auth.mu.Lock()
		control := stream.node.auth
		if control.closed {
			control.mu.Unlock()
			return protocol.Update{}, unavailable()
		}
		revision, changed := control.cursor, control.changed
		if stream.entry != nil {
			revision = stream.entry.revision
		}
		if stream.initial || revision != stream.cursor {
			stream.initial, stream.cursor = false, revision
			if stream.entry != nil {
				value := protocol.Update{Kind: stream.topic, Data: stream.entry.provider}
				if stream.topic == "connector_login" {
					value.Data = stream.entry.connector
				}
				control.mu.Unlock()
				return value, nil
			}
			control.mu.Unlock()
			var value protocol.PluginConnectorChanges
			err := stream.node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				value.Types, err = store.ConnectorTypes(ctx, db)
				return err
			})
			if err == nil {
				value.Connections, err = stream.node.connectorConnections(ctx)
			}
			value.Cursor = revision
			return protocol.Update{Kind: "connections_changed", Data: value}, err
		}
		control.mu.Unlock()
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-stream.closed:
			return protocol.Update{}, unavailable()
		case <-changed:
		}
	}
}
