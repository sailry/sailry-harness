package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"maps"
	"net"
	"net/http"
	"strings"
	"sync/atomic"
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
	// Provider accounts belong to the controller. Connector commands under a
	// package scope were confined to that package by the plugin authority at
	// admission (store.CheckPluginRequest, store.CheckPluginCardRequest).
	if request.Plugin != nil && strings.HasSuffix(request.Command.Kind, "_provider_login") {
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
			if user, ok := store.UserFromContext(ctx); ok {
				owner = store.WithUser(owner, user)
			}
			if user, ok := store.CapturedUserFromContext(ctx); ok {
				owner = store.WithCapturedUser(owner, user)
			}
			operation, cancel := context.WithCancel(owner)
			defer cancel()
			node.auth.mu.Lock()
			entry.cancel = cancel
			stopped := entry.stopped || node.auth.closed
			node.auth.mu.Unlock()
			if stopped {
				return
			}
			operation = node.plugins.memory.Context(operation, capture.Target.Package, "callback", pluginMemoryUser(operation))
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
				recording := context.WithoutCancel(ctx)
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
			node.pluginCommitted(changes, ctx, nil)
			for _, change := range changes {
				if change.Kind == "credential_changed" {
					if changed, ok := dispatchPayload[protocol.EventCredentialChangedPayload](change.Data); ok {
						node.auth.mu.Lock()
						entry.provider.Credential = &changed.Credential
						node.auth.mu.Unlock()
					}
				}
			}
			for _, change := range changes {
				if change.Kind != "provider_changed" {
					continue
				}
				if provider, ok := dispatchPayload[protocol.ConversationProvider](change.Data); ok {
					if err := node.discoverProviderLoginModels(operation, caller, provider); err != nil {
						failure := pluginFailure(err)
						node.auth.mu.Lock()
						entry.provider.ModelError = failure
						node.auth.mu.Unlock()
					}
				}
				break
			}
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
		return protocol.NewConfigurationFault(protocol.ConfigurationLoginRequired, "provider sign-in is required")
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
		return protocol.NewConfigurationFault(protocol.ConfigurationLoginRequired, "provider sign-in is required")
	}
	secret, effective, err := node.awaitProviderAuthorization(ctx, *invocation.Provider, func() (protocol.CredentialRef, error) {
		effective := *reference
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			effective, err = store.EffectiveTurnCredential(ctx, db, invocation.Run.Turn, *reference)
			return err
		})
		return effective, err
	}, func(effective protocol.CredentialRef, waiting bool) error {
		var state *protocol.ConversationRunWaiting
		if waiting {
			state = &protocol.ConversationRunWaiting{Kind: "controller_login", Credential: effective, Provider: invocation.Provider.ID}
		}
		return node.setCredentialWait(context.WithoutCancel(ctx), invocation.Run.Turn, state)
	})
	if err == nil {
		invocation.Secret, invocation.AuthorizationFailure = secret, nil
		invocation.EffectiveCredential = &effective
	}
	return err
}

func controllerLoginWaitable(err error) bool {
	var failure *protocol.Fault
	if !errors.As(err, &failure) {
		return false
	}
	if failure.Code == protocol.ErrorCodeExpired {
		return true
	}
	if failure.Details != nil && failure.Details.Kind == "controller_login" {
		if detail, ok := dispatchPayload[protocol.FaultDetailsControllerLoginPayload](failure.Details.Data); ok {
			return detail.Reason == "renewal_uncertain" || detail.Reason == "renewal_rejected"
		}
	}
	return false
}

func (node *Node) awaitProviderAuthorization(ctx context.Context, provider protocol.ConversationProvider, resolve func() (protocol.CredentialRef, error), project func(protocol.CredentialRef, bool) error) (*protocol.Secret, protocol.CredentialRef, error) {
	waiting := false
	var reference, waitingReference protocol.CredentialRef
	defer func() {
		if waiting {
			_ = project(waitingReference, false)
		}
	}()
	for {
		// Subscribe before checking the cache: a commit between the read and
		// waiting must still wake this exact, already admitted model step.
		node.auth.mu.Lock()
		changed, closed := node.auth.changed, node.auth.closed
		node.auth.mu.Unlock()
		if closed {
			return nil, reference, context.Canceled
		}
		var err error
		reference, err = resolve()
		if err != nil {
			return nil, reference, err
		}
		secret, err := node.providerAuthorization(ctx, provider, reference)
		if err == nil {
			return secret, reference, nil
		}
		if !controllerLoginWaitable(err) {
			return nil, reference, err
		}
		// A newer projection of this same credential may add the trusted
		// identity an older executor cache lacked when the wait began.
		if err := project(reference, true); err != nil {
			return nil, reference, err
		}
		waiting, waitingReference = true, reference
		select {
		case <-ctx.Done():
			return nil, reference, ctx.Err()
		case <-changed:
		}
	}
}

func (node *Node) providerAuthorization(ctx context.Context, provider protocol.ConversationProvider, reference protocol.CredentialRef) (*protocol.Secret, error) {
	if reference.Node != node.ID() {
		return node.readProviderAuthorization(ctx, provider, reference)
	}
	if err := node.renewProviderCredential(ctx, provider, reference, false); err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		if secret, readErr := node.readProviderAuthorization(ctx, provider, reference); readErr == nil {
			return secret, nil
		} else if controllerLoginWaitable(readErr) {
			return nil, readErr
		}
		return nil, err
	}
	return node.readProviderAuthorization(ctx, provider, reference)
}

