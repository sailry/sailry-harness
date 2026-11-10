package live

import (
	"cmp"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNavigationEntry struct {
	id      string
	owner   *Runtime
	info    protocol.PluginInfo
	scope   protocol.PluginContext
	options protocol.PluginDesktopNavigationOptions
}

type pluginSettingsEntry struct {
	section settings.Section
	owner   *Runtime
	info    protocol.PluginInfo
	scope   protocol.PluginContext
}

func unmountedPlugin(name string) bool {
	return name == "files" || name == "git"
}

func (r *Runtime) projectPluginViews(view *ui.Context) {
	root := r.localRuntime()
	state := root.pluginViews
	selected := root.packageOwner()
	locale := root.packageLocale()
	entries := []pluginNavigationEntry{}
	for _, source := range []struct {
		owner *Runtime
		scope protocol.PluginScope
	}{{selected, protocol.PluginScopeHost}, {root, protocol.PluginScopeDesktop}} {
		for _, info := range root.pluginViewCatalog(source.owner) {
			extension := info.Extension
			if extension == nil || extension.Scope != source.scope || extension.Desktop == nil || unmountedPlugin(info.Summary.Name) ||
				!info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
				continue
			}
			desktop := extension.Desktop
			if desktop.Navigation == nil {
				continue
			}
			scope := protocol.DefaultPluginContext()
			scope.Package = packageReference(info.Summary)
			scope.Surface = desktop.NavigationOptions.Surface
			if desktop.NavigationOptions.Target == protocol.PluginDesktopNavigationTargetWorktree &&
				source.owner.workspace != nil && source.owner.workspace.selected != (protocol.WorktreeID{}) {
				worktree := source.owner.workspace.selected
				scope.Worktree = &worktree
			}
			entries = append(entries, pluginNavigationEntry{
				id:    "plugin:" + source.owner.controller.Target().String() + "/" + info.Summary.Name,
				owner: source.owner, info: info, scope: scope, options: desktop.NavigationOptions,
			})
		}
	}
	slices.SortFunc(entries, func(first, second pluginNavigationEntry) int {
		if order := cmp.Compare(first.options.Order, second.options.Order); order != 0 {
			return order
		}
		return cmp.Compare(first.id, second.id)
	})
	navigation := make(map[string]pluginNavigationEntry, len(entries))
	features := make([]shell.Feature, 0, len(entries))
	for _, entry := range entries {
		navigation[entry.id] = entry
		declared := entry.info.Extension.Desktop.Navigation
		features = append(features, shell.Feature{
			ID: entry.id, Label: pluginNavigation(*declared, locale),
			Icon: root.pluginNavigationIcon(view, declared.Icon), DefaultPinned: entry.options.Pinned,
		})
	}
	state.navigation = navigation
	root.App.SetPluginFeatures(features)
	var settingsEntries []pluginSettingsEntry
	for _, info := range root.pluginViewCatalog(selected) {
		if info.Extension == nil || info.Extension.SettingsPage == nil || info.Summary.Compatibility.Kind != "compatible" {
			continue
		}
		scope := protocol.DefaultPluginContext()
		scope.Package, scope.Surface = packageReference(info.Summary), protocol.PluginDesktopSurface{Kind: "settings"}
		settingsEntries = append(settingsEntries, pluginSettingsEntry{
			section: settings.Section("plugin:" + selected.controller.Target().String() + "/" + info.Summary.Name),
			owner:   selected, info: info, scope: scope,
		})
	}
	slices.SortFunc(settingsEntries, func(first, second pluginSettingsEntry) int {
		if order := cmp.Compare(first.info.Extension.SettingsPage.Placement.Order, second.info.Extension.SettingsPage.Placement.Order); order != 0 {
			return order
		}
		return cmp.Compare(first.section, second.section)
	})
	pages := make([]settings.PluginPage, 0, len(settingsEntries))
	state.settings = make(map[settings.Section]pluginSettingsEntry, len(settingsEntries))
	for _, entry := range settingsEntries {
		state.settings[entry.section] = entry
		page := entry.info.Extension.SettingsPage
		pages = append(pages, settings.PluginPage{
			Section: entry.section, Label: pluginNavigation(page.Navigation, locale),
			Icon:  root.pluginNavigationIcon(view, page.Navigation.Icon),
			Group: "settings_group_" + string(page.Placement.Group),
			Order: page.Placement.Order,
			Render: func(current *ui.Context) {
				live, exists := root.pluginViews.settings[entry.section]
				if !exists || live.owner != entry.owner || live.scope.Package != entry.scope.Package {
					return
				}
				root.renderPluginSettings(current, entry)
			},
		})
	}
	root.App.Settings.SetPluginPages(pages)
}

func (r *Runtime) pluginSettingsRetained(mounted *pluginMountedView) bool {
	for _, entry := range r.localRuntime().pluginViews.settings {
		if entry.owner == mounted.owner && entry.info.Summary.Name == mounted.scope.Package.Name &&
			pluginScopeIdentity(entry.owner.controller.Target(), entry.scope) == mounted.key.scope {
			return true
		}
	}
	return false
}

func (r *Runtime) pluginNavigationRetained(mounted *pluginMountedView) bool {
	for _, entry := range r.localRuntime().pluginViews.navigation {
		if entry.owner == mounted.owner && entry.info.Summary.Name == mounted.scope.Package.Name &&
			pluginScopeIdentity(entry.owner.controller.Target(), entry.scope) == mounted.key.scope {
			return true
		}
	}
	return false
}

func (r *Runtime) selectPluginView(id string) bool {
	root := r.localRuntime()
	if root.pluginViews == nil {
		return false
	}
	entry, exists := root.pluginViews.navigation[id]
	if !exists || !entry.owner.controller.Connected() ||
		entry.options.Target == protocol.PluginDesktopNavigationTargetWorktree && entry.scope.Worktree == nil {
		return false
	}
	if entry.scope.Surface.Kind == "settings" {
		section := settings.Section("plugin:" + entry.owner.controller.Target().String() + "/" + entry.info.Summary.Name)
		if _, exists := root.pluginViews.settings[section]; exists {
			root.App.Settings.Open(section)
			root.App.Navigate(shell.SettingsPage)
		} else {
			entry.owner.configurePackage(entry.info.Summary.Name)
		}
		return false
	}
	key := pluginMountKey{scope: pluginScopeIdentity(entry.owner.controller.Target(), entry.scope), role: "navigation", name: entry.scope.Package.Name}
	if previous := root.pluginViews.mounts[key]; previous != nil && previous.failure != nil {
		root.dropPluginView(previous, true)
		delete(root.pluginViews.mounts, key)
	}
	if mounted := root.ensurePluginView(nil, entry.owner, entry.info, entry.scope, "navigation", nil); mounted != nil && mounted.host != nil {
		mounted.host.Enter()
	}
	return true
}

func (r *Runtime) renderPluginPage(view *ui.Context, id string) {
	root := r.localRuntime()
	if root.pluginViews == nil {
		return
	}
	entry, exists := root.pluginViews.navigation[id]
	if !exists || entry.options.Target == protocol.PluginDesktopNavigationTargetWorktree && entry.scope.Worktree == nil {
		ui.Box(view).Fill()
		return
	}
	mounted := root.ensurePluginView(view, entry.owner, entry.info, entry.scope, "navigation", nil)
	if !root.renderMountedPlugin(view, mounted) {
		ui.Box(view).Fill()
	}
}
