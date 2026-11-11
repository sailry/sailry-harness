package node

import (
	"context"
	"database/sql"
	"slices"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) prepareConnectorCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, bool, error) {
	switch input := request.Command.Data.(type) {
	case *protocol.CommandStartConnectorLoginPayload:
		output, err := node.beginConnectorLogin(ctx, caller, request.ID, *input)
		return output, true, err
	case *protocol.CommandSubmitConnectorInputPayload:
		output, err := node.connectorLoginInput(caller, request.ID, input.Attempt, &input.Input, false)
		return output, true, err
	case *protocol.CommandCancelConnectorLoginPayload:
		output, err := node.connectorLoginInput(caller, request.ID, input.Attempt, nil, true)
		return output, true, err
	case *protocol.CommandInspectConnectorCliPayload:
		var declaration protocol.PluginConnectorDeclaration
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			_, declaration, err = store.ConnectorDeclaration(ctx, db, input.Package, input.Connector)
			return err
		})
		var identity protocol.PluginConnectorIdentity
		if err == nil {
			identity, err = node.connectorCLIIdentity(ctx, declaration)
		}
		return protocol.Output{Kind: "connector_identity", Data: identity}, true, err
	case *protocol.CommandProbeConnectionPayload:
		value, err := node.probeConnector(ctx, input.Connection)
		return protocol.Output{Kind: "connection", Data: value}, true, err
	case *protocol.CommandConfirmConnectorConsentPayload:
		var capture store.ConnectorLoginCapture
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			capture, err = store.PrepareConnectorConsent(ctx, db, input.Connection, input.ExpectedRevision)
			return err
		})
		if err == nil {
			err = node.captureConnectorTargets(ctx, &capture)
		}
		if err == nil {
			node.auth.mu.Lock()
			node.auth.consents[request.ID] = capture
			node.auth.mu.Unlock()
		}
		return protocol.Output{}, true, err
	}
	// A package sees only its own connector types and connections.
	owned := func(name string) bool { return request.Plugin == nil || request.Plugin.Package.Name == name }
	switch request.Command.Kind {
	case "list_connector_types":
		var values []protocol.PluginConnectorType
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			values, err = store.ConnectorTypes(ctx, db)
			return err
		})
		values = slices.DeleteFunc(values, func(value protocol.PluginConnectorType) bool { return !owned(value.Package.Name) })
		return protocol.Output{Kind: "connector_types", Data: values}, true, err
	case "list_connections":
		values, err := node.connectorConnections(ctx)
		values = slices.DeleteFunc(values, func(value protocol.PluginConnectorConnection) bool { return !owned(value.Package.Name) })
		return protocol.Output{Kind: "connections", Data: values}, true, err
	}
	return protocol.Output{}, false, nil
}

// connectorMutation reports the connector commands whose change
// mutateAuthCommand writes in the command's transaction.
func connectorMutation(kind string) bool {
	switch kind {
	case "start_connector_login", "update_connection", "set_connection_enabled", "disconnect_connection", "remove_connection", "confirm_connector_consent":
		return true
	}
	return false
}

func (node *Node) mutateAuthCommand(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, bool, error) {
	if !connectorMutation(request.Command.Kind) {
		return protocol.Output{}, nil, false, nil
	}
	if request.Command.Kind == "start_connector_login" {
		node.auth.mu.Lock()
		entry, err := node.authEntry(caller, request.ID, "connector_login")
		var capture *store.ConnectorLoginCapture
		if entry != nil {
			capture = entry.connection
		}
		node.auth.mu.Unlock()
		if err != nil {
			return protocol.Output{}, nil, true, err
		}
		err = store.StartConnectorLogin(ctx, db, *capture)
		return protocol.Output{Kind: "connector_login", Data: capture.Attempt}, nil, true, err
	}
	if request.Plugin != nil {
		return protocol.Output{}, nil, true, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "connection editing requires a controller"}
	}
	node.auth.mu.Lock()
	consent, exists := node.auth.consents[request.ID]
	node.auth.mu.Unlock()
	var capture *store.ConnectorLoginCapture
	if exists {
		capture = &consent
	}
	output, err := store.ExecuteConnectorCommand(ctx, db, request.Command, capture)
	return output, nil, true, err
}