func (node *Node) renewProviderCredential(ctx context.Context, provider protocol.ConversationProvider, reference protocol.CredentialRef, scheduled bool) error {
	for ctx.Err() == nil {
		node.auth.mu.Lock()
		renewal := node.auth.renewals[reference.ID]
		start := renewal == nil
		if start {
			renewal = &authRenewal{done: make(chan struct{})}
			node.auth.renewals[reference.ID] = renewal
		}
		node.auth.mu.Unlock()
		if start {
			finish := func(err error) {
				node.auth.mu.Lock()
				renewal.err = err
				delete(node.auth.renewals, reference.ID)
				close(renewal.done)
				node.auth.mu.Unlock()
			}
			if err := node.tasks.Submit(func(owner context.Context) {
				if user, ok := store.UserFromContext(ctx); ok {
					owner = store.WithUser(owner, user)
				}
				if user, ok := store.CapturedUserFromContext(ctx); ok {
					owner = store.WithCapturedUser(owner, user)
				}
				finish(node.runProviderRenewal(owner, provider, reference, scheduled))
			}); err != nil {
				finish(err)
			}
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-renewal.done:
			// A turn may join a scheduled renewal that made no request
			// because the account was no longer selected. It still has
			// authority to renew the credential captured at admission.
			if scheduled || !errors.Is(renewal.err, store.ErrProviderRenewalNotSelected) {
				return renewal.err
			}
		}
	}
	return ctx.Err()
}

func (node *Node) setCredentialWait(ctx context.Context, turn protocol.TurnID, waiting *protocol.ConversationRunWaiting) error {
	var changes []protocol.Event
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		changes, err = store.SetRunCredentialWait(ctx, db, node.ID(), turn, waiting)
		if err == nil && len(changes) != 0 {
			_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		}
		return err
	})
	if err == nil && len(changes) != 0 {
		node.publish()
		// Recovery preflight also waits on this signal. A normal renewal
		// can clear the login wait after the credential's own wake has
		// already been consumed, so publish the state transition as well.
		node.authChanged()
	}
	return err
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

func (node *Node) runProviderRenewal(ctx context.Context, provider protocol.ConversationProvider, reference protocol.CredentialRef, scheduled bool) error {
	begin := store.BeginProviderRenewal
	if scheduled {
		begin = store.BeginScheduledProviderRenewal
	}
	var renewal *store.ProviderRenewal
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		renewal, err = begin(ctx, db, node.ID(), provider, reference, time.Now())
		return err
	}); err != nil || renewal == nil {
		return err
	}
	target := renewal.Capture.Target
	ctx = node.plugins.memory.Context(ctx, target.Package, "callback", pluginMemoryUser(ctx))
	bundle, err := node.providerBundle(ctx, target)
	transport := &renewalTransport{base: node.auth.client.Transport}
	client := *node.auth.client
	client.Transport = transport
	var grant providers.AccountGrant
	if err == nil && target.Declaration.Flow.Kind == "script" {
		var previous any
		body, _ := json.Marshal(renewal.Grant)
		_ = json.Unmarshal(body, &previous)
		result, operationErr := authorization.Run(ctx, authorization.Input{
			Declaration: target.Declaration, Fields: target.Fields, Bundle: bundle, Previous: previous, HostID: node.auth.host,
			Client: &client,
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
		// The shared client has no timeout of its own.
		operation, cancel := context.WithTimeout(ctx, authorization.Deadline)
		status, document, operationErr := authorization.Response(operation, &client, target.Declaration.Refresh.Request, fields, false)
		cancel()
		err = operationErr
		if err == nil {
			err = authorization.Granted(status, document)
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
	var failure error
	if err != nil {
		var fault *protocol.Fault
		copilot := renewal.Grant.Kind == protocol.AuthenticationCopilot
		rejected := errors.As(err, &fault) && fault.Code == protocol.ErrorCodePermissionDenied
		switch {
		case !transport.sent.Load():
			// No request left this Node, so the grant was not consumed. It returns
			// without its uncertain marker and a later use renews it again.
			grant, failure = renewal.Grant, err
		case rejected:
			// The provider refused the grant. It stays stored and marked, so later
			// uses ask for a new sign-in instead of renewing again.
			grant, failure = renewal.Grant, err
			grant.RenewalRejected = true
		case copilot:
			// The grant was never marked: a later use repeats the exchange.
			return err
		default:
			_ = node.profile.Store.Node.Write(context.WithoutCancel(ctx), func(db *sql.Conn) error {
				return store.MarkProviderRenewalUncertain(context.WithoutCancel(ctx), db, renewal.Capture.Provider, renewal.Credential)
			})
			node.authChanged()
			return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "provider authorization renewal did not complete; sign in again"}
		}
	}
	var changes []protocol.Event
	recording := context.WithoutCancel(ctx)
	err = node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
		var err error
		changes, err = store.CompleteProviderRenewal(recording, db, node.ID(), *renewal, grant)
		if err == nil {
			_, err = store.RecordEvents(recording, db, node.ID(), changes)
		}
		return err
	})
	if err == nil {
		node.pluginCommitted(changes, ctx, nil)
		node.authChanged()
		err = failure
	}
	return err
}

// renewalTransport records whether any request of one renewal got past
// connection setup.
type renewalTransport struct {
	base http.RoundTripper
	sent atomic.Bool
}

func (transport *renewalTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	base := transport.base
	if base == nil {
		base = http.DefaultTransport
	}
	response, err := base.RoundTrip(request)
	var dial *net.OpError
	if err == nil || !errors.As(err, &dial) || dial.Op != "dial" {
		transport.sent.Store(true)
	}
	return response, err
}
