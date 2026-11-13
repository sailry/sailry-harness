package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"maps"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"

	"github.com/kaptinlin/jsonpointer"
	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func connectorUnavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "connector authorization is unavailable"}
}

func connectorAuth(declaration protocol.PluginConnectorDeclaration, method protocol.PluginConnectorKind) (protocol.PluginConnectorAuth, error) {
	for _, auth := range declaration.Auth {
		if auth.Kind == string(method) {
			return auth, nil
		}
	}
	return protocol.PluginConnectorAuth{}, connectorUnavailable()
}

func connectorURL(declaration protocol.PluginConnectorDeclaration, template string, fields map[string]string) (string, error) {
	value, err := authorization.Expand(template, fields)
	if err != nil {
		return "", err
	}
	address, err := url.Parse(value)
	if err != nil || address.Scheme != "https" || address.Host == "" || address.User != nil || address.Fragment != "" || !slices.Contains(declaration.Origins, address.Scheme+"://"+address.Host) {
		return "", &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "connector request is outside its declared origins"}
	}
	return value, nil
}

func connectorClientFields(client protocol.PluginConnectorClient, record store.ConnectorRecord, values map[string]string) error {
	switch client.Kind {
	case "Bundled":
		definition, ok := dispatchPayload[protocol.PluginConnectorClientBundledPayload](client.Data)
		if !ok {
			return connectorUnavailable()
		}
		values["client_id"] = definition.Bundled.ClientID
	case "Fields":
		definition, ok := dispatchPayload[protocol.PluginConnectorClientFieldsPayload](client.Data)
		if !ok {
			return connectorUnavailable()
		}
		values["client_id"] = record.Fields[definition.Fields.ClientID]
		if definition.Fields.ClientSecret != nil {
			values["client_secret"] = record.Secrets[*definition.Fields.ClientSecret].Expose()
		}
	default:
		return connectorUnavailable()
	}
	if values["client_id"] == "" {
		return connectorUnavailable()
	}
	return nil
}

func connectorModelAuth(capture store.ConnectorLoginCapture, registration bool) (protocol.PluginModelAuthDeclaration, map[string]string, error) {
	declaration := protocol.DefaultPluginModelAuthDeclaration()
	declaration.ID, declaration.Display = capture.Declaration.ID, capture.Declaration.Display
	fields := maps.Clone(capture.Record.Fields)
	if fields == nil {
		fields = map[string]string{}
	}
	for name, secret := range capture.Record.Secrets {
		fields[name] = secret.Expose()
	}
	auth, err := connectorAuth(capture.Declaration, capture.Record.Method)
	if err != nil {
		return declaration, fields, err
	}
	endpoint := func(value string) (protocol.PluginModelAuthEndpoint, error) {
		target, err := connectorURL(capture.Declaration, value, capture.Record.Fields)
		return protocol.PluginModelAuthEndpoint{Url: target, Method: protocol.PluginModelAuthMethodPost, Headers: map[string]string{}}, err
	}
	switch auth.Kind {
	case "oauth2":
		auth, ok := dispatchPayload[protocol.PluginConnectorAuthOauth2Payload](auth.Data)
		if !ok || connectorClientFields(auth.Client, capture.Record, fields) != nil {
			return declaration, fields, connectorUnavailable()
		}
		token, err := endpoint(auth.Token)
		if err != nil {
			return declaration, fields, err
		}
		refreshPath, expiryPath := "/refresh_token", "/expires_in"
		mapping := protocol.PluginModelAuthMapping{Access: "/access_token", Refresh: &refreshPath, ExpiresIn: &expiryPath, Extra: map[string]string{}}
		declaration.Token, declaration.Scopes = &mapping, auth.Scopes
		if auth.Refresh {
			refresh := token
			form := map[string]string{"grant_type": "refresh_token", "refresh_token": "{refresh}", "client_id": "{client_id}"}
			// A confidential client renews with its secret, as it logs in.
			if fields["client_secret"] != "" {
				form["client_secret"] = "{client_secret}"
			}
			refresh.Body = &protocol.PluginModelAuthBody{Kind: "form", Data: protocol.PluginModelAuthBodyFormPayload{Fields: form}}
			declaration.Refresh = &protocol.PluginModelAuthRefresh{Request: refresh, Token: mapping}
		}
		if auth.Flow == protocol.PluginConnectorFlowPkce {
			authorize, err := connectorURL(capture.Declaration, auth.Authorize, capture.Record.Fields)
			if err != nil {
				return declaration, fields, err
			}
			declaration.Flow = protocol.PluginModelAuthFlow{Kind: "pkce", Data: protocol.PluginModelAuthFlowPkcePayload{Authorize: authorize, Token: token, Redirect: protocol.PluginModelAuthRedirectLoopback}}
		} else if auth.Device != nil {
			device, err := endpoint(*auth.Device)
			if err != nil {
				return declaration, fields, err
			}
			declaration.Flow = protocol.PluginModelAuthFlow{Kind: "device_code", Data: protocol.PluginModelAuthFlowDeviceCodePayload{Device: device, Token: token}}
		} else {
			return declaration, fields, connectorUnavailable()
		}
	case "application":
		auth, ok := dispatchPayload[protocol.PluginConnectorAuthApplicationPayload](auth.Data)
		if !ok {
			return declaration, fields, connectorUnavailable()
		}
		fields["client_id"] = capture.Record.Fields[auth.Fields.ClientID]
		if auth.Fields.ClientSecret != nil {
			fields["client_secret"] = capture.Record.Secrets[*auth.Fields.ClientSecret].Expose()
		}
		if registration {
			if auth.Registration == nil || *auth.Registration == nil {
				return declaration, fields, connectorUnavailable()
			}
			register := **auth.Registration
			declaration.Flow, declaration.Token, declaration.Scopes = register.Flow, &register.Token, register.Scopes
			break
		}
		if fields["client_id"] == "" || auth.Fields.ClientSecret != nil && fields["client_secret"] == "" {
			return declaration, fields, connectorUnavailable()
		}
		token, err := endpoint(auth.Token.Url)
		if err != nil {
			return declaration, fields, err
		}
		if auth.Token.Form {
			token.Body = &protocol.PluginModelAuthBody{Kind: "form", Data: protocol.PluginModelAuthBodyFormPayload{Fields: auth.Token.Body}}
		} else {
			body, err := json.Marshal(auth.Token.Body)
			if err != nil {
				return declaration, fields, err
			}
			token.Body = &protocol.PluginModelAuthBody{Kind: "json", Data: protocol.PluginModelAuthBodyJsonPayload{Value: body}}
		}
		declaration.Flow = protocol.PluginModelAuthFlow{Kind: "token_exchange", Data: protocol.PluginModelAuthFlowTokenExchangePayload{Token: token}}
		declaration.Token = &protocol.PluginModelAuthMapping{Access: auth.Token.Access, ExpiresIn: &auth.Token.ExpiresIn, Extra: map[string]string{}}
	default:
		return declaration, fields, connectorUnavailable()
	}
	return declaration, fields, nil
}

