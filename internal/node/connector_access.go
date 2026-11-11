package node

import (
	"context"
	"database/sql"
	"maps"
	"net/http"
	"reflect"
	"sync"
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
	if record.Method == protocol.PluginConnectorKindSecret {
		return nil, nil
	}
	value, _ := node.auth.connectorLocks.LoadOrStore(record.ID, &sync.Mutex{})
	lock := value.(*sync.Mutex)
	lock.Lock()
	defer lock.Unlock()
	var current store.ConnectorRecord
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
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
	if record.Grant.ExpiresAtMs == nil || *record.Grant.ExpiresAtMs > uint64(time.Now().UnixMilli()) {
		token := protocol.NewSecret(record.Grant.Access)
		return &token, nil
	}
	if record.Method == protocol.PluginConnectorKindMcpOauth {
		source, err := node.connectorMCPSource(record)
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
	owner := context.WithoutCancel(node.ctx)
	err = node.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
		var err error
		record, err = store.SaveConnectorGrant(owner, db, record, pending)
		return err
	})
	if err != nil {
		return nil, err
	}
	operation, cancel := context.WithTimeout(node.ctx, authorization.Deadline)
	defer cancel()
	var issued authorization.Issued
	if record.Method == protocol.PluginConnectorKindOauth2 {
		for name, value := range previous.Grant.Fields() {
			fields[name] = value
		}
		status, document, err := authorization.Response(operation, authorization.HTTPClient(), declarationAuth.Refresh.Request, fields, false)
		if err != nil || status < 200 || status >= 300 {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "connector renewal did not complete; connect again"}
		}
		issued, err = authorization.ParseIssued(document, declarationAuth.Refresh.Token, uint64(time.Now().UnixMilli()), previous.Grant)
		if err != nil {
			return nil, err
		}
	} else {
		result, err := authorization.Run(operation, authorization.Input{Declaration: declarationAuth, Fields: fields})
		if err != nil || result.Issued == nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "connector authorization did not complete; connect again"}
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
