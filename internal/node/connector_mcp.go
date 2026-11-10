package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"maps"
	"net/url"
	"reflect"
	"slices"
	"strings"
	"sync"
	"time"

	sdkauth "github.com/modelcontextprotocol/go-sdk/auth"
	"github.com/modelcontextprotocol/go-sdk/oauthex"
	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http/httpguts"
	"golang.org/x/oauth2"
)

func connectorMCPDefinition(server protocol.PluginMcpServer) protocol.PluginMcpDefinition {
	body, _ := json.Marshal(server.Data)
	switch server.Kind {
	case "stdio":
		var value protocol.PluginMcpServerStdioPayload
		_ = json.Unmarshal(body, &value)
		names := slices.Sorted(maps.Keys(value.Env))
		return protocol.PluginMcpDefinition{Kind: "stdio", Data: protocol.PluginMcpDefinitionStdioPayload{Command: value.Command, Args: value.Args, Env: names}}
	case "sse":
		var value protocol.PluginMcpServerSsePayload
		_ = json.Unmarshal(body, &value)
		return protocol.PluginMcpDefinition{Kind: "sse", Data: protocol.PluginMcpDefinitionSsePayload{Url: value.Url, Headers: slices.Sorted(maps.Keys(value.Headers))}}
	default:
		var value protocol.PluginMcpServerHttpPayload
		_ = json.Unmarshal(body, &value)
		return protocol.PluginMcpDefinition{Kind: "streamable-http", Data: protocol.PluginMcpDefinitionStreamableHttpPayload{Url: value.Url, Headers: slices.Sorted(maps.Keys(value.Headers))}}
	}
}

func (node *Node) captureConnectorTargets(ctx context.Context, capture *store.ConnectorLoginCapture) error {
	if capture.Record.Bound == nil || !slices.ContainsFunc(capture.Declaration.Inject, func(injection protocol.PluginConnectorInjection) bool { return injection.Server != nil }) {
		return nil
	}
	_, _, configuration, err := node.capturedPluginMCP(ctx, capture.Attempt.Package, true)
	if err != nil {
		return err
	}
	for _, injection := range capture.Declaration.Inject {
		if injection.Server == nil {
			continue
		}
		server, exists := configuration.Servers[*injection.Server]
		if !exists {
			return connectorUnavailable()
		}
		if server.Kind == "stdio" {
			if injection.Env == nil || injection.Header != nil {
				return connectorUnavailable()
			}
		} else {
			if injection.Header == nil || injection.Env != nil {
				return connectorUnavailable()
			}
			var target struct {
				URL string `json:"url"`
			}
			body, _ := json.Marshal(server.Data)
			if json.Unmarshal(body, &target) != nil {
				return connectorUnavailable()
			}
			address, err := url.Parse(target.URL)
			if err != nil || !slices.Contains(capture.Declaration.Origins, address.Scheme+"://"+address.Host) {
				return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "connector MCP target is outside its declared origins"}
			}
		}
		capture.Record.Bound.Targets[*injection.Server] = connectorMCPDefinition(server)
	}
	return nil
}

type connectorOAuthRecord struct {
	Config oauth2.Config `json:"config"`
	Token  oauth2.Token  `json:"token"`
}

func connectorOAuthIssued(config *oauth2.Config, token *oauth2.Token) authorization.Issued {
	extra := map[string]string{}
	body, _ := json.Marshal(connectorOAuthRecord{Config: *config, Token: *token})
	extra["mcp_oauth"] = string(body)
	issued := authorization.Issued{Access: token.AccessToken, Extra: extra}
	if token.RefreshToken != "" {
		refresh := token.RefreshToken
		issued.Refresh = &refresh
	}
	if !token.Expiry.IsZero() {
		expiry := uint64(token.Expiry.UnixMilli())
		issued.ExpiresAtMs = &expiry
	}
	return issued
}

