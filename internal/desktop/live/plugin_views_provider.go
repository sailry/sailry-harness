package live

import (
	"context"
	"encoding/json"
	"slices"
	"strconv"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The provider slot is the channel rows of Settings › Providers
// (settings/providers/contributions.rs, list.rs provider_usage). One registry
// for the Node the pages hold mounts every enabled package that declares a
// control there, once for all the channels; each package is told the channels
// that are its own (readProviderContexts) and publishes a figure for each,
// scoped to the channel and its credential. A row shows the figures published
// for its channel in place of its login quota. A channel no package publishes
// for shows nothing more.

// providerSlotShown reports whether the provider slot stays mounted for a
// Node: while Settings holds that Node and Demo mode is off. A package's account
// figure is asked of the Node, and the demo asks it nothing.
func providerSlotShown(shown, owner *Runtime, demo bool) bool {
	return owner != nil && shown == owner && !demo
}

// providerUsage is what the provider slot holds for a channel of the Node
// Settings holds (settings.ProviderActions.Usage).
func (r *Runtime) providerUsage(view *ui.Context, channel string) *settings.ChannelUsage {
	root := r.localRuntime()
	owner := root.shownSettings()
	if owner == nil || root.App.Settings.DemoMode || owner.authorizations == nil {
		return nil
	}
	registry := owner.pluginRegistry(protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "provider"}})
	if registry == nil {
		return nil
	}
	for _, provider := range owner.snapshot().Providers {
		if provider.ID.String() != channel {
			continue
		}
		scope, scoped := providerPluginScope(provider, owner.authorizations.credentials)
		if !scoped {
			return nil
		}
		entries := registry.ProviderEntries(scope)
		if len(entries) == 0 {
			return nil
		}
		figures := make([]conversation.Metric, len(entries))
		for index, entry := range entries {
			figures[index] = root.pluginMetric(view, entry)
		}
		return &settings.ChannelUsage{
			Lines: providerUsageLines(figures[0]), Icon: figures[0].Icon,
			Card: func(view *ui.Context, over ui.Element) { conversation.UsageCard(view, over, figures...) },
		}
	}
	return nil
}

// providerUsageLines are the lines of a channel's row for its first figure:
// its first two meters, and without a meter its first row's value or its own
// (settings/providers/list/summary.rs lines). The card on hover has the rest.
func providerUsageLines(figure conversation.Metric) []settings.UsageLine {
	var lines []settings.UsageLine
	for _, row := range figure.Rows {
		if row.Meter && len(lines) < 2 {
			lines = append(lines, settings.UsageLine{Label: strings.TrimSpace(row.Label), Value: row.Value, Meter: true, Progress: row.Progress})
		}
	}
	if len(lines) != 0 {
		return lines
	}
	if len(figure.Rows) != 0 {
		return []settings.UsageLine{{Value: figure.Rows[0].Value}}
	}
	return []settings.UsageLine{{Value: figure.Value}}
}

// providerPluginScope is the identity a package's figure for a channel is
// published and read under: the channel, its credential, and the revisions of
// both, so a figure of an account signed out or replaced is never shown
// (plugins/providers.rs Source::scope). A channel without a current
// credential has none.
func providerPluginScope(provider protocol.ConversationProvider, credentials map[protocol.CredentialID]protocol.Credential) (protocol.PluginUiProviderScope, bool) {
	if provider.Credential == nil {
		return protocol.PluginUiProviderScope{}, false
	}
	credential, exists := credentials[provider.Credential.ID]
	if !exists || credential.Provider != provider.ID || credential.Revoked {
		return protocol.PluginUiProviderScope{}, false
	}
	return protocol.PluginUiProviderScope{
		Provider:   provider.ID,
		Revision:   strconv.FormatUint(provider.Revision, 10) + ":" + strconv.FormatUint(credential.Revision, 10),
		Credential: *provider.Credential,
	}, true
}

