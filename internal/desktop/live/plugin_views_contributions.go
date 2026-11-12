package live

import (
	"cmp"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginContributionOwner struct {
	key      pluginScopeKey
	owner    *Runtime
	scope    protocol.PluginContext
	registry *desktopplugins.Registry
	draft    string
	seen     bool
	queued   bool
}

func (r *Runtime) pluginComposerView(id string) *viewmodel.Conversation {
	root := r.localRuntime()
	if conversation := root.App.Data.Find(id); conversation != nil {
		return conversation
	}
	if root.pluginViews != nil {
		return root.pluginViews.drafts[id]
	}
	return nil
}

func pluginViewAccepts(surface string, slot protocol.PluginUiSlot) bool {
	switch surface {
	case "provider":
		return slot == "provider"
	case "project":
		return slot == "project" || slot == "project_menu"
	default:
		return slot != "provider" && slot != "project" && slot != "project_menu"
	}
}

// nativeProjectControl reports whether a bundled package's project control
// is the application's own rather than a mounted one: the overview's New
// terminal and AI CLI buttons, and the project menu's New terminal and
// Worktrees entries, which projectPluginMenu draws in the slot while their
// package is enabled. The commands package's CLI submenu of the project menu
// is not shown at all (AGENTS.md, Removed UI).
func nativeProjectControl(name, id string) bool {
	switch name {
	case "commands":
		return id == "terminal-new" || id == "terminal-cli" || id == "terminal-menu-new" || id == "terminal-menu-cli"
	case pluginWorktreesPackage:
		return id == "manage"
	}
	return false
}

// pluginViewDeclarations are the controls a package shows on a surface.
func pluginViewDeclarations(name string, declarations []protocol.PluginUiContribution, surface string) []protocol.PluginUiContribution {
	result := make([]protocol.PluginUiContribution, 0, len(declarations))
	for _, declaration := range declarations {
		if pluginViewAccepts(surface, declaration.Slot) && !(surface == "project" && nativeProjectControl(name, declaration.ID)) {
			result = append(result, declaration)
		}
	}
	return result
}

func (r *Runtime) pluginRegistry(scope protocol.PluginContext, presentation ...string) *desktopplugins.Registry {
	root := r.localRuntime()
	root.initPluginViews()
	if root.pluginViews == nil || root.pluginViews.closed {
		return nil
	}
	if scope.Surface.Kind == "" || scope.Surface.Kind == "workspace" {
		scope.Surface = protocol.PluginDesktopSurface{Kind: "composer"}
	}
	if scope.Surface.Kind == "project" {
		scope.Session = nil
	}
	scope.Package, scope.Invocation, scope.Turn = protocol.PluginReference{}, nil, nil
	key := pluginScopeIdentity(r.controller.Target(), scope)
	draft := ""
	if scope.Surface.Kind == "composer" && scope.Session == nil {
		draft = root.App.Sidebar.Selected
		if len(presentation) != 0 {
			draft = presentation[0]
		}
		key.presentation = draft
	}
	owner := root.pluginViews.registries[key]
	// Every frame synchronizes the registries it starts with. Only one made
	// or rebound while a frame is built asks for the next frame; asking on
	// each lookup would redraw an idle window forever.
	changed := false
	if owner == nil {
		owner = &pluginContributionOwner{key: key, owner: r, scope: scope, draft: draft}
		owner.registry = desktopplugins.NewRegistry(desktopplugins.RegistryOptions{
			Node: r.controller.Target(), Context: scope, Invalidate: root.wake,
		})
		root.pluginViews.registries[key] = owner
		changed = true
	} else if owner.owner != r || owner.draft != draft {
		for mountKey, mounted := range root.pluginViews.mounts {
			if mountKey.role == "contribution" && mountKey.scope == key {
				root.dropPluginView(mounted, true)
				delete(root.pluginViews.mounts, mountKey)
			}
		}
		owner.owner, owner.scope, owner.draft = r, scope, draft
		changed = true
	}
	owner.seen = true
	if changed && !owner.queued {
		owner.queued = true
		root.enqueue(func(current *ui.Context) {
			owner.queued = false
			if root.pluginViews == nil || root.pluginViews.closed || root.pluginViews.registries[key] != owner {
				return
			}
			root.syncPluginRegistry(current, owner)
		})
	}
	return owner.registry
}

func rootPluginRegistryCurrent(root *Runtime, owner *pluginContributionOwner) bool {
	if !pluginViewScopeExists(owner.owner, owner.scope) {
		return false
	}
	if owner.scope.Surface.Kind == "provider" {
		return providerSlotShown(root.shownSettings(), owner.owner, root.App.Settings.DemoMode)
	}
	if owner.draft == "" {
		return true
	}
	// A demo conversation's contributions last as long as the demo has it.
	if id, demo := strings.CutPrefix(owner.draft, pluginDemoPresentation); demo {
		return root.App.DemoConversation(id) != nil
	}
	conversation := root.pluginComposerView(owner.draft)
	if conversation == nil {
		return false
	}
	composer := root.App.Composer(conversation.ID)
	current := root.conversationRuntime(conversation, composer)
	return current == owner.owner &&
		pluginScopeIdentity(current.controller.Target(), composerPluginScope(current, conversation, composer)) ==
			pluginScopeIdentity(owner.owner.controller.Target(), owner.scope)
}

func (r *Runtime) syncPluginRegistry(view *ui.Context, owner *pluginContributionOwner) {
	root := r.localRuntime()
	if root.pluginViews == nil || root.pluginViews.registries[owner.key] != owner {
		return
	}
	if !rootPluginRegistryCurrent(root, owner) {
		root.closePluginRegistry(owner)
		return
	}
	if !owner.owner.controller.Connected() {
		for _, mounted := range root.pluginViews.mounts {
			if mounted.key.role == "contribution" && mounted.key.scope == owner.key {
				root.disconnectPluginView(mounted)
			}
		}
		return
	}
	var assistant *protocol.PluginConversationBinding
	if owner.scope.Session != nil && owner.scope.Surface.Kind != "project" {
		if session, exists := owner.owner.session(owner.scope.Session.String()); exists {
			assistant = session.Config.Assistant
		}
	} else if owner.draft != "" {
		if conversation := root.pluginComposerView(owner.draft); conversation != nil {
			composer := root.App.Composer(conversation.ID)
			if config, exists := owner.owner.composerBases[composer]; exists {
				assistant = config.Assistant
			}
		}
	}
	demo := strings.HasPrefix(owner.draft, pluginDemoPresentation)
	desired := map[string]protocol.PluginInfo{}
	for _, info := range root.pluginViewCatalog(owner.owner) {
		extension := info.Extension
		// Over a demo conversation the statistics package alone is shown: it
		// reads the composer state and nothing of the Node.
		if demo && info.Summary.Name != pluginStatisticsPackage {
			continue
		}
		if extension == nil || extension.Desktop == nil || extension.Desktop.UiEntry == nil ||
			unmountedPlugin(info.Summary.Name) || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" ||
			len(pluginViewDeclarations(info.Summary.Name, extension.Ui, owner.scope.Surface.Kind)) == 0 {
			continue
		}
		if assistant == nil {
			if extension.Scope != protocol.PluginScopeHost {
				continue
			}
		} else if assistant.Package.Name != info.Summary.Name &&
			!(extension.Scope == protocol.PluginScopeHost && extension.Desktop.UiShared) {
			continue
		}
		desired[info.Summary.Name] = info
	}
	for key, mounted := range root.pluginViews.mounts {
		if key.role != "contribution" || key.scope != owner.key {
			continue
		}
		info, available := desired[key.name]
		if !available || packageReference(info.Summary) != mounted.scope.Package || mounted.locale != root.packageLocale() {
			root.dropPluginView(mounted, true)
			delete(root.pluginViews.mounts, key)
		}
	}
	for _, info := range desired {
		root.ensurePluginView(view, owner.owner, info, owner.scope, "contribution", owner.registry)
	}
}

func (r *Runtime) closePluginRegistry(owner *pluginContributionOwner) {
	root := r.localRuntime()
	for key, mounted := range root.pluginViews.mounts {
		if key.role == "contribution" && key.scope == owner.key {
			root.dropPluginView(mounted, true)
			delete(root.pluginViews.mounts, key)
		}
	}
	_ = owner.registry.Close()
	delete(root.pluginViews.registries, owner.key)
}

func (r *Runtime) pluginContribution(key desktopplugins.ContributionKey) (*pluginContributionOwner, *pluginMountedView) {
	root := r.localRuntime()
	if root.pluginViews == nil || root.pluginViews.closed {
		return nil, nil
	}
	for scope, owner := range root.pluginViews.registries {
		if scope.node != key.Node || scope.worktree != pluginWorktreeKey(key.Worktree) || scope.session != pluginSessionKey(key.Session) {
			continue
		}
		current, exists := owner.registry.Key(key.Package, key.ID)
		if !exists || current.Generation != key.Generation {
			continue
		}
		mounted := root.pluginViews.mounts[pluginMountKey{scope: scope, role: "contribution", name: key.Package.Name}]
		if mounted != nil && mounted.scope.Package == key.Package && root.pluginViewCurrent(mounted) {
			return owner, mounted
		}
	}
	return nil, nil
}

func pluginWorktreeKey(worktree *protocol.WorktreeID) string {
	if worktree == nil {
		return ""
	}
	return worktree.String()
}

func pluginSessionKey(session *protocol.SessionID) string {
	if session == nil {
		return ""
	}
	return session.String()
}

func (r *Runtime) renderPluginContribution(view *ui.Context, key desktopplugins.ContributionKey) bool {
	owner, mounted := r.pluginContribution(key)
	if owner == nil {
		return false
	}
	owner.seen = true
	return r.localRuntime().renderMountedPlugin(view, mounted)
}

func (r *Runtime) retryPluginContribution(key desktopplugins.ContributionKey) bool {
	owner, mounted := r.pluginContribution(key)
	if owner == nil || mounted.failure == nil || !owner.owner.controller.Connected() {
		return false
	}
	root := r.localRuntime()
	root.enqueue(func(view *ui.Context) {
		current, captured := root.pluginContribution(key)
		if current != owner || captured != mounted {
			return
		}
		root.dropPluginView(mounted, true)
		delete(root.pluginViews.mounts, mounted.key)
		root.syncPluginRegistry(view, owner)
	})
	return true
}

func (r *Runtime) renderPluginOverlays(view *ui.Context) {
	if view == nil || r.pluginViews == nil || r.pluginViews.closed {
		return
	}
	var mountedViews []*pluginMountedView
	for _, mounted := range r.pluginViews.mounts {
		owner := r.pluginViews.registries[mounted.key.scope]
		if mounted.key.role != "contribution" || mounted.seen || owner == nil || !owner.seen ||
			mounted.info.Extension == nil || mounted.info.Extension.Desktop == nil || !mounted.info.Extension.Desktop.UiOverlay ||
			mounted.mounted == nil || !r.pluginViewCurrent(mounted) {
			continue
		}
		mountedViews = append(mountedViews, mounted)
	}
	slices.SortFunc(mountedViews, func(first, second *pluginMountedView) int {
		if order := cmp.Compare(first.key.scope.node.String(), second.key.scope.node.String()); order != 0 {
			return order
		}
		if order := cmp.Compare(first.key.scope.worktree, second.key.scope.worktree); order != 0 {
			return order
		}
		if order := cmp.Compare(first.key.scope.session, second.key.scope.session); order != 0 {
			return order
		}
		if order := cmp.Compare(first.key.scope.surface, second.key.scope.surface); order != 0 {
			return order
		}
		if order := cmp.Compare(first.key.scope.presentation, second.key.scope.presentation); order != 0 {
			return order
		}
		return cmp.Compare(first.key.name, second.key.name)
	})
	for _, mounted := range mountedViews {
		current := view.Key("plugin-overlay").Key(mounted.generation)
		ui.Overlay(current, func() {
			ui.Box(current).Absolute().Left(0).Right(0).Top(0).Bottom(0).PassThrough().Children(func() {
				r.renderMountedPlugin(current, mounted)
			})
		})
	}
}
