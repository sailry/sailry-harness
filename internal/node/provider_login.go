package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"maps"
	"strings"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) prepareAuthCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, bool, error) {
	if request.Command.Kind == "request_provider_http" || request.Command.Kind == "execute_provider_http" {
		output, err := node.providerHTTP(ctx, request)
		return output, true, err
	}
	switch request.Command.Kind {
	case "begin_provider_login", "complete_provider_login", "cancel_provider_login", "start_connector_login", "submit_connector_input", "cancel_connector_login",
		"list_connector_types", "list_connections", "probe_connection", "inspect_connector_cli", "confirm_connector_consent":
	default:
		return protocol.Output{}, false, nil
	}
	if request.Plugin != nil {
		return protocol.Output{}, true, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "account authorization requires a controller"}
	}
	switch input := request.Command.Data.(type) {
	case *protocol.CommandBeginProviderLoginPayload:
		output, err := node.beginProviderLogin(ctx, caller, request.ID, *input)
		return output, true, err
	case *protocol.CommandCompleteProviderLoginPayload:
		output, err := node.providerLoginInput(caller, request.ID, input.Attempt, &input.Callback, false)
		return output, true, err
	case *protocol.CommandCancelProviderLoginPayload:
		output, err := node.providerLoginInput(caller, request.ID, input.Attempt, nil, true)
		return output, true, err
	}
	return node.prepareConnectorCommand(ctx, caller, request)
}

func (node *Node) providerBundle(ctx context.Context, target providers.AccountTarget) (*plugins.ScriptBundle, error) {
	var script *protocol.PluginModelAdapterScript
	if target.Declaration.Flow.Kind == "script" {
		flow, ok := dispatchPayload[protocol.PluginModelAuthFlowScriptPayload](target.Declaration.Flow.Data)
		if !ok {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "model authorization script declaration is invalid"}
		}
		script = &flow.Script
	} else if target.Declaration.Exchange != nil {
		script = &target.Declaration.Exchange.Script
	}
	if script == nil {
		return nil, nil
	}
	files, err := node.plugins.manager.ReadBundle(ctx, target.Package, script.Entry, script.Resources)
	if err != nil {
		return nil, err
	}
	return &plugins.ScriptBundle{Entry: script.Entry, Files: files}, nil
}

func loginCallback(declaration protocol.PluginModelAuthDeclaration, supplied *string) (string, bool, error) {
	redirect := ""
	if supplied != nil {
		redirect = *supplied
	}
	switch declaration.Flow.Kind {
	case "script":
		flow, ok := dispatchPayload[protocol.PluginModelAuthFlowScriptPayload](declaration.Flow.Data)
		if !ok {
			break
		}
		if flow.Callback == nil {
			if supplied == nil {
				return "", false, nil
			}
			break
		}
		if redirect == "" && flow.Callback.Manual && flow.Callback.RedirectUri != nil {
			redirect = *flow.Callback.RedirectUri
		}
		if err := authorization.ValidateRedirect(redirect, *flow.Callback); err != nil {
			return "", false, err
		}
		return redirect, true, nil
	case "pkce":
		flow, ok := dispatchPayload[protocol.PluginModelAuthFlowPkcePayload](declaration.Flow.Data)
		if !ok {
			break
		}
		if flow.Redirect == protocol.PluginModelAuthRedirectPaste {
			if supplied != nil || flow.RedirectUri == nil {
				break
			}
			redirect = *flow.RedirectUri
		} else if supplied == nil {
			break
		}
		return redirect, true, nil
	default:
		if supplied == nil {
			return "", false, nil
		}
	}
	return "", false, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "model authorization callback does not match its declaration"}
}