func (node *Node) runConnectorLogin(ctx context.Context, capture *store.ConnectorLoginCapture, callback <-chan string, progress func(protocol.ConversationLoginState) error) (*authorization.Issued, *protocol.PluginConnectorIdentity, error) {
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	switch capture.Record.Method {
	case protocol.PluginConnectorKindSecret:
		auth, _ := connectorAuth(capture.Declaration, capture.Record.Method)
		definition, ok := dispatchPayload[protocol.PluginConnectorAuthSecretPayload](auth.Data)
		if !ok {
			return nil, nil, connectorUnavailable()
		}
		for _, name := range definition.Fields {
			if capture.Record.Secrets[name].Expose() == "" {
				return nil, nil, connectorUnavailable()
			}
		}
		identity, err := node.connectorIdentity(ctx, *capture, nil)
		return nil, identity, err
	case protocol.PluginConnectorKindLocalCli:
		identity, err := node.connectorCLIIdentity(ctx, capture.Declaration)
		return nil, &identity, err
	case protocol.PluginConnectorKindMcpOauth:
		return node.connectorMCPLogin(ctx, capture, callback, progress)
	}
	run := func(registration bool) (*authorization.Issued, error) {
		declaration, fields, err := connectorModelAuth(*capture, registration)
		if err != nil {
			return nil, err
		}
		target := providers.AccountTarget{Package: capture.Attempt.Package, Declaration: declaration}
		bundle, err := node.providerBundle(ctx, target)
		if err != nil {
			return nil, err
		}
		var returned <-chan string
		redirect := ""
		if declaration.Flow.Kind == "pkce" || declaration.Flow.Kind == "script" {
			redirect, _, err = loginCallback(declaration, capture.Input.Redirect)
			if err != nil {
				return nil, err
			}
			returned = callback
		}
		operation := node.plugins.memory.Context(ctx, capture.Attempt.Package, "callback")
		result, err := authorization.Run(operation, authorization.Input{Declaration: declaration, Fields: fields, Redirect: redirect, Callback: returned, Bundle: bundle, HostID: node.auth.host, Progress: progress})
		return result.Issued, err
	}
	if capture.Input.Registration {
		issued, err := run(true)
		if err != nil || issued == nil {
			return nil, nil, err
		}
		auth, _ := connectorAuth(capture.Declaration, capture.Record.Method)
		application, ok := dispatchPayload[protocol.PluginConnectorAuthApplicationPayload](auth.Data)
		if !ok || application.Registration == nil || *application.Registration == nil {
			return nil, nil, connectorUnavailable()
		}
		issues := (**application.Registration).Issues
		if issues.ClientSecret == nil || issued.Extra["client_id"] == "" {
			return nil, nil, connectorUnavailable()
		}
		capture.Record.Fields[issues.ClientID] = issued.Extra["client_id"]
		capture.Record.Secrets[*issues.ClientSecret] = protocol.NewSecret(issued.Access)
	}
	issued, err := run(false)
	if err != nil || issued == nil {
		return nil, nil, err
	}
	identity, err := node.connectorIdentity(ctx, *capture, issued)
	return issued, identity, err
}