func (node *Node) connectorMCPLogin(ctx context.Context, capture *store.ConnectorLoginCapture, callback <-chan string, progress func(protocol.ConversationLoginState) error) (*authorization.Issued, *protocol.PluginConnectorIdentity, error) {
	auth, err := connectorAuth(capture.Declaration, capture.Record.Method)
	if err != nil {
		return nil, nil, err
	}
	definition, ok := dispatchPayload[protocol.PluginConnectorAuthMcpOauthPayload](auth.Data)
	if !ok || capture.Input.Redirect == nil {
		return nil, nil, connectorUnavailable()
	}
	redirect := *capture.Input.Redirect
	if authorization.ValidateRedirect(redirect, protocol.PluginModelAuthCallback{Path: "/auth/callback"}) != nil {
		return nil, nil, connectorUnavailable()
	}
	_, _, configuration, err := node.capturedPluginMCP(ctx, capture.Attempt.Package, true)
	if err != nil {
		return nil, nil, err
	}
	server, exists := configuration.Servers[definition.Server]
	if !exists || server.Kind == "stdio" {
		return nil, nil, connectorUnavailable()
	}
	var target struct {
		URL string `json:"url"`
	}
	body, _ := json.Marshal(server.Data)
	if json.Unmarshal(body, &target) != nil {
		return nil, nil, connectorUnavailable()
	}
	var issued *authorization.Issued
	config := &sdkauth.AuthorizationCodeHandlerConfig{
		RedirectURL: redirect, Client: authorization.HTTPClient(), RequestRefreshToken: true,
		DynamicClientRegistrationConfig: &sdkauth.DynamicClientRegistrationConfig{Metadata: &oauthex.ClientRegistrationMetadata{
			RedirectURIs: []string{redirect}, TokenEndpointAuthMethod: "none", ClientName: "Sailry",
			GrantTypes: []string{"authorization_code", "refresh_token"}, ResponseTypes: []string{"code"},
		}},
		AuthorizationCodeFetcher: func(ctx context.Context, args *sdkauth.AuthorizationArgs) (*sdkauth.AuthorizationResult, error) {
			if err := progress(protocol.ConversationLoginState{Kind: "browser", Data: protocol.ConversationLoginStateBrowserPayload{
				AuthorizationUrl: args.URL, ExpiresAtMs: uint64(time.Now().Add(authorization.Deadline).UnixMilli()),
			}}); err != nil {
				return nil, err
			}
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case returned := <-callback:
				value, err := url.Parse(returned)
				if err != nil {
					return nil, connectorUnavailable()
				}
				expected, _ := url.Parse(redirect)
				if value.Scheme != expected.Scheme || value.Host != expected.Host || value.Path != expected.Path || value.Query().Get("error") != "" {
					return nil, connectorUnavailable()
				}
				return &sdkauth.AuthorizationResult{Code: value.Query().Get("code"), State: value.Query().Get("state"), Iss: value.Query().Get("iss")}, nil
			}
		},
		NewTokenSource: func(_ context.Context, config *oauth2.Config, token *oauth2.Token) (oauth2.TokenSource, error) {
			value := connectorOAuthIssued(config, token)
			issued = &value
			return oauth2.StaticTokenSource(token), nil
		},
	}
	if definition.Client != nil {
		fields := map[string]string{}
		if err := connectorClientFields(*definition.Client, capture.Record, fields); err != nil {
			return nil, nil, err
		}
		config.PreregisteredClient = &oauthex.ClientCredentials{ClientID: fields["client_id"]}
		if secret := fields["client_secret"]; secret != "" {
			config.PreregisteredClient.ClientSecretAuth = &oauthex.ClientSecretAuth{ClientSecret: secret}
		}
	}
	handler, err := sdkauth.NewAuthorizationCodeHandler(config)
	if err != nil {
		return nil, nil, connectorUnavailable()
	}
	operation, cancel := context.WithTimeout(ctx, authorization.Deadline)
	defer cancel()
	session, err := mcp.Open(operation, mcp.Launch{Kind: server.Kind, URL: target.URL, OAuth: handler}, nil)
	if session != nil {
		err = errors.Join(err, session.Close())
	}
	if err != nil || issued == nil {
		return nil, nil, connectorUnavailable()
	}
	return issued, nil, nil
}