func (node *Node) beginConnectorLogin(ctx context.Context, caller protocol.NodeID, attempt protocol.RequestID, input protocol.CommandStartConnectorLoginPayload) (protocol.Output, error) {
	var capture store.ConnectorLoginCapture
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		capture, err = store.PrepareConnectorLogin(ctx, db, attempt, input)
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	if err := node.captureConnectorTargets(ctx, &capture); err != nil {
		return protocol.Output{}, err
	}
	entry := &authEntry{owner: caller, kind: "connector_login", revision: 1, connection: &capture, callback: make(chan string, 1),
		connector: protocol.PluginConnectorLoginUpdate{Attempt: capture.Attempt, Revision: 1, State: protocol.PluginConnectorLoginState{Kind: "starting"}}}
	returned := entry.callback
	if err := node.addAuthEntry(attempt, entry); err != nil {
		return protocol.Output{}, err
	}
	node.auth.mu.Lock()
	node.auth.effects[attempt] = func(accepted bool) {
		if !accepted {
			_ = node.setConnectorLogin(entry, protocol.PluginConnectorLoginState{Kind: "cancelled"})
			return
		}
		err := node.tasks.Submit(func(owner context.Context) {
			operation, cancel := context.WithCancel(owner)
			defer cancel()
			node.auth.mu.Lock()
			entry.cancel = cancel
			stopped := entry.stopped || node.auth.closed
			node.auth.mu.Unlock()
			if stopped {
				cancel()
			}
			grant, identity, err := node.runConnectorLogin(operation, &capture, returned, func(state protocol.ConversationLoginState) error {
				return node.connectorProgress(entry, state)
			})
			recording := context.WithoutCancel(node.ctx)
			var value protocol.PluginConnectorConnection
			saveErr := node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
				var writeErr error
				value, writeErr = store.CompleteConnectorLogin(recording, db, capture, grant, identity, err != nil)
				return writeErr
			})
			if err == nil {
				err = saveErr
			}
			node.authChanged()
			if err != nil {
				state := protocol.PluginConnectorLoginState{Kind: "failed", Data: pluginFailure(err)}
				if operation.Err() != nil {
					state = protocol.PluginConnectorLoginState{Kind: "cancelled"}
				}
				_ = node.setConnectorLogin(entry, state)
			} else {
				_ = node.setConnectorLogin(entry, protocol.PluginConnectorLoginState{Kind: "connected", Data: value})
			}
		})
		if err != nil {
			_ = node.setConnectorLogin(entry, protocol.PluginConnectorLoginState{Kind: "failed", Data: pluginFailure(err)})
		}
	}
	node.auth.mu.Unlock()
	return protocol.Output{Kind: "connector_login", Data: capture.Attempt}, nil
}

func (node *Node) setConnectorLogin(entry *authEntry, state protocol.PluginConnectorLoginState) error {
	node.auth.mu.Lock()
	defer node.auth.mu.Unlock()
	if node.auth.closed || entry.finished {
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "authorization attempt has finished"}
	}
	entry.revision++
	entry.connector.State, entry.connector.Revision = state, entry.revision
	entry.finished = state.Kind == "connected" || state.Kind == "failed" || state.Kind == "cancelled"
	node.auth.notify()
	return nil
}

func (node *Node) connectorProgress(entry *authEntry, state protocol.ConversationLoginState) error {
	value := protocol.PluginConnectorLoginState{Kind: state.Kind, Data: state.Data}
	if state.Kind == "pending" {
		pending, ok := dispatchPayload[protocol.ConversationLoginStatePendingPayload](state.Data)
		if !ok {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "connector device authorization is invalid"}
		}
		value.Kind, value.Data = "device", protocol.PluginConnectorLoginStateDevicePayload{Code: pending.UserCode, Url: pending.VerificationUrl, ExpiresAtMs: pending.ExpiresAtMs}
	} else if state.Kind == "browser" {
		browser, ok := dispatchPayload[protocol.ConversationLoginStateBrowserPayload](state.Data)
		if !ok {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "connector browser authorization is invalid"}
		}
		value.Data = protocol.PluginConnectorLoginStateBrowserPayload{Url: browser.AuthorizationUrl, ExpiresAtMs: browser.ExpiresAtMs}
	}
	return node.setConnectorLogin(entry, value)
}

func (node *Node) connectorLoginInput(caller protocol.NodeID, request, attempt protocol.RequestID, input *protocol.Secret, cancel bool) (protocol.Output, error) {
	node.auth.mu.Lock()
	defer node.auth.mu.Unlock()
	entry, err := node.authEntry(caller, attempt, "connector_login")
	if err != nil {
		return protocol.Output{}, err
	}
	if entry.finished || (!cancel && entry.callback == nil) {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "connector authorization is not awaiting this action"}
	}
	if input != nil && (len(input.Expose()) > 16*1024 || strings.ContainsFunc(input.Expose(), unicode.IsControl)) {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "connector authorization callback is invalid"}
	}
	node.auth.effects[request] = func(accepted bool) {
		if !accepted {
			return
		}
		node.auth.mu.Lock()
		if cancel {
			entry.stopped = true
			if entry.cancel != nil {
				entry.cancel()
			}
			node.auth.mu.Unlock()
			_ = node.setConnectorLogin(entry, protocol.PluginConnectorLoginState{Kind: "cancelled"})
			return
		}
		if entry.finished {
			node.auth.mu.Unlock()
			return
		}
		callback := entry.callback
		entry.callback = nil
		node.auth.mu.Unlock()
		if callback != nil {
			callback <- input.Expose()
		}
		_ = node.setConnectorLogin(entry, protocol.PluginConnectorLoginState{Kind: "exchanging"})
	}
	return protocol.Output{Kind: "connector_login", Data: entry.connector.Attempt}, nil
}
