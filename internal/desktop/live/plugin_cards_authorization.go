package live

import (
	"context"
	"errors"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginCardBrowserRequest struct {
	target string
	mode   protocol.PluginCardMode
}

type pluginCardAuthorization struct {
	ctx        context.Context
	cancel     context.CancelFunc
	generation uint64
	attempt    *protocol.PluginConnectorAttempt
	request    *protocol.Request
	pending    bool
	observing  bool
	receiving  bool
	returned   <-chan string
	redirect   *string
	url        string
	expires    uint64
	state      string
	browser    *pluginCardBrowserRequest
}

func (login *pluginCardAuthorization) canOpen(now time.Time) bool {
	return login.state == "pending" && login.redirect != nil && login.returned != nil &&
		login.url != "" && login.expires > uint64(max(now.UnixMilli(), 0))
}

func (login *pluginCardAuthorization) canCancel() bool {
	return login.attempt != nil && login.request == nil && !login.pending
}

func (login *pluginCardAuthorization) keepsAlive(now time.Time) bool {
	return login.pending || login.canOpen(now)
}

func (login *pluginCardAuthorization) suspend() {
	if login.cancel != nil {
		login.cancel()
	}
	login.generation++
	login.ctx, login.cancel, login.returned, login.redirect, login.browser = nil, nil, nil, nil, nil
	login.url, login.expires, login.pending, login.observing, login.receiving = "", 0, false, false, false
	if login.request != nil {
		login.state = "unconfirmed"
	}
}

func (r *Runtime) pluginCardAction(view *ui.Context, card *pluginCardInstance, identity string, action desktopplugins.CardAction) {
	if !r.pluginCardEligible(card) || !card.visible || card.host == nil || !card.host.Permits(action) {
		r.pluginCardToast(view, i18n.T(view, "plugins_failed"), true)
		return
	}
	switch action.Kind {
	case "connect":
		if action.Target != nil && action.Mode != nil {
			card.login.browser = &pluginCardBrowserRequest{target: *action.Target, mode: *action.Mode}
		} else {
			card.login.browser = nil
		}
		r.beginPluginCardLogin(view, card)
	case "cancel":
		r.cancelPluginCardLogin(card)
	case "refresh":
		card.host.Refresh(identity)
	case "settings":
		r.openPluginCardSettings(card)
	case "browser":
		r.openPluginCardBrowser(view, card, *action.Target, *action.Mode)
	}
}

func (r *Runtime) pluginCardLoginState(card *pluginCardInstance) {
	if card.host == nil {
		return
	}
	state := card.login.state
	if state == "" {
		state = "idle"
	}
	if card.login.request != nil && !card.login.pending {
		state = "unconfirmed"
	}
	card.host.Login(state, card.login.canOpen(time.Now()), card.login.canCancel())
}

func (r *Runtime) beginPluginCardLogin(view *ui.Context, card *pluginCardInstance) {
	login := &card.login
	if login.pending {
		return
	}
	if login.request != nil {
		r.sendPluginCardLogin(card, true)
		return
	}
	if login.attempt != nil {
		if login.redirect == nil {
			login.browser = nil
		}
		r.observePluginCardLogin(card, *login.attempt)
		return
	}
	if card.declaration.McpServer == nil {
		return
	}
	if login.cancel != nil {
		login.cancel()
	}
	login.generation++
	login.ctx, login.cancel = context.WithCancel(card.ctx)
	redirect, returned, _, err := localAuthorization(login.ctx, callbackPresentation())
	if err != nil || redirect == nil {
		login.suspend()
		login.state = "failed"
		r.pluginCardLoginState(card)
		r.pluginCardToast(view, i18n.T(view, "mcp_oauth_callback_failed"), true)
		return
	}
	login.redirect, login.returned, login.state, login.pending = redirect, returned, "starting", true
	r.pluginCardLoginState(card)
	generation, epoch, ctx := card.generation, login.generation, login.ctx
	server, scope := *card.declaration.McpServer, card.scope()
	if !card.owner.async(func() {
		request, err := card.prepareLogin(ctx, scope, server, *redirect)
		r.enqueue(func(current *ui.Context) {
			if !r.pluginCardLive(card, generation) || card.login.generation != epoch {
				return
			}
			card.login.pending = false
			if err != nil {
				card.login.suspend()
				card.login.state = "failed"
				r.pluginCardLoginState(card)
				r.pluginCardToast(current, i18n.T(current, "mcp_oauth_failed"), true)
				return
			}
			card.login.request = &request
			r.sendPluginCardLogin(card, false)
		})
	}) {
		login.suspend()
	}
}

func (card *pluginCardInstance) prepareLogin(ctx context.Context, scope protocol.PluginContext, server, redirect string) (protocol.Request, error) {
	output, err := card.owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: scope.Package.Name}})
	if err != nil {
		return protocol.Request{}, err
	}
	info, valid := detail[protocol.PluginInfo](output.Data)
	if output.Kind != "plugin" || !valid || !info.Summary.Enabled || packageReference(info.Summary) != scope.Package {
		return protocol.Request{}, pluginNativeFault(protocol.ErrorCodeRevisionConflict, "plugin configuration changed")
	}
	connector, err := pluginCardConnector(info, server)
	if err != nil {
		return protocol.Request{}, err
	}
	output, err = card.execute(ctx, protocol.Command{Kind: "list_connections"})
	if err != nil {
		return protocol.Request{}, err
	}
	connections, valid := detail[[]protocol.PluginConnectorConnection](output.Data)
	if output.Kind != "connections" || !valid {
		return protocol.Request{}, errors.New("card connections response expected")
	}
	input := protocol.DefaultPluginConnectorInput()
	input.Redirect = &redirect
	payload := protocol.CommandStartConnectorLoginPayload{
		Package: scope.Package, Connector: connector, Method: protocol.PluginConnectorKindMcpOauth, Input: input,
	}
	for _, connection := range connections {
		if connection.Package.Name == scope.Package.Name && connection.Connector == connector {
			payload.Connection, payload.ExpectedRevision = &connection.ID, &connection.Revision
			break
		}
	}
	request := card.owner.controller.Prepare(protocol.Command{Kind: "start_connector_login", Data: payload})
	request.Plugin = &scope
	return request, nil
}