func (node *Node) beginProviderLogin(ctx context.Context, caller protocol.NodeID, attempt protocol.RequestID, input protocol.CommandBeginProviderLoginPayload) (protocol.Output, error) {
	var capture store.ProviderLoginCapture
	var registration *string
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		capture, err = store.CaptureProviderLogin(ctx, db, input.Provider, input.ExpectedRevision, attempt)
		if err == nil {
			registration, err = store.OAuthRegistration(ctx, db, capture.Target.Package.Name+":"+capture.Target.Flow)
		}
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	redirect, callback, err := loginCallback(capture.Target.Declaration, input.Redirect)
	if err != nil {
		return protocol.Output{}, err
	}
	bundle, err := node.providerBundle(ctx, capture.Target)
	if err != nil {
		return protocol.Output{}, err
	}
	entry := &authEntry{owner: caller, kind: "provider_login", revision: 1, capture: &capture,
		provider: protocol.ConversationLoginUpdate{Attempt: capture.Attempt, Revision: 1, State: protocol.ConversationLoginState{Kind: "starting"}}}
	if callback {
		entry.callback = make(chan string, 1)
	}
	returned := entry.callback
	if err := node.addAuthEntry(attempt, entry); err != nil {
		return protocol.Output{}, err
	}
	node.auth.mu.Lock()
	node.auth.effects[attempt] = func(accepted bool) {
		if !accepted {
			node.setProviderLogin(entry, protocol.ConversationLoginState{Kind: "cancelled"})
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
				return
			}
			result, err := authorization.Run(operation, authorization.Input{
				Declaration: capture.Target.Declaration, Fields: capture.Target.Fields, Redirect: redirect, Callback: returned, Bundle: bundle,
				Client: node.auth.client,
				HostID: node.auth.host, Registration: registration,
				Registered: func(ctx context.Context, client string) error {
					return node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
						return store.SaveOAuthRegistration(ctx, db, capture.Target.Package.Name+":"+capture.Target.Flow, client)
					})
				},
				Progress: func(state protocol.ConversationLoginState) error { return node.setProviderLogin(entry, state) },
			})
			var grant providers.AccountGrant
			if err == nil {
				grant, err = providerLoginGrant(capture, result)
			}
			var changes []protocol.Event
			if err == nil {
				recording := context.WithoutCancel(node.ctx)
				err = node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
					var err error
					changes, err = store.CompleteProviderLogin(recording, db, node.ID(), capture, grant)
					if err == nil {
						_, err = store.RecordEvents(recording, db, node.ID(), changes)
					}
					return err
				})
			}
			if err != nil {
				state := protocol.ConversationLoginState{Kind: "failed", Data: pluginFailure(err)}
				if operation.Err() != nil {
					state = protocol.ConversationLoginState{Kind: "cancelled"}
				}
				_ = node.setProviderLogin(entry, state)
				return
			}
			node.pluginCommitted(changes)
			_ = node.setProviderLogin(entry, protocol.ConversationLoginState{Kind: "connected"})
		})
		if err != nil {
			_ = node.setProviderLogin(entry, protocol.ConversationLoginState{Kind: "failed", Data: pluginFailure(err)})
		}
	}
	node.auth.mu.Unlock()
	return protocol.Output{Kind: "provider_login", Data: capture.Attempt}, nil
}

func providerLoginGrant(capture store.ProviderLoginCapture, result authorization.Result) (providers.AccountGrant, error) {
	if result.Grant != nil {
		var grant providers.AccountGrant
		if json.Unmarshal(result.Grant, &grant) != nil || grant.Kind != capture.Provider.Authentication {
			return grant, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "model authorization returned an invalid grant"}
		}
		return grant, nil
	}
	if result.Issued == nil || capture.Provider.Authentication != protocol.AuthenticationPlugin {
		return providers.AccountGrant{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "model authorization did not return its declared grant"}
	}
	return providers.AccountGrant{Kind: protocol.AuthenticationPlugin, Record: &providers.AccountRecord{Target: capture.Target, Issued: *result.Issued}}, nil
}

func (node *Node) setProviderLogin(entry *authEntry, state protocol.ConversationLoginState) error {
	node.auth.mu.Lock()
	defer node.auth.mu.Unlock()
	if node.auth.closed || entry.finished {
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "authorization attempt has finished"}
	}
	entry.revision++
	entry.provider.State, entry.provider.Revision = state, entry.revision
	entry.finished = state.Kind == "connected" || state.Kind == "failed" || state.Kind == "cancelled"
	node.auth.notify()
	return nil
}

func (node *Node) providerLoginInput(caller protocol.NodeID, request, attempt protocol.RequestID, callback *protocol.Secret, cancel bool) (protocol.Output, error) {
	node.auth.mu.Lock()
	defer node.auth.mu.Unlock()
	entry, err := node.authEntry(caller, attempt, "provider_login")
	if err != nil {
		return protocol.Output{}, err
	}
	if entry.finished || (!cancel && entry.callback == nil) {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "authorization is not awaiting this action"}
	}
	if callback != nil && (len(callback.Expose()) > 16*1024 || strings.ContainsFunc(callback.Expose(), unicode.IsControl)) {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "authorization callback is invalid"}
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
			_ = node.setProviderLogin(entry, protocol.ConversationLoginState{Kind: "cancelled"})
			return
		}
		if entry.finished {
			node.auth.mu.Unlock()
			return
		}
		target := entry.callback
		entry.callback = nil
		node.auth.mu.Unlock()
		if target != nil {
			target <- callback.Expose()
		}
		_ = node.setProviderLogin(entry, protocol.ConversationLoginState{Kind: "exchanging"})
	}
	return protocol.Output{Kind: "provider_login", Data: entry.provider.Attempt}, nil
}

func (node *Node) renewProvider(ctx context.Context, provider protocol.ProviderID) error {
	var captured protocol.ConversationProvider
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		captured, err = store.CaptureProviderAuthorization(ctx, db, provider)
		return err
	})
	if err != nil || captured.Authentication == protocol.AuthenticationApiKey {
		return err
	}
	if captured.Credential == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "provider sign-in is required"}
	}
	_, err = node.providerAuthorization(ctx, captured, *captured.Credential)
	return err
}

