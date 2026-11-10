package live

import (
	"context"
	"encoding/json"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginPaneOpen struct {
	Resource    pluginPaneResource `json:"resource"`
	Title       string             `json:"title"`
	Interaction *uint64            `json:"interaction"`
}

type pluginPaneTitle struct {
	Resource pluginPaneResource `json:"resource"`
	Title    string             `json:"title"`
}

func (r *Runtime) bindPluginResource(mounted *pluginMountedView, native *plugins.NativeBinding) {
	root := r.localRuntime()
	root.initPluginResources()
	var pane *pluginPane
	for _, candidate := range root.pluginResources.panes {
		if candidate.owner == mounted.owner && candidate.reference == mounted.scope.Package &&
			"resource:"+candidate.key.kind+"/"+candidate.key.id == mounted.key.role &&
			mounted.scope.Worktree != nil && *mounted.scope.Worktree == candidate.key.worktree {
			pane = candidate
			pane.mounted = mounted
			break
		}
	}
	native.Operations["sailry/ui.pane"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if pane == nil {
			return json.RawMessage("null"), nil
		}
		var id protocol.TerminalID
		encoded, _ := json.Marshal(pane.key.id)
		if err := json.Unmarshal(encoded, &id); err != nil {
			return nil, err
		}
		return pluginNativeJSON(map[string]any{"resource": pluginPaneResource{Kind: pane.key.kind, ID: id}})
	}
	native.Operations["sailry/ui.openPane"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		request, err := pluginNativeArgument[pluginPaneOpen](arguments, 0)
		if err != nil {
			return nil, err
		}
		key, allowed := root.pluginPaneTarget(mounted, request.Resource, false)
		if !allowed {
			return json.RawMessage("false"), nil
		}
		if !native.Post(func(*ui.Context) {
			if current, allowed := root.pluginPaneTarget(mounted, request.Resource, false); allowed && current == key {
				root.openPluginPane(mounted, key, request.Title)
			}
		}) {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view is closed")
		}
		return json.RawMessage("true"), nil
	}
	native.Operations["sailry/ui.updatePane"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		request, err := pluginNativeArgument[struct {
			Title *string `json:"title"`
			Busy  *bool   `json:"busy"`
		}](arguments, 0)
		if err != nil {
			return nil, err
		}
		if pane == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view has no workspace pane")
		}
		if !native.Post(func(*ui.Context) {
			if root.pluginResources.panes[pane.key] != pane || pane.mounted != mounted {
				return
			}
			if request.Title != nil {
				pane.title = *request.Title
			}
			if request.Busy != nil {
				pane.busy = *request.Busy
			}
			root.wake()
		}) {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view is closed")
		}
		return json.RawMessage("null"), nil
	}
	native.Operations["sailry/ui.closePane"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if pane == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view has no workspace pane")
		}
		if !native.Post(func(*ui.Context) {
			if pane.mounted == mounted {
				root.detachPluginResource(pane)
			}
		}) {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view is closed")
		}
		return json.RawMessage("null"), nil
	}
	native.Operations["sailry/ui.nextPaneEvent"] = func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		if pane == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view has no workspace pane")
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-mounted.ctx.Done():
			return nil, mounted.ctx.Err()
		case event := <-pane.events:
			return event, nil
		}
	}
	native.Operations["sailry/ui.publishResourceTitles"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		request, err := pluginNativeArgument[[]pluginPaneTitle](arguments, 0)
		if err != nil {
			return nil, err
		}
		titles := map[pluginResourceKey]string{}
		for _, title := range request {
			key, allowed := root.pluginPaneTarget(mounted, title.Resource, true)
			if !allowed {
				return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "resource title is unavailable in this view")
			}
			titles[key] = title.Title
		}
		if !native.Post(func(*ui.Context) {
			for key, title := range root.pluginResources.titles {
				if title.source == mounted {
					delete(root.pluginResources.titles, key)
				}
			}
			for key, title := range titles {
				if _, allowed := root.pluginPaneTarget(mounted, pluginPaneResource{Kind: key.kind, ID: pluginResourceTerminalID(key)}, true); !allowed {
					continue
				}
				root.pluginResources.titles[pluginResourceTitleKey{resource: key, name: mounted.scope.Package.Name}] = pluginResourceTitle{source: mounted, value: title}
				if pane := root.pluginResources.panes[key]; pane != nil && pane.reference.Name == mounted.scope.Package.Name {
					pane.title = title
				}
			}
			root.wake()
		}) {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view is closed")
		}
		return json.RawMessage("null"), nil
	}
}

func pluginResourceTerminalID(key pluginResourceKey) protocol.TerminalID {
	var id protocol.TerminalID
	body, _ := json.Marshal(key.id)
	_ = json.Unmarshal(body, &id)
	return id
}

func (r *Runtime) pluginPaneTarget(mounted *pluginMountedView, resource pluginPaneResource, title bool) (pluginResourceKey, bool) {
	if resource.Kind != "terminal" || mounted.scope.Worktree == nil || !r.pluginViewCurrent(mounted) {
		return pluginResourceKey{}, false
	}
	info, available := pluginResourcePackage(mounted.owner, mounted.scope.Package)
	if !available || info.Extension == nil ||
		!slices.Contains(info.Extension.Actions, protocol.PluginActionReadTerminals) ||
		!slices.Contains(info.Extension.Actions, protocol.PluginActionControlTerminals) {
		return pluginResourceKey{}, false
	}
	// Only a package whose renderer for the kind is mounted opens or titles
	// its panes; a core-owned kind has none.
	if _, declared := pluginResourceRenderer(info, resource.Kind); !declared {
		return pluginResourceKey{}, false
	}
	key := pluginResourceKey{node: mounted.owner.controller.Target(), worktree: *mounted.scope.Worktree, kind: resource.Kind, id: resource.ID.String()}
	if !pluginTerminalPresent(mounted.owner, key) {
		return key, false
	}
	return key, true
}

func (r *Runtime) openPluginPane(source *pluginMountedView, key pluginResourceKey, title string) {
	root := r.localRuntime()
	if source.scope.Surface.Kind == "project" &&
		(root.selectedRuntime() != source.owner || source.owner.workspace == nil || source.owner.workspace.selected != key.worktree) {
		return
	}
	pane := root.pluginResources.panes[key]
	if pane != nil && pane.reference != source.scope.Package {
		root.detachPluginResource(pane)
		pane = nil
	}
	if pane == nil {
		scope := protocol.DefaultPluginContext()
		worktree := key.worktree
		scope.Package, scope.Worktree, scope.Surface = source.scope.Package, &worktree, protocol.PluginDesktopSurface{Kind: "workspace"}
		tab := panel.Tab{Kind: panel.Terminal, ID: "plugin/" + key.node.String() + "/" + key.worktree.String() + "/" + key.kind + "/" + key.id}
		pane = &pluginPane{
			key: key, owner: source.owner, reference: source.scope.Package, tab: tab, scope: scope,
			title: title, events: make(chan json.RawMessage, 1),
		}
		root.pluginResources.panes[key] = pane
	}
	root.App.Panel.Show(pane.tab)
	root.App.PanelOpen = true
	root.wake()
}