func pluginCardConnector(info protocol.PluginInfo, server string) (string, error) {
	found := ""
	if info.Extension != nil {
		for _, connector := range info.Extension.Connectors {
			matched := false
			for _, auth := range connector.Auth {
				if auth.Kind == "mcp_oauth" {
					method, valid := detail[protocol.PluginConnectorAuthMcpOauthPayload](auth.Data)
					matched = matched || valid && method.Server == server
				}
			}
			if matched {
				if found != "" {
					return "", pluginNativeFault(protocol.ErrorCodeInvalidRequest, "card connector is ambiguous")
				}
				found = connector.ID
			}
		}
	}
	if found == "" {
		return "", pluginNativeFault(protocol.ErrorCodeNotConfigured, "card connector is unavailable")
	}
	return found, nil
}

func (r *Runtime) cancelPluginCardLogin(card *pluginCardInstance) {
	login := &card.login
	if login.pending {
		return
	}
	if login.request != nil {
		r.sendPluginCardLogin(card, true)
		return
	}
	if login.attempt == nil {
		return
	}
	request := card.owner.controller.Prepare(protocol.Command{Kind: "cancel_connector_login", Data: protocol.CommandCancelConnectorLoginPayload{Attempt: login.attempt.ID}})
	scope := card.scope()
	request.Plugin = &scope
	login.request = &request
	r.sendPluginCardLogin(card, false)
}

func (r *Runtime) sendPluginCardLogin(card *pluginCardInstance, inspect bool) {
	login := &card.login
	if login.pending || login.request == nil || card.ctx == nil || card.ctx.Err() != nil {
		return
	}
	login.pending = true
	r.pluginCardLoginState(card)
	request, generation, epoch, ctx := *login.request, card.generation, login.generation, card.ctx
	if !card.owner.async(func() {
		var output protocol.Output
		var err error
		confirmed := false
		if inspect {
			outcome, failure := card.owner.controller.Outcome(ctx, request)
			output, err, confirmed = pluginCardInspected(outcome, failure)
		} else {
			output, err = card.owner.controller.Execute(ctx, request)
			confirmed = err == nil || !pluginCardUncertain(faultCode(err))
		}
		r.enqueue(func(view *ui.Context) {
			if !r.pluginCardLive(card, generation) || card.login.generation != epoch {
				return
			}
			login := &card.login
			login.pending = false
			attempt, valid := detail[protocol.PluginConnectorAttempt](output.Data)
			if err == nil && output.Kind == "connector_login" && valid {
				login.request = nil
				switch request.Command.Kind {
				case "start_connector_login":
					r.observePluginCardLogin(card, attempt)
				case "cancel_connector_login":
					login.suspend()
					login.attempt, login.state = nil, "cancelled"
				}
			} else if err != nil && confirmed {
				login.request = nil
				login.suspend()
				login.state = "failed"
				r.pluginCardToast(view, i18n.T(view, "mcp_oauth_failed"), true)
			} else {
				login.state = "unconfirmed"
				login.url, login.browser = "", nil
				kit.Toast(view, kit.Uncertain, i18n.T(view, "plugins_unknown"))
			}
			r.pluginCardLoginState(card)
		})
	}) {
		login.pending = false
	}
}

func pluginCardUncertain(code protocol.ErrorCode) bool {
	switch code {
	case protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy, protocol.ErrorCodeInternal, protocol.ErrorCodeCancelled:
		return true
	}
	return false
}

func pluginCardInspected(outcome protocol.RequestOutcome, err error) (protocol.Output, error, bool) {
	if err != nil {
		return protocol.Output{}, err, false
	}
	if outcome.Kind == "completed" {
		result, valid := detail[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
		if valid {
			if result.Err != nil {
				return protocol.Output{}, result.Err, true
			}
			if result.Ok != nil {
				return *result.Ok, nil, true
			}
		}
	}
	return protocol.Output{}, pluginNativeFault(protocol.ErrorCodeOutcomeUnknown, "login outcome is unknown"), false
}
