package live

import (
	"context"
	"maps"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func connectorPresentation(declaration protocol.PluginConnectorDeclaration, method protocol.PluginConnectorKind, registration bool) *protocol.PluginModelAuthPresentation {
	for _, auth := range declaration.Auth {
		if auth.Kind != string(method) {
			continue
		}
		switch auth.Kind {
		case "mcp_oauth":
			return callbackPresentation()
		case "oauth2":
			value, ok := detail[protocol.PluginConnectorAuthOauth2Payload](auth.Data)
			if ok && value.Flow == protocol.PluginConnectorFlowPkce {
				return callbackPresentation()
			}
		case "application":
			value, ok := detail[protocol.PluginConnectorAuthApplicationPayload](auth.Data)
			if ok && registration && value.Registration != nil && *value.Registration != nil {
				flow := (**value.Registration).Flow
				if flow.Kind == "pkce" {
					pkce, _ := detail[protocol.PluginModelAuthFlowPkcePayload](flow.Data)
					return &protocol.PluginModelAuthPresentation{Kind: "pkce", Data: protocol.PluginModelAuthPresentationPkcePayload{Redirect: pkce.Redirect}}
				}
				if flow.Kind == "script" {
					script, _ := detail[protocol.PluginModelAuthFlowScriptPayload](flow.Data)
					return &protocol.PluginModelAuthPresentation{Kind: "script", Data: protocol.PluginModelAuthPresentationScriptPayload{Callback: script.Callback}}
				}
			}
		}
	}
	return nil
}

func (r *Runtime) saveConnector(draft settings.ConnectorDraft, done func(bool)) {
	kind, exists := r.authorizations.types[draft.Type]
	if !exists {
		if done != nil {
			done(false)
		}
		return
	}
	method := protocol.PluginConnectorKindSecret
	for _, auth := range kind.Declaration.Auth {
		if connectorMethod(protocol.PluginConnectorKind(auth.Kind)) == draft.Method {
			method = protocol.PluginConnectorKind(auth.Kind)
			break
		}
	}
	input := protocol.PluginConnectorInput{Fields: map[string]string{}, Secrets: map[string]protocol.Secret{}, Registration: draft.Registration}
	for _, field := range kind.Declaration.Fields {
		value, present := draft.Fields[field.Name]
		if field.Secret {
			if present && value != "" {
				input.Secrets[field.Name] = protocol.NewSecret(value)
			}
		} else if present {
			input.Fields[field.Name] = value
		}
	}
	connection, existing := r.authorizations.connections[draft.Service]
	// An existing connection saves its fields as they are. One whose method
	// authorizes elsewhere does so only when asked to (draft.Update), and
	// authorizes again when the Node drops its authorization for the change.
	if existing && (method == protocol.PluginConnectorKindSecret || method == protocol.PluginConnectorKindApplication || draft.Update) && !draft.Registration {
		r.execute(protocol.Command{Kind: "update_connection", Data: protocol.CommandUpdateConnectionPayload{Connection: connection.ID, ExpectedRevision: draft.ExpectedRevision, Input: input}}, func(c *ui.Context, output protocol.Output, err error) {
			if err != nil {
				r.connectorError(c, err)
				if done != nil {
					done(false)
				}
				return
			}
			if saved, ok := detail[protocol.PluginConnectorConnection](output.Data); ok {
				r.authorizations.connections[draft.Service] = saved
				if saved.State == protocol.PluginConnectorStateNotConnected {
					draft.ExpectedRevision = saved.Revision
					r.beginConnectorAuthorization(draft, kind, method, input, done)
					return
				}
			}
			r.projectConnectors()
			kit.Toast(c, kit.Done, i18n.T(c, "connector_saved"))
			if done != nil {
				done(true)
			}
		})
		return
	}
	r.beginConnectorAuthorization(draft, kind, method, input, done)
}

func (r *Runtime) beginConnectorAuthorization(draft settings.ConnectorDraft, kind protocol.PluginConnectorType, method protocol.PluginConnectorKind, input protocol.PluginConnectorInput, done func(bool)) {
	if previous := r.authorizations.connector; previous != nil {
		r.cancelConnectorLogin()
		previous.cancel()
	}
	ctx, cancel := context.WithCancel(r.ctx)
	redirect, returned, manual, err := localAuthorization(ctx, connectorPresentation(kind.Declaration, method, draft.Registration))
	if err != nil {
		cancel()
		r.enqueue(func(c *ui.Context) { r.connectorError(c, err) })
		if done != nil {
			done(false)
		}
		return
	}
	input.Redirect = redirect
	attempt := &liveAuthorization{ctx: ctx, cancel: cancel, connector: draft, title: kind.Declaration.Display.Label}
	attempt.connector.Fields = maps.Clone(draft.Fields)
	r.authorizations.connector = attempt
	actions := r.App.Settings.ConnectorActions
	actions.Authorization = settings.Authorization{Open: true, Title: attempt.title, Flow: settings.FlowStarting, Callback: manual, Cancel: r.cancelConnectorLogin,
		OpenURL: func(value string) { r.enqueue(func(c *ui.Context) { c.OpenURL(value) }) }}
	payload := protocol.CommandStartConnectorLoginPayload{Package: kind.Package, Connector: kind.Declaration.ID, Method: method, Input: input}
	if connection, exists := r.authorizations.connections[draft.Service]; exists {
		payload.Connection, payload.ExpectedRevision = &connection.ID, &draft.ExpectedRevision
	}
	r.execute(protocol.Command{Kind: "start_connector_login", Data: payload}, func(c *ui.Context, output protocol.Output, err error) {
		if r.authorizations.connector != attempt {
			return
		}
		if done != nil {
			done(err == nil)
		}
		if err != nil {
			cancel()
			actions.Authorization.Flow = settings.FlowFailed
			r.connectorError(c, err)
			return
		}
		value, ok := detail[protocol.PluginConnectorAttempt](output.Data)
		if !ok {
			cancel()
			return
		}
		attempt.id = value.ID
		r.watchConnectorLogin(ctx, attempt)
		if returned != nil {
			r.observe(func() {
				select {
				case <-ctx.Done():
				case callback := <-returned:
					r.enqueue(func(*ui.Context) {
						if r.authorizations.connector == attempt {
							r.submitConnectorLogin(callback)
						}
					})
				}
			})
		}
	})
}

func (r *Runtime) watchConnectorLogin(ctx context.Context, attempt *liveAuthorization) {
	r.observe(func() {
		stream, err := r.controller.Subscribe(ctx, protocol.Topic{Kind: "connector_login", Data: attempt.id})
		if err != nil {
			r.enqueue(func(c *ui.Context) { r.connectorError(c, err) })
			return
		}
		defer stream.Close()
		for {
			update, err := stream.Next(ctx)
			if err != nil {
				return
			}
			value, ok := detail[protocol.PluginConnectorLoginUpdate](update.Data)
			if !ok {
				continue
			}
			r.enqueue(func(c *ui.Context) {
				if r.authorizations.connector != attempt {
					return
				}
				state := protocol.ConversationLoginState{Kind: value.State.Kind, Data: value.State.Data}
				if value.State.Kind == "device" {
					device, _ := detail[protocol.PluginConnectorLoginStateDevicePayload](value.State.Data)
					state.Kind, state.Data = "pending", protocol.ConversationLoginStatePendingPayload{VerificationUrl: device.Url, UserCode: device.Code, ExpiresAtMs: device.ExpiresAtMs}
				} else if value.State.Kind == "browser" {
					browser, _ := detail[protocol.PluginConnectorLoginStateBrowserPayload](value.State.Data)
					state.Data = protocol.ConversationLoginStateBrowserPayload{AuthorizationUrl: browser.Url, ExpiresAtMs: browser.ExpiresAtMs}
				}
				view := &r.App.Settings.ConnectorActions.Authorization
				expiry, terminal := projectAuthorization(view, state)
				if state.Kind == "browser" && view.URL != "" {
					c.OpenURL(view.URL)
				}
				if state.Kind == "failed" {
					failure, _ := detail[protocol.Fault](state.Data)
					r.connectorError(c, &failure)
				}
				if terminal {
					attempt.cancel()
				}
				if state.Kind == "connected" {
					key := "connector_connected"
					if attempt.connector.Service != "" || attempt.connector.Method == "connector_token" || attempt.connector.Method == "connector_application" {
						key = "connector_saved"
					} else if attempt.connector.Method == "connector_cli" {
						key = "connector_bound"
					}
					kit.Toast(c, kit.Done, i18n.T(c, key))
					if connection, ok := detail[protocol.PluginConnectorConnection](value.State.Data); ok {
						r.authorizations.connections[connectorTypeID(connection.Package, connection.Connector)] = connection
						r.projectConnectors()
						r.readConnectorPackages()
					}
				}
				if expiry != 0 {
					r.authorizationCountdown(attempt, view, expiry, false)
				}
			})
			if value.State.Kind == "connected" || value.State.Kind == "failed" || value.State.Kind == "cancelled" {
				return
			}
		}
	})
}

func (r *Runtime) submitConnectorLogin(callback string) {
	attempt := r.authorizations.connector
	if attempt == nil || attempt.id == (protocol.RequestID{}) {
		return
	}
	view := &r.App.Settings.ConnectorActions.Authorization
	view.Busy = true
	r.execute(protocol.Command{Kind: "submit_connector_input", Data: protocol.CommandSubmitConnectorInputPayload{Attempt: attempt.id, Input: protocol.NewSecret(callback)}}, func(c *ui.Context, _ protocol.Output, err error) {
		if r.authorizations.connector != attempt {
			return
		}
		view.Busy = false
		r.connectorError(c, err)
		if err == nil {
			view.Pasted = ""
		}
	})
}

func (r *Runtime) cancelConnectorLogin() {
	attempt := r.authorizations.connector
	if attempt == nil {
		return
	}
	view := &r.App.Settings.ConnectorActions.Authorization
	if attempt.id == (protocol.RequestID{}) {
		view.Open = false
		attempt.cancel()
		r.authorizations.connector = nil
		return
	}
	view.Cancelling = true
	r.execute(protocol.Command{Kind: "cancel_connector_login", Data: protocol.CommandCancelConnectorLoginPayload{Attempt: attempt.id}}, func(c *ui.Context, _ protocol.Output, err error) {
		if r.authorizations.connector != attempt {
			return
		}
		view.Cancelling = false
		r.connectorError(c, err)
		if err == nil {
			view.Open = false
			attempt.cancel()
		}
	})
}
