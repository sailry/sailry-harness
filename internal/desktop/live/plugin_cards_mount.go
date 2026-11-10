package live

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/egoist/mygo/ui"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) ensurePluginCard(view *ui.Context, card *pluginCardInstance) {
	if card.loading || card.mounted != nil || card.attempted || !card.visible || !r.pluginCardEligible(card) {
		return
	}
	state := r.pluginCards
	state.generation++
	card.generation, card.locale = state.generation, r.packageLocale()
	card.ctx, card.cancel = context.WithCancel(card.owner.ctx)
	card.loading, card.attempted, card.failure, card.reported = true, true, nil, false
	generation, ctx := card.generation, card.ctx
	reference, receipt := card.summary.Package, card.summary.Receipt
	if !card.owner.async(func() {
		output, err := card.owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin_version", Data: protocol.CommandReadPluginVersionPayload{Package: reference}})
		info, valid := detail[protocol.PluginInfo](output.Data)
		var declaration *protocol.PluginCardDeclaration
		if err == nil && output.Kind == "plugin" && valid && info.Extension != nil {
			for _, candidate := range info.Extension.Cards {
				if candidate.Key == card.summary.Key {
					declaration = &candidate
					break
				}
			}
		}
		if err == nil && (declaration == nil || !nodeplugins.CardAccepts(*declaration, card.summary.Props)) {
			err = errors.New("captured card declaration is unavailable")
		}
		var bundle protocol.PluginDesktopBundle
		if err == nil {
			output, err = card.owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin_view", Data: protocol.CommandReadPluginViewPayload{
				Package: reference, Surface: protocol.PluginDesktopSurface{Kind: "card", Data: receipt},
			}})
			bundle, valid = detail[protocol.PluginDesktopBundle](output.Data)
			if err == nil && (output.Kind != "plugin_view" || !valid || bundle.Package != reference || bundle.Entry != declaration.Entry) {
				err = errors.New("unexpected immutable card view response")
			}
		}
		r.enqueue(func(current *ui.Context) {
			if !r.pluginCardLive(card, generation) {
				return
			}
			card.loading = false
			if err != nil {
				r.failPluginCard(current, card, err, "plugin_initialization_failed")
				return
			}
			card.info, card.declaration = info, *declaration
			r.mountPluginCard(current, card, bundle)
		})
	}) {
		r.suspendPluginCard(card)
	}
}

func (r *Runtime) mountPluginCard(view *ui.Context, card *pluginCardInstance, bundle protocol.PluginDesktopBundle) {
	generation := card.generation
	ctx, declaration := card.ctx, card.declaration
	scope := card.scope()
	native := card.owner.pluginNativeBinding(ctx, scope, "")
	if native == nil {
		r.failPluginCard(view, card, errors.New("card native services are unavailable"), "plugin_initialization_failed")
		return
	}
	native.Invalidate = r.wake
	post := native.Post
	native.Post = func(operation func(*ui.Context)) bool {
		if ctx.Err() != nil {
			return false
		}
		return post(func(current *ui.Context) {
			if r.pluginCardLive(card, generation) {
				operation(current)
			}
		})
	}
	var host *desktopplugins.CardHost
	host, err := desktopplugins.NewCardHost(desktopplugins.CardBindings{
		Context: ctx, Summary: card.summary, Declaration: declaration, Locale: card.locale, Native: native,
		ValidateProps: func(value json.RawMessage) bool { return nodeplugins.CardAccepts(declaration, value) },
		Status: func(ctx context.Context) (protocol.PluginConnectionStatus, error) {
			return card.status(ctx, declaration.Key)
		},
		Action: func(identity string, action desktopplugins.CardAction) {
			r.enqueue(func(current *ui.Context) {
				if r.pluginCardLive(card, generation) && card.visible && card.host == host {
					r.pluginCardAction(current, card, identity, action)
				}
			})
		},
		Toast: func(message string, failed bool) {
			r.enqueue(func(current *ui.Context) {
				if r.pluginCardLive(card, generation) {
					r.pluginCardToast(current, message, failed)
				}
			})
		},
	})
	if err != nil {
		r.failPluginCard(view, card, err, "plugin_initialization_failed")
		return
	}
	card.host = host
	runtime, err := desktopplugins.NewRuntime(desktopplugins.RuntimeBindings{
		Modules: host.Modules(), Components: host.Components(), Invalidate: r.wake, BindActions: native.BindActions,
		Post: func(operation func()) {
			r.enqueue(func(*ui.Context) {
				if r.pluginCardLive(card, generation) {
					operation()
				}
			})
		},
	})
	if err != nil {
		r.failPluginCard(view, card, err, "plugin_initialization_failed")
		return
	}
	card.runtime = runtime
	runtimeScope := scope
	runtimeScope.Package = bundle.Package
	card.mounted, err = runtime.Mount(bundle, runtimeScope)
	if err != nil {
		r.failPluginCard(view, card, err, "plugin_initialization_failed")
		return
	}
	r.pluginCardLoginState(card)
	card.owner.observe(func() {
		ticker := time.NewTicker(time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				r.enqueue(func(*ui.Context) {
					if r.pluginCardLive(card, generation) {
						r.pluginCardLoginState(card)
					}
				})
			}
		}
	})
}

func (card *pluginCardInstance) scope() protocol.PluginContext {
	card.mu.Lock()
	reference := card.reference
	card.mu.Unlock()
	return protocol.PluginContext{
		Surface: protocol.PluginDesktopSurface{Kind: "card", Data: card.summary.Receipt}, Package: reference,
		Worktree: new(card.summary.Worktree), Session: new(card.summary.Receipt.Session),
	}
}

func (card *pluginCardInstance) updateReference(reference protocol.PluginReference) {
	card.mu.Lock()
	defer card.mu.Unlock()
	if reference.Name == card.reference.Name && reference.Digest == card.reference.Digest && reference.SettingsRevision >= card.reference.SettingsRevision {
		card.reference = reference
	}
}

func (card *pluginCardInstance) execute(ctx context.Context, command protocol.Command) (protocol.Output, error) {
	request := card.owner.controller.Prepare(command)
	scope := card.scope()
	request.Plugin = &scope
	return card.owner.controller.Execute(ctx, request)
}

func (card *pluginCardInstance) status(ctx context.Context, key string) (protocol.PluginConnectionStatus, error) {
	scope := card.scope()
	output, err := card.execute(ctx, protocol.Command{Kind: "read_plugin_connection", Data: protocol.CommandReadPluginConnectionPayload{
		Package: scope.Package, Card: key,
	}})
	if err != nil {
		return protocol.PluginConnectionStatus{}, err
	}
	status, valid := detail[protocol.PluginConnectionStatus](output.Data)
	if output.Kind != "plugin_connection" || !valid || status.Card != key ||
		status.Package.Name != card.summary.Package.Name || status.Package.Digest != card.summary.Package.Digest {
		return status, errors.New("unexpected card connection status")
	}
	card.updateReference(status.Package)
	return status, nil
}
