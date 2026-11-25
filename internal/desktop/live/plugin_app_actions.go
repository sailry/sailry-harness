package live

import (
	"context"
	"encoding/json"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) syncMountedPluginAppNavigation(mounted *pluginMountedView, visible bool) {
	if !visible || mounted.host == nil || mounted.mounted == nil || mounted.disconnected || r.App.DemoOn() ||
		!r.resourceVisible(mounted.key.scope.node.String()) || !mounted.appNavigationEnabled() {
		mounted.releaseAppNavigation()
		return
	}
	if mounted.appNavigation != nil {
		return
	}
	scope := shell.AppActionScope{Node: mounted.owner.controller.Target()}
	if mounted.scope.Worktree != nil {
		scope.Worktree = *mounted.scope.Worktree
	}
	if mounted.scope.Session != nil {
		scope.Session = *mounted.scope.Session
	}
	mounted.appNavigation = r.App.BindAppNavigation(scope, func() bool {
		return r.mountedPluginAppNavigationCurrent(mounted, scope)
	}, func(permission protocol.PluginAction) bool {
		return mounted.info.Extension != nil && slices.Contains(mounted.info.Extension.Actions, permission)
	})
}

func (mounted *pluginMountedView) releaseAppNavigation() {
	if mounted.appNavigation != nil {
		mounted.appNavigation.Close()
		mounted.appNavigation = nil
	}
}

func (mounted *pluginMountedView) invokeAppAction(view *ui.Context, id string, input json.RawMessage) error {
	if mounted.appNavigation == nil {
		return pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin navigation source is not visible")
	}
	return mounted.appNavigation.Invoke(view, shell.AppActionID(id), input)
}

func (mounted *pluginMountedView) appNavigationEnabled() bool {
	return mounted.key.role != "settings" || slices.ContainsFunc(mounted.owner.snapshot().Plugins, func(summary protocol.PluginSummary) bool {
		return summary.Enabled && packageReference(summary) == mounted.scope.Package
	})
}

func (r *Runtime) mountedPluginAppNavigationCurrent(mounted *pluginMountedView, scope shell.AppActionScope) bool {
	node := scope.Node.String()
	if mounted.ctx.Err() != nil || mounted.disconnected || mounted.mounted == nil || !mounted.owner.controller.Connected() ||
		r.runtimeFor(node) != mounted.owner || !r.resourceVisible(node) || !r.pluginViewCurrent(mounted) {
		return false
	}
	if mounted.key.role == "contribution" {
		owner := r.pluginViews.registries[mounted.key.scope]
		return owner != nil && owner.seen
	}
	return mounted.seen && mounted.appNavigationEnabled()
}

func (state *pluginNativeState) invokeAppAction(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	if len(arguments) == 0 || len(arguments) > 2 {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "app action requires an ID and optional input")
	}
	id, err := pluginNativeArgument[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	input := json.RawMessage(`{}`)
	if len(arguments) == 2 {
		input = arguments[1]
	}
	return state.update(ctx, func(view *ui.Context) (json.RawMessage, error) {
		if state.native.InvokeAppAction == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "app navigation is unavailable on this surface")
		}
		return json.RawMessage("null"), state.native.InvokeAppAction(view, id, input)
	})
}
