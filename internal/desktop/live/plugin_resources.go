package live

import (
	"encoding/json"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginResourceKey struct {
	node     protocol.NodeID
	worktree protocol.WorktreeID
	kind     string
	id       string
}

type pluginPaneResource struct {
	Kind string              `json:"kind"`
	ID   protocol.TerminalID `json:"id"`
}

type pluginPane struct {
	key       pluginResourceKey
	owner     *Runtime
	reference protocol.PluginReference
	tab       panel.Tab
	scope     protocol.PluginContext
	title     string
	busy      bool
	mounted   *pluginMountedView
	events    chan json.RawMessage
}

type pluginResourceTitle struct {
	source *pluginMountedView
	value  string
}

type pluginResourceTitleKey struct {
	resource pluginResourceKey
	name     string
}

type pluginResourceCreate struct {
	owner     *Runtime
	scope     protocol.PluginContext
	reference protocol.PluginReference
	create    string
	registry  *plugins.Registry
}

type pluginResourceState struct {
	panes   map[pluginResourceKey]*pluginPane
	titles  map[pluginResourceTitleKey]pluginResourceTitle
	pending map[pluginResourceKey]*pluginResourceCreate
}

func (r *Runtime) initPluginResources() {
	if r.parent != nil || r.pluginResources != nil {
		return
	}
	r.pluginResources = &pluginResourceState{
		panes: map[pluginResourceKey]*pluginPane{}, titles: map[pluginResourceTitleKey]pluginResourceTitle{},
		pending: map[pluginResourceKey]*pluginResourceCreate{},
	}
	r.App.Panel.SetPluginResourceView(r.renderPluginResource)
	r.App.Panel.SetPluginResourceCreate(r.createPluginResource)
	r.App.Panel.SetPluginResourceClose(r.closePluginResource)
	r.App.Panel.SetPluginResourceTitle(r.pluginResourceTitle)
}

func (r *Runtime) pluginResourcesFrame(view *ui.Context) {
	if r.parent != nil || r.pluginResources == nil {
		return
	}
	state := r.pluginResources
	for key, pane := range state.panes {
		if pane.owner.ctx.Err() != nil || !slices.Contains(r.App.Panel.Tabs, pane.tab) {
			r.detachPluginResource(pane)
			continue
		}
		if pane.owner.controller.Connected() && !pluginTerminalPresent(pane.owner, key) {
			r.detachPluginResource(pane)
		}
	}
	for key, title := range state.titles {
		if title.source.ctx.Err() != nil || !r.pluginViewCurrent(title.source) ||
			!pluginTerminalPresent(title.source.owner, key.resource) {
			delete(state.titles, key)
		}
	}
	for key, request := range state.pending {
		info, available := pluginResourcePackage(request.owner, request.reference)
		if !available || !pluginViewScopeExists(request.owner, request.scope) ||
			request.owner != r.selectedRuntime() || request.owner.workspace == nil || request.owner.workspace.selected != key.worktree {
			delete(state.pending, key)
			continue
		}
		request.registry = request.owner.pluginRegistry(request.scope)
		if request.registry == nil || !request.registry.Ready(request.reference) {
			continue
		}
		delete(state.pending, key)
		contribution, exists := request.registry.Key(request.reference, request.create)
		if !exists || !slices.ContainsFunc(request.registry.Entries("project"), func(entry plugins.ContributionEntry) bool {
			return entry.Key.ID == contribution.ID && entry.Key.Package == contribution.Package &&
				entry.Key.Generation == contribution.Generation && entry.State.Visible && entry.State.Enabled && !entry.State.Loading
		}) {
			r.packageReport(view, pluginNativeFault(protocol.ErrorCodeUnavailable, "resource creation contribution is unavailable"))
			continue
		}
		if _, available := pluginResourceRenderer(info, key.kind); !available {
			continue
		}
		if err := request.registry.Invoke(contribution, "invoke", json.RawMessage("null")); err != nil {
			r.packageReport(view, err)
		}
	}
}

func pluginResourceRenderer(info protocol.PluginInfo, kind string) (protocol.PluginDesktopRenderer, bool) {
	if !pluginNativePackageAvailable(info) || info.Extension == nil || info.Extension.Desktop == nil || info.Extension.Desktop.Entry == nil {
		return protocol.PluginDesktopRenderer{}, false
	}
	for _, renderer := range info.Extension.Desktop.Renderers {
		if string(renderer.Resource) == kind {
			return renderer, true
		}
	}
	return protocol.PluginDesktopRenderer{}, false
}

func pluginResourcePackage(owner *Runtime, reference protocol.PluginReference) (protocol.PluginInfo, bool) {
	if owner == nil || owner.packages == nil || owner.ctx.Err() != nil || !owner.controller.Connected() {
		return protocol.PluginInfo{}, false
	}
	info, exists := owner.packages.infos[reference.Name]
	return info, exists && pluginNativePackageAvailable(info) && packageReference(info.Summary) == reference
}

func pluginTerminalPresent(owner *Runtime, key pluginResourceKey) bool {
	if owner.controller.Target() != key.node || key.kind != "terminal" {
		return false
	}
	return slices.ContainsFunc(owner.controller.Snapshot().Terminals, func(info protocol.TerminalInfo) bool {
		return info.ID.String() == key.id && info.Worktree != nil && *info.Worktree == key.worktree && info.Status.Kind != "closed"
	})
}

func (r *Runtime) createPluginResource(kind panel.Kind) bool {
	if kind != panel.Terminal && kind != panel.Browser {
		return false
	}
	root := r.localRuntime()
	root.initPluginResources()
	owner := root.selectedRuntime()
	if owner == nil || !owner.controller.Connected() || owner.workspace == nil || owner.workspace.selected == (protocol.WorktreeID{}) {
		root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "resource creation requires an available workspace"))
		return true
	}
	for _, info := range root.pluginViewCatalog(owner) {
		renderer, available := pluginResourceRenderer(info, string(kind))
		if !available || renderer.Create == nil {
			continue
		}
		scope := protocol.DefaultPluginContext()
		worktree := owner.workspace.selected
		scope.Worktree, scope.Surface = &worktree, protocol.PluginDesktopSurface{Kind: "project"}
		scope.Package = packageReference(info.Summary)
		key := pluginResourceKey{node: owner.controller.Target(), worktree: worktree, kind: string(kind)}
		if root.pluginResources.pending[key] == nil {
			root.pluginResources.pending[key] = &pluginResourceCreate{
				owner: owner, scope: scope, reference: scope.Package, create: *renderer.Create,
				registry: owner.pluginRegistry(scope),
			}
		}
		root.wake()
		return true
	}
	root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "workspace resource renderer is unavailable"))
	return true
}