// providerPluginContexts are the channels a package's provider view is told
// of, each with its scope, and those scopes: the enabled channels that are
// the package's own and have a current credential, none while the Node is
// away (plugins/providers.rs Source::read). The channel's endpoint and
// account stay out.
func providerPluginContexts(connected bool, providers []protocol.ConversationProvider, credentials map[protocol.CredentialID]protocol.Credential, info protocol.PluginInfo) (map[string]any, []protocol.PluginUiProviderScope) {
	contexts, scopes := []map[string]any{}, []protocol.PluginUiProviderScope{}
	if connected {
		for _, provider := range providers {
			scope, scoped := providerPluginScope(provider, credentials)
			if !scoped {
				continue
			}
			channel, shown := pluginProviderContext(provider, provider.DefaultModel, info)
			if !shown {
				continue
			}
			channel["scope"] = scope
			contexts, scopes = append(contexts, channel), append(scopes, scope)
		}
	}
	return map[string]any{"connected": connected, "providers": contexts}, scopes
}

// pluginNativeProviders is a provider view's channels as it last read them,
// and the scopes its registry accepts figures for.
type pluginNativeProviders struct {
	mu     sync.Mutex
	view   pluginNativeProjection
	scopes []protocol.PluginUiProviderScope
	set    bool
}

// providerOperations gives a view mounted in the provider slot its channels.
// A view of another surface has none, and the calls answer unavailable.
func (state *pluginNativeState) providerOperations() {
	if state.scope.Surface.Kind != "provider" {
		return
	}
	state.providers = &pluginNativeProviders{view: pluginNativeProjection{changed: make(chan struct{})}}
	state.native.Operations["sailry/ui.readProviderContexts"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		state.refreshProviders()
		state.providers.mu.Lock()
		defer state.providers.mu.Unlock()
		return append(json.RawMessage{}, state.providers.view.body...), state.providers.view.err
	}
	state.native.Operations["sailry/ui.nextProviderContexts"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		seen, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		for {
			state.providers.mu.Lock()
			view := &state.providers.view
			body, cursor, changed, failure := append(json.RawMessage{}, view.body...), view.cursor, view.changed, view.err
			state.providers.mu.Unlock()
			if failure != nil {
				return nil, failure
			}
			if body != nil && strconv.FormatUint(cursor, 10) != seen {
				return body, nil
			}
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-state.native.Context.Done():
				return nil, state.native.Context.Err()
			case <-changed:
			}
		}
	}
	state.refreshProviders()
}

// refreshProviders publishes the view's channels as they are now. The
// registry is given their scopes first: the view publishes a figure for a
// channel as soon as it reads of it, and the registry refuses a figure whose
// scope it was not given and forgets those of a scope withdrawn.
func (state *pluginNativeState) refreshProviders() {
	owner := state.providers
	if owner == nil {
		return
	}
	runtime := state.runtime
	info, available := state.packageInfo()
	var credentials map[protocol.CredentialID]protocol.Credential
	if runtime.authorizations != nil {
		credentials = runtime.authorizations.credentials
	}
	value, scopes := providerPluginContexts(available && runtime.controller.Connected(), runtime.snapshot().Providers, credentials, info)
	if !owner.set || !slices.Equal(owner.scopes, scopes) {
		var registry *pluginContributionOwner
		if root := runtime.localRuntime(); root.pluginViews != nil {
			registry = root.pluginViews.registries[pluginScopeIdentity(runtime.controller.Target(), state.scope)]
		}
		if registry != nil && registry.registry.SetProviderScopes(state.scope.Package, scopes) == nil {
			owner.scopes, owner.set = scopes, true
		} else {
			// The registry took no scope: the view is told of no channel
			// until it does, on a later frame.
			value["providers"], owner.set = []map[string]any{}, false
		}
	}
	owner.mu.Lock()
	publishNativeProjection(&owner.view, value, nil)
	owner.mu.Unlock()
}
