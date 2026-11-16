package node

import (
	"context"
	"database/sql"
	"errors"
	"maps"
	"net/http"
	"reflect"
	"time"

	sdkauth "github.com/modelcontextprotocol/go-sdk/auth"
	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/oauth2"
)

type connectorCachedCLI struct {
	Auth          protocol.PluginConnectorAuth
	AuthRevision  uint64
	GrantRevision uint64
	Identity      string
	Expires       time.Time
	Token         protocol.Secret
}

func (node *Node) connectorAccess(ctx context.Context, record store.ConnectorRecord, declaration protocol.PluginConnectorDeclaration) (*protocol.Secret, error) {
	var remote bool
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		controller, err := store.CurrentController(ctx, db, node.ID())
		remote = controller != node.ID()
		return err
	})
	if err != nil {
		return nil, err
	}
	if remote {
		var token *protocol.Secret
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			token, err = store.ReadRuntimeConnectorCredential(ctx, db, node.ID(), record)
			return err
		})
		return token, err
	}
	if record.Method == protocol.PluginConnectorKindSecret {
		return nil, nil
	}
	value, _ := node.auth.connectorLocks.LoadOrStore(record.ID, make(chan struct{}, 1))
	lock := value.(chan struct{})
	select {
	case lock <- struct{}{}:
		defer func() { <-lock }()
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	var current store.ConnectorRecord
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		current, err = store.ReadConnectorRecord(ctx, db, record.ID)
		return err
	})
	if err != nil {
		return nil, err
	}
	if current.AuthRevision != record.AuthRevision || !current.Enabled || current.Login != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "connector authorization changed"}
	}
	record = current
	if record.Method == protocol.PluginConnectorKindLocalCli {
		auth, err := connectorAuth(declaration, record.Method)
		if err != nil {
			return nil, err
		}
		node.auth.mu.Lock()
		cached, exists := node.auth.cli[record.ID]
		node.auth.mu.Unlock()
		if exists && record.Identity != nil && cached.AuthRevision == record.AuthRevision && cached.GrantRevision == record.GrantRevision && reflect.DeepEqual(cached.Auth, auth) && cached.Identity == record.Identity.ID && time.Now().Before(cached.Expires) {
			value := cached.Token
			return &value, nil
		}
		token, err := node.connectorCLIToken(ctx, declaration, record.Identity)
		if err != nil {
			return nil, err
		}
		definition, _ := dispatchPayload[protocol.PluginConnectorAuthLocalCliPayload](auth.Data)
		node.auth.mu.Lock()
		node.auth.cli[record.ID] = connectorCachedCLI{Auth: auth, AuthRevision: record.AuthRevision, GrantRevision: record.GrantRevision, Identity: record.Identity.ID, Expires: time.Now().Add(time.Duration(definition.TimeoutMs) * time.Millisecond), Token: token}
		node.auth.mu.Unlock()
		return &token, nil
	}
	if record.Grant == nil {
		return nil, connectorUnavailable()
	}
	if record.Grant.Extra["renewal_pending"] == "true" {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "connector renewal is uncertain; connect again"}
	}
	due := store.ConnectorRenewalAt(record)
	if due == nil && record.Grant.ExpiresAtMs != nil && *record.Grant.ExpiresAtMs <= uint64(time.Now().UnixMilli()) {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "connector authorization cannot be refreshed"}
	}
	if due == nil || *due > uint64(time.Now().UnixMilli()) {
		token := protocol.NewSecret(record.Grant.Access)
		return &token, nil
	}
	if record.Method == protocol.PluginConnectorKindMcpOauth {
		source, err := node.connectorMCPSource(record, ctx)
		if err != nil {
			return nil, err
		}
		token, err := source.Token()
		if err != nil {
			return nil, err
		}
		value := protocol.NewSecret(token.AccessToken)
		return &value, nil
	}
	capture := store.ConnectorLoginCapture{Record: record, Declaration: declaration}
	declarationAuth, fields, err := connectorModelAuth(capture, false)
	if err != nil {
		return nil, err
	}
	if record.Method == protocol.PluginConnectorKindOauth2 && (declarationAuth.Refresh == nil || record.Grant.Refresh == nil) {
		// No exchange is sent, so the grant keeps no uncertain marker.
		return nil, &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "connector authorization cannot be refreshed"}
	}
	previous := record
	pending := *record.Grant
	pending.Extra = maps.Clone(pending.Extra)
	if pending.Extra == nil {
		pending.Extra = map[string]string{}
	}
	pending.Extra["renewal_pending"] = "true"
	owner := context.WithoutCancel(ctx)
	err = node.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
		var err error
		record, err = store.SaveConnectorGrant(owner, db, record, pending)
		return err
	})
	if err != nil {
		return nil, err
	}
	defer func() { node.connectorCredentialCommitted(owner, record) }()
	lifetime, err := node.pluginOwnerContext(ctx)
	if err != nil {
		return nil, err
	}
	operation, cancel := context.WithTimeout(lifetime, authorization.Deadline)
	defer cancel()
	transport := &renewalTransport{base: node.auth.client.Transport}
	client := *node.auth.client
	client.Transport = transport
	var issued authorization.Issued
	if record.Method == protocol.PluginConnectorKindOauth2 {
		for name, value := range previous.Grant.Fields() {
			fields[name] = value
		}
		status, document, err := authorization.Response(operation, &client, declarationAuth.Refresh.Request, fields, false)
		if err == nil {
			err = authorization.Granted(status, document)
		}
		if err == nil {
			issued, err = authorization.ParseIssued(document, declarationAuth.Refresh.Token, uint64(time.Now().UnixMilli()), previous.Grant)
		}
		if err != nil {
			_, err = node.connectorRenewalFailed(record, *previous.Grant, transport.sent.Load(), err, ctx)
			return nil, err
		}
	} else {
		result, err := authorization.Run(operation, authorization.Input{Declaration: declarationAuth, Fields: fields, Client: &client})
		if err == nil && result.Issued == nil {
			err = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "connector authorization returned no grant"}
		}
		if err != nil {
			_, err = node.connectorRenewalFailed(record, *previous.Grant, transport.sent.Load(), err, ctx)
			return nil, err
		}
		issued = *result.Issued
	}
	delete(issued.Extra, "renewal_pending")
	err = node.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
		var err error
		record, err = store.SaveConnectorGrant(owner, db, record, issued)
		return err
	})
	if err != nil {
		return nil, err
	}
	node.authChanged()
	token := protocol.NewSecret(issued.Access)
	return &token, nil
}