func (r *Runtime) renderPluginResource(view *ui.Context, tab panel.Tab) bool {
	if tab.Kind != panel.Terminal {
		return false
	}
	root := r.localRuntime()
	root.initPluginResources()
	var pane *pluginPane
	for _, candidate := range root.pluginResources.panes {
		if candidate.tab == tab {
			pane = candidate
			break
		}
	}
	if pane == nil && !strings.HasPrefix(tab.ID, "plugin/") {
		owner := root.selectedRuntime()
		if root.packages != nil {
			if terminal := root.packages.terminals[tab.ID]; terminal != nil {
				owner = terminal.owner
			}
		}
		if owner != nil {
			for _, terminal := range owner.controller.Snapshot().Terminals {
				if terminal.ID.String() != tab.ID || terminal.Worktree == nil || terminal.Status.Kind == "closed" {
					continue
				}
				for _, info := range root.pluginViewCatalog(owner) {
					if _, exists := pluginResourceRenderer(info, "terminal"); !exists {
						continue
					}
					scope := protocol.DefaultPluginContext()
					worktree := *terminal.Worktree
					scope.Package, scope.Worktree, scope.Surface = packageReference(info.Summary), &worktree, protocol.PluginDesktopSurface{Kind: "workspace"}
					key := pluginResourceKey{node: owner.controller.Target(), worktree: worktree, kind: "terminal", id: terminal.ID.String()}
					pane = &pluginPane{
						key: key, owner: owner, reference: scope.Package, tab: tab, scope: scope,
						events: make(chan json.RawMessage, 1),
					}
					root.pluginResources.panes[key] = pane
					break
				}
				break
			}
		}
	}
	if pane == nil {
		kit.Result(view, kit.ResultProps{Icon: panel.ResourceOf(tab.Kind).Icon, Title: i18n.T(view, "terminal_unavailable")}).Grow(1).Justify(ui.Center)
		return true
	}
	info, available := pluginResourcePackage(pane.owner, pane.reference)
	if !available {
		if pane.mounted != nil && pane.mounted.mounted != nil && pane.mounted.mounted.Suspended() {
			root.renderMountedPlugin(view, pane.mounted)
		} else {
			kit.Result(view, kit.ResultProps{Icon: panel.ResourceOf(tab.Kind).Icon, Title: i18n.T(view, "terminal_unavailable")}).Grow(1).Justify(ui.Center)
		}
		return true
	}
	pane.mounted = root.ensurePluginView(view, pane.owner, info, pane.scope, "resource:"+pane.key.kind+"/"+pane.key.id, nil)
	root.renderMountedPlugin(view, pane.mounted)
	return true
}

func (r *Runtime) closePluginResource(tab panel.Tab) bool {
	root := r.localRuntime()
	if root.pluginResources == nil {
		return false
	}
	for _, pane := range root.pluginResources.panes {
		if pane.tab != tab {
			continue
		}
		if pane.busy {
			return true
		}
		if pane.mounted == nil || !root.pluginViewCurrent(pane.mounted) || pane.mounted.mounted == nil {
			root.detachPluginResource(pane)
			return true
		}
		select {
		case pane.events <- json.RawMessage(`{"kind":"close"}`):
		default:
		}
		root.wake()
		return true
	}
	return false
}

func (r *Runtime) detachPluginResource(pane *pluginPane) {
	root := r.localRuntime()
	if root.pluginResources == nil || root.pluginResources.panes[pane.key] != pane {
		return
	}
	delete(root.pluginResources.panes, pane.key)
	root.App.Panel.Tabs, root.App.Panel.Active = panel.Closed(root.App.Panel.Tabs, pane.tab, root.App.Panel.Active)
	if pane.mounted != nil && root.pluginViews != nil && root.pluginViews.mounts[pane.mounted.key] == pane.mounted {
		root.dropPluginView(pane.mounted, true)
		delete(root.pluginViews.mounts, pane.mounted.key)
	}
	root.wake()
}

func (r *Runtime) pluginResourceTitle(tab panel.Tab) (string, bool) {
	root := r.localRuntime()
	if root.pluginResources == nil {
		return "", false
	}
	for _, pane := range root.pluginResources.panes {
		if pane.tab == tab && pane.title != "" {
			return pane.title, true
		}
	}
	return "", false
}