func (node *Node) authorizeMCP(ctx context.Context, info protocol.PluginInfo, serverName string, launch *mcp.Launch) (any, error) {
	identities := []any{}
	for _, declaration := range store.ConnectorDeclarations(info) {
		if !slices.ContainsFunc(declaration.Inject, func(injection protocol.PluginConnectorInjection) bool {
			return injection.Server != nil && *injection.Server == serverName
		}) {
			continue
		}
		var record store.ConnectorRecord
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			record, _, err = store.ConnectorAuthorization(ctx, db, plugins.PackageReference(info), declaration.ID, nil, nil)
			return err
		})
		if err != nil {
			var failure *protocol.Fault
			if info.McpSource != nil && errors.As(err, &failure) && failure.Code == protocol.ErrorCodeNotConfigured {
				continue
			}
			return nil, err
		}
		if record.Bound != nil {
			_, _, configuration, err := node.capturedPluginMCP(ctx, plugins.PackageReference(info), false)
			if err != nil {
				return nil, err
			}
			server, exists := configuration.Servers[serverName]
			expected, bound := record.Bound.Targets[serverName]
			if !exists || !bound || !sameConnectorTarget(expected, connectorMCPDefinition(server)) {
				return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "connector MCP target changed; confirm the connection"}
			}
		}
		grant, err := node.connectorAccess(ctx, record, declaration)
		if err != nil {
			return nil, err
		}
		previous := record
		err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			record, _, err = store.ConnectorAuthorization(ctx, db, plugins.PackageReference(info), declaration.ID, &previous.ID, nil)
			return err
		})
		if err != nil {
			return nil, err
		}
		if record.AuthRevision != previous.AuthRevision || grant != nil && record.Method != protocol.PluginConnectorKindLocalCli && (record.Grant == nil || record.Grant.Access != grant.Expose()) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "connector authorization changed"}
		}
		if record.Method == protocol.PluginConnectorKindMcpOauth {
			source, err := node.connectorMCPSource(record)
			if err != nil {
				return nil, err
			}
			launch.OAuth = &connectorBearerHandler{source: source}
		}
		for _, injection := range declaration.Inject {
			if injection.Server == nil || *injection.Server != serverName {
				continue
			}
			token, err := connectorInjection(record, declaration, injection, grant)
			if err != nil {
				return nil, err
			}
			if token == nil {
				continue
			}
			value := injection.Prefix + token.Expose()
			if injection.Env != nil && launch.Kind == "stdio" && injection.Header == nil {
				if launch.Env == nil {
					launch.Env = map[string]string{}
				}
				if _, exists := launch.Env[*injection.Env]; exists {
					return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "connector environment slot cannot be overridden"}
				}
				launch.Env[*injection.Env] = value
			} else if injection.Header != nil && launch.Kind != "stdio" && injection.Env == nil {
				target, err := url.Parse(launch.URL)
				if err != nil || !slices.Contains(declaration.Origins, target.Scheme+"://"+target.Host) || !httpguts.ValidHeaderFieldName(*injection.Header) || !httpguts.ValidHeaderFieldValue(value) {
					return nil, connectorUnavailable()
				}
				if launch.Headers == nil {
					launch.Headers = map[string]string{}
				}
				for name := range launch.Headers {
					if strings.EqualFold(name, *injection.Header) {
						return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "connector header cannot be overridden"}
					}
				}
				launch.Headers[*injection.Header] = value
			} else {
				return nil, connectorUnavailable()
			}
		}
		identities = append(identities, []any{record.ID, record.AuthRevision, record.GrantRevision})
	}
	return identities, nil
}

type connectorMCPAuthority struct {
	info     protocol.PluginInfo
	settings store.ResolvedPluginSettings
	server   string
	target   protocol.PluginMcpServer
	base     mcp.Launch
	launch   mcp.Launch
	identity any
}