func (node *Node) authorizeTurnProvider(ctx context.Context, invocation *store.AgentInvocation) error {
	if invocation.Provider == nil || invocation.Provider.Authentication == protocol.AuthenticationApiKey {
		return nil
	}
	reference := invocation.Turn.Config.Credential
	if reference == nil {
		reference = invocation.Provider.Credential
	}
	if reference == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "provider sign-in is required"}
	}
	secret, err := node.providerAuthorization(ctx, *invocation.Provider, *reference)
	if err == nil {
		invocation.Secret, invocation.AuthorizationFailure = secret, nil
	}
	return err
}

func (node *Node) providerAuthorization(ctx context.Context, provider protocol.ConversationProvider, reference protocol.CredentialRef) (*protocol.Secret, error) {
	if reference.Node != node.ID() {
		return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "credential belongs to another Node"}
	}
	node.auth.mu.Lock()
	if previous := node.auth.renewals[reference.ID]; previous != nil {
		node.auth.mu.Unlock()
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-previous.done:
			if previous.err != nil {
				return nil, previous.err
			}
		}
		return node.readProviderAuthorization(ctx, provider, reference)
	}
	renewal := &authRenewal{done: make(chan struct{})}
	node.auth.renewals[reference.ID] = renewal
	node.auth.mu.Unlock()
	err := node.tasks.Submit(func(owner context.Context) {
		err := node.runProviderRenewal(owner, provider, reference)
		node.auth.mu.Lock()
		renewal.err = err
		delete(node.auth.renewals, reference.ID)
		close(renewal.done)
		node.auth.mu.Unlock()
	})
	if err != nil {
		node.auth.mu.Lock()
		renewal.err = err
		delete(node.auth.renewals, reference.ID)
		close(renewal.done)
		node.auth.mu.Unlock()
		return nil, err
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-renewal.done:
		if renewal.err != nil {
			return nil, renewal.err
		}
	}
	return node.readProviderAuthorization(ctx, provider, reference)
}

func (node *Node) readProviderAuthorization(ctx context.Context, provider protocol.ConversationProvider, reference protocol.CredentialRef) (*protocol.Secret, error) {
	var secret *protocol.Secret
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		secret, err = store.ReadProviderAuthorization(ctx, db, node.ID(), provider, reference)
		return err
	})
	return secret, err
}

func (node *Node) runProviderRenewal(ctx context.Context, provider protocol.ConversationProvider, reference protocol.CredentialRef) error {
	var renewal *store.ProviderRenewal
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		renewal, err = store.BeginProviderRenewal(ctx, db, node.ID(), provider, reference, time.Now())
		return err
	}); err != nil || renewal == nil {
		return err
	}
	target := renewal.Capture.Target
	bundle, err := node.providerBundle(ctx, target)
	var grant providers.AccountGrant
	if err == nil && target.Declaration.Flow.Kind == "script" {
		var previous any
		body, _ := json.Marshal(renewal.Grant)
		_ = json.Unmarshal(body, &previous)
		result, operationErr := authorization.Run(ctx, authorization.Input{
			Declaration: target.Declaration, Fields: target.Fields, Bundle: bundle, Previous: previous, HostID: node.auth.host,
			Client: node.auth.client,
		})
		err = operationErr
		if err == nil {
			grant, err = providerLoginGrant(renewal.Capture, result)
		}
	} else if err == nil {
		fields := maps.Clone(target.Fields)
		for name, value := range renewal.Grant.Record.Issued.Fields() {
			fields[name] = value
		}
		status, document, operationErr := authorization.Response(ctx, node.auth.client, target.Declaration.Refresh.Request, fields, false)
		err = operationErr
		if err == nil && (status < 200 || status >= 300) {
			err = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider authorization renewal was rejected"}
		}
		if err == nil {
			if target.Declaration.Exchange != nil {
				body, operationErr := plugins.Translate(ctx, *bundle, target.Declaration.Exchange.Handler, document, 64*1024, time.Minute)
				err = operationErr
				if err == nil {
					err = json.Unmarshal(body, &document)
				}
			}
			if err == nil {
				issued, operationErr := authorization.ParseIssued(document, target.Declaration.Refresh.Token, uint64(time.Now().UnixMilli()), &renewal.Grant.Record.Issued)
				err = operationErr
				grant = providers.AccountGrant{Kind: protocol.AuthenticationPlugin, Record: &providers.AccountRecord{Target: target, Issued: issued}}
			}
		}
	}
	if err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "provider authorization renewal did not complete; sign in again"}
	}
	var changes []protocol.Event
	recording := context.WithoutCancel(node.ctx)
	err = node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
		var err error
		changes, err = store.CompleteProviderRenewal(recording, db, node.ID(), *renewal, grant)
		if err == nil {
			_, err = store.RecordEvents(recording, db, node.ID(), changes)
		}
		return err
	})
	if err == nil {
		node.pluginCommitted(changes)
	}
	return err
}