func (node *Node) connectorIdentity(ctx context.Context, capture store.ConnectorLoginCapture, grant *authorization.Issued) (*protocol.PluginConnectorIdentity, error) {
	if capture.Declaration.Identity == nil && capture.Declaration.Health == nil {
		return nil, nil
	}
	call := func(template string) (int, any, error) {
		target, err := connectorURL(capture.Declaration, template, capture.Record.Fields)
		if err != nil {
			return 0, nil, err
		}
		input, err := plugins.PrepareHTTP(protocol.PluginHttpRequest{Method: http.MethodGet, Url: target, Headers: map[string]string{}, TimeoutMs: 30_000}, nil, nil)
		if err != nil {
			return 0, nil, err
		}
		var access *protocol.Secret
		if grant != nil {
			value := protocol.NewSecret(grant.Access)
			access = &value
		}
		input, err = plugins.BindConnectorHTTP(input, capture.Declaration, capture.Record.Method, capture.Record.Secrets, access)
		if err != nil {
			return 0, nil, err
		}
		endpoint := protocol.PluginModelAuthEndpoint{Url: target, Method: protocol.PluginModelAuthMethodGet, Headers: map[string]string{}}
		for name := range input.Headers {
			endpoint.Headers[name] = input.Headers.Get(name)
		}
		return authorization.Response(ctx, authorization.HTTPClient(), endpoint, map[string]string{}, true)
	}
	if health := capture.Declaration.Health; health != nil {
		status, _, err := call(health.Request)
		if err != nil {
			return nil, err
		}
		if !slices.Contains(health.Ok, uint16(status)) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "connector authorization was rejected by the service"}
		}
	}
	if identity := capture.Declaration.Identity; identity != nil {
		status, document, err := call(identity.Request)
		if err != nil || status < 200 || status >= 300 {
			return nil, connectorUnavailable()
		}
		name, err := jsonpointer.Value(document, identity.Name)
		if err != nil {
			return nil, connectorUnavailable()
		}
		id, err := jsonpointer.Value(document, identity.ID)
		if err != nil {
			return nil, connectorUnavailable()
		}
		identityName, okName := name.(string)
		identityID, okID := id.(string)
		if !okName || !okID || strings.TrimSpace(identityName) == "" || identityID == "" || len(identityName) > 1024 || len(identityID) > 1024 {
			return nil, connectorUnavailable()
		}
		return &protocol.PluginConnectorIdentity{Name: identityName, ID: identityID}, nil
	}
	return nil, nil
}

func (node *Node) probeConnector(ctx context.Context, id protocol.PluginConnectorID) (protocol.PluginConnectorConnection, error) {
	var record store.ConnectorRecord
	var info protocol.PluginInfo
	var declaration protocol.PluginConnectorDeclaration
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		record, err = store.ReadConnectorRecord(ctx, db, id)
		if err != nil {
			return err
		}
		value, err := store.ReadPlugin(ctx, db, record.Plugin)
		if err != nil {
			return err
		}
		if value == nil {
			return connectorUnavailable()
		}
		info = *value
		_, declaration, err = store.ConnectorDeclaration(ctx, db, plugins.PackageReference(info), record.Connector)
		return err
	})
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	if _, err := node.connectorAccess(ctx, record, declaration); err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		record, err = store.ReadConnectorRecord(ctx, db, record.ID)
		return err
	})
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	capture := store.ConnectorLoginCapture{Package: info, Declaration: declaration, Record: record, Attempt: protocol.PluginConnectorAttempt{Package: plugins.PackageReference(info)}}
	var identity *protocol.PluginConnectorIdentity
	if record.Method == protocol.PluginConnectorKindLocalCli {
		value, operationErr := node.connectorCLIIdentity(ctx, declaration)
		identity, err = &value, operationErr
	} else {
		identity, err = node.connectorIdentity(ctx, capture, record.Grant)
	}
	if err == nil && record.Identity != nil && identity != nil && record.Identity.ID != identity.ID {
		err = &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "connector account changed; connect again"}
	}
	operationErr := err
	var failure *protocol.Fault
	if operationErr != nil {
		failure = pluginFailure(operationErr)
	}
	var projection protocol.PluginConnectorConnection
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		projection, err = store.ProbeConnector(ctx, db, record, identity, uint64(time.Now().UnixMilli()), failure)
		return err
	})
	if err == nil {
		node.authChanged()
		err = operationErr
	}
	return projection, err
}