func (node *Node) validateMCPAuthorization(ctx context.Context, capture connectorMCPAuthority) error {
	info, settings, configuration, err := node.capturedPluginMCP(ctx, plugins.PackageReference(capture.info), true)
	if err != nil {
		return err
	}
	allowed, configured := settings.MCPAllowed[capture.server]
	if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" || configured && !allowed {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "MCP server is unavailable"}
	}
	server, exists := configuration.Servers[capture.server]
	if !exists || !reflect.DeepEqual(server, capture.target) || !reflect.DeepEqual(settings.Secrets, capture.settings.Secrets) {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "MCP credentials or destination changed"}
	}
	launch := capture.base
	launch.Env, launch.Headers = maps.Clone(launch.Env), maps.Clone(launch.Headers)
	identity, err := node.authorizeMCP(ctx, info, capture.server, &launch)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(identity, capture.identity) || !reflect.DeepEqual(launch.Env, capture.launch.Env) || !reflect.DeepEqual(launch.Headers, capture.launch.Headers) {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "MCP credentials changed"}
	}
	return nil
}

func sameConnectorTarget(first, second protocol.PluginMcpDefinition) bool {
	firstBody, _ := json.Marshal(first)
	secondBody, _ := json.Marshal(second)
	return string(firstBody) == string(secondBody)
}

func connectorInjection(record store.ConnectorRecord, declaration protocol.PluginConnectorDeclaration, injection protocol.PluginConnectorInjection, grant *protocol.Secret) (*protocol.Secret, error) {
	field := injection.Field
	if field == nil && record.Method == protocol.PluginConnectorKindSecret {
		auth, _ := connectorAuth(declaration, record.Method)
		value, ok := dispatchPayload[protocol.PluginConnectorAuthSecretPayload](auth.Data)
		if !ok || len(value.Fields) == 0 {
			return nil, connectorUnavailable()
		}
		field = &value.Fields[0]
	}
	if field == nil {
		if grant == nil {
			return nil, connectorUnavailable()
		}
		return grant, nil
	}
	for _, definition := range declaration.Fields {
		if definition.Name != *field || !definition.Secret {
			continue
		}
		token, exists := record.Secrets[*field]
		if exists && token.Expose() != "" {
			return &token, nil
		}
		if definition.Optional {
			return nil, nil
		}
	}
	return nil, connectorUnavailable()
}

type connectorOAuthSource struct {
	node   *Node
	mu     sync.Mutex
	record store.ConnectorRecord
	value  connectorOAuthRecord
}

func (node *Node) connectorMCPSource(record store.ConnectorRecord) (*connectorOAuthSource, error) {
	var value connectorOAuthRecord
	if record.Grant == nil || json.Unmarshal([]byte(record.Grant.Extra["mcp_oauth"]), &value) != nil {
		return nil, connectorUnavailable()
	}
	return &connectorOAuthSource{node: node, record: record, value: value}, nil
}

func (source *connectorOAuthSource) Token() (*oauth2.Token, error) {
	source.mu.Lock()
	defer source.mu.Unlock()
	if source.value.Token.Valid() {
		return &source.value.Token, nil
	}
	pending := *source.record.Grant
	pending.Extra = maps.Clone(pending.Extra)
	pending.Extra["renewal_pending"] = "true"
	recording := context.WithoutCancel(source.node.ctx)
	err := source.node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
		var err error
		source.record, err = store.SaveConnectorGrant(recording, db, source.record, pending)
		return err
	})
	if err != nil {
		return nil, err
	}
	ctx := context.WithValue(source.node.ctx, oauth2.HTTPClient, authorization.HTTPClient())
	token, err := source.value.Config.TokenSource(ctx, &source.value.Token).Token()
	if err != nil {
		return nil, connectorUnavailable()
	}
	issued := connectorOAuthIssued(&source.value.Config, token)
	var record store.ConnectorRecord
	err = source.node.profile.Store.Node.Write(context.WithoutCancel(ctx), func(db *sql.Conn) error {
		var err error
		record, err = store.SaveConnectorGrant(context.WithoutCancel(ctx), db, source.record, issued)
		return err
	})
	if err != nil {
		return nil, err
	}
	source.record, source.value.Token = record, *token
	source.node.authChanged()
	return token, nil
}
