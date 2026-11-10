package live

import (
	"context"
	"reflect"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) observePluginCardLogin(card *pluginCardInstance, attempt protocol.PluginConnectorAttempt) {
	login := &card.login
	if login.observing {
		return
	}
	if login.ctx == nil || login.ctx.Err() != nil {
		login.generation++
		login.ctx, login.cancel = context.WithCancel(card.ctx)
	}
	login.attempt, login.observing = &attempt, true
	generation, epoch, ctx := card.generation, login.generation, login.ctx
	if !card.owner.observe(func() {
		var previous *protocol.PluginConnectorLoginUpdate
		delay := 250 * time.Millisecond
		for ctx.Err() == nil {
			stream, err := card.owner.controller.Subscribe(ctx, protocol.Topic{Kind: "connector_login", Data: attempt.ID})
			if err == nil {
				for ctx.Err() == nil {
					update, failure := stream.Next(ctx)
					if failure != nil {
						err = failure
						break
					}
					value, valid := detail[protocol.PluginConnectorLoginUpdate](update.Data)
					if update.Kind != "connector_login" || !valid || value.Attempt != attempt || value.Revision == 0 {
						err = pluginNativeFault(protocol.ErrorCodeInvalidRequest, "card authorization update belongs to another attempt")
						break
					}
					if previous != nil {
						if value.Revision < previous.Revision {
							continue
						}
						if value.Revision == previous.Revision && !reflect.DeepEqual(value, *previous) {
							err = pluginNativeFault(protocol.ErrorCodeInvalidRequest, "card authorization revision changed its contents")
							break
						}
					}
					previous = &value
					r.enqueue(func(view *ui.Context) {
						if r.pluginCardLive(card, generation) && card.login.generation == epoch {
							r.applyPluginCardLogin(view, card, value)
						}
					})
					delay = 250 * time.Millisecond
					if value.State.Kind == "connected" || value.State.Kind == "cancelled" || value.State.Kind == "failed" {
						_ = stream.Close()
						return
					}
				}
				_ = stream.Close()
			}
			if ctx.Err() != nil {
				return
			}
			retry := faultCode(err) == protocol.ErrorCodeUnavailable || faultCode(err) == protocol.ErrorCodeBusy
			r.enqueue(func(*ui.Context) {
				if r.pluginCardLive(card, generation) && card.login.generation == epoch {
					card.login.observing, card.login.url, card.login.browser = retry, "", nil
					card.login.state = "unconfirmed"
					r.pluginCardLoginState(card)
				}
			})
			if !retry {
				return
			}
			timer := time.NewTimer(delay)
			select {
			case <-ctx.Done():
				timer.Stop()
				return
			case <-timer.C:
			}
			delay = min(delay*2, 5*time.Second)
		}
	}) {
		login.observing = false
	}
}

func (r *Runtime) applyPluginCardLogin(view *ui.Context, card *pluginCardInstance, update protocol.PluginConnectorLoginUpdate) {
	login := &card.login
	switch update.State.Kind {
	case "starting":
		login.state = "starting"
	case "browser":
		browser, valid := detail[protocol.PluginConnectorLoginStateBrowserPayload](update.State.Data)
		if !valid {
			return
		}
		login.url, login.expires, login.state = browser.Url, browser.ExpiresAtMs, "pending"
		r.receivePluginCardCallback(card)
		if requested := login.browser; requested != nil {
			login.browser = nil
			r.openPluginCardBrowser(view, card, requested.target, requested.mode)
		}
	case "exchanging":
		login.url, login.state = "", "exchanging"
	case "connected":
		connection, valid := detail[protocol.PluginConnectorConnection](update.State.Data)
		if !valid || connection.Package.Name != card.summary.Package.Name || connection.Package.Digest != card.summary.Package.Digest {
			return
		}
		notify := login.state != "connected"
		card.updateReference(connection.Package)
		login.suspend()
		login.attempt, login.request, login.state = nil, nil, "connected"
		if notify {
			r.pluginCardToast(view, i18n.T(view, "connector_connected"), false)
		}
	case "cancelled":
		login.suspend()
		login.attempt, login.state = nil, "cancelled"
	case "failed":
		login.suspend()
		login.attempt, login.state = nil, "failed"
		r.pluginCardToast(view, i18n.T(view, "mcp_oauth_failed"), true)
	default:
		login.url, login.state = "", "pending"
	}
	r.pluginCardLoginState(card)
}

func (r *Runtime) receivePluginCardCallback(card *pluginCardInstance) {
	login := &card.login
	if login.receiving || login.returned == nil || login.redirect == nil {
		return
	}
	login.receiving = true
	ctx, returned, generation, epoch := login.ctx, login.returned, card.generation, login.generation
	if !card.owner.observe(func() {
		select {
		case <-ctx.Done():
			return
		case callback := <-returned:
			r.enqueue(func(*ui.Context) {
				if !r.pluginCardLive(card, generation) || card.login.generation != epoch {
					return
				}
				login := &card.login
				login.receiving, login.returned, login.redirect, login.url = false, nil, nil, ""
				if login.attempt == nil {
					return
				}
				request := card.owner.controller.Prepare(protocol.Command{Kind: "submit_connector_input", Data: protocol.CommandSubmitConnectorInputPayload{
					Attempt: login.attempt.ID, Input: protocol.NewSecret(callback),
				}})
				scope := card.scope()
				request.Plugin = &scope
				login.request = &request
				r.pluginCardLoginState(card)
				r.sendPluginCardLogin(card, false)
			})
		}
	}) {
		login.receiving = false
	}
}