// connectorRenewalFailed settles a renewal that issued nothing. The grant
// keeps its uncertain marker only when a request left this Node and the
// service gave no definite answer: the service may then have replaced the
// renewal credential. A request that never left and a definite refusal both
// leave the stored grant as it was, so the marker is removed; the first can be
// tried again, the second needs a new sign-in.
func (node *Node) connectorRenewalFailed(record store.ConnectorRecord, previous authorization.Issued, sent bool, cause error, contexts ...context.Context) (store.ConnectorRecord, error) {
	ctx := node.ctx
	if len(contexts) != 0 {
		ctx = contexts[0]
	}
	var fault *protocol.Fault
	refused := sent && errors.As(cause, &fault) && fault.Code == protocol.ErrorCodePermissionDenied
	if sent && !refused {
		if record.Grant != nil {
			uncertain := *record.Grant
			uncertain.Extra = maps.Clone(uncertain.Extra)
			uncertain.Extra["renewal_uncertain"] = "true"
			_ = node.profile.Store.Node.Write(context.WithoutCancel(ctx), func(db *sql.Conn) error {
				var err error
				record, err = store.SaveConnectorGrant(context.WithoutCancel(ctx), db, record, uncertain)
				return err
			})
			node.connectorCredentialCommitted(ctx, record)
		}
		return record, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "connector renewal did not complete; connect again"}
	}
	previous.Extra = maps.Clone(previous.Extra)
	if previous.Extra == nil {
		previous.Extra = map[string]string{}
	}
	if refused {
		previous.Extra["renewal_rejected"] = "true"
	}
	owner := context.WithoutCancel(ctx)
	var restored store.ConnectorRecord
	err := node.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
		var err error
		restored, err = store.SaveConnectorGrant(owner, db, record, previous)
		return err
	})
	if err != nil {
		return record, err
	}
	record = restored
	node.connectorCredentialCommitted(ctx, record)
	if refused {
		return record, &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "connector authorization was refused; connect again"}
	}
	return record, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "connector service could not be reached"}
}

func (node *Node) connectorCredentialCommitted(ctx context.Context, record store.ConnectorRecord) {
	recording := context.WithoutCancel(ctx)
	var changes []protocol.Event
	_ = node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
		value := protocol.DefaultPluginConnectorChanges()
		var err error
		value.Types, err = store.ConnectorTypes(recording, db)
		if err == nil {
			value.Connections, err = store.Connections(recording, db)
		}
		if err != nil {
			return err
		}
		node.auth.mu.Lock()
		value.Cursor = node.auth.cursor + 1
		node.auth.mu.Unlock()
		changes = []protocol.Event{{Kind: "connections_changed", Data: value}}
		_, err = store.RecordEvents(recording, db, node.ID(), changes)
		return err
	})
	node.authChanged()
	node.NotifyConfigurationFollow(changes)
	node.publish()
}

func (node *Node) authorizePluginHTTP(ctx context.Context, request protocol.Request, info protocol.PluginInfo, input protocol.PluginHttpRequest, prepared plugins.HTTPInput) (plugins.HTTPInput, error) {
	if input.Connector == nil {
		if input.Connection != nil {
			return plugins.HTTPInput{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "connection requires a declared connector"}
		}
		return prepared, nil
	}
	if input.Credential != nil || request.Plugin == nil {
		return plugins.HTTPInput{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "connector authorization requires captured plugin provenance"}
	}
	var record store.ConnectorRecord
	var declaration protocol.PluginConnectorDeclaration
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		record, declaration, err = store.ConnectorAuthorization(ctx, db, plugins.PackageReference(info), *input.Connector, input.Connection, request.Plugin.Turn)
		return err
	})
	if err != nil {
		return plugins.HTTPInput{}, err
	}
	grant, err := node.connectorAccess(ctx, record, declaration)
	if err != nil {
		return plugins.HTTPInput{}, err
	}
	return plugins.BindConnectorHTTP(prepared, declaration, record.Method, record.Secrets, grant)
}

type connectorBearerHandler struct {
	source oauth2.TokenSource
}

func (handler *connectorBearerHandler) TokenSource(context.Context) (oauth2.TokenSource, error) {
	return handler.source, nil
}

func (handler *connectorBearerHandler) Authorize(_ context.Context, _ *http.Request, response *http.Response) error {
	if response != nil && response.Body != nil {
		_ = response.Body.Close()
	}
	return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "MCP authorization requires reconnecting the service"}
}

var _ sdkauth.OAuthHandler = (*connectorBearerHandler)(nil)
