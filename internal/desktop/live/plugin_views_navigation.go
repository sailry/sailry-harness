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
	status  string // display-only when the selected Node cannot provide the page
}

func hasPluginSettingsContent(info protocol.PluginInfo) bool {
	if info.Extension == nil || info.Extension.SettingsPage == nil {
		return false
	}
	page := info.Extension.SettingsPage
	return page.Entry != nil || page.EnabledControl || info.Settings != nil
}

func localPluginSettings(info protocol.PluginInfo) bool {
	extension := info.Extension
	if extension == nil {
		return false
	}
	if desktopPlugin(info) {
		return true
	}
	// A browser-only custom page configures the controller's browser
	// profile. The package itself can still be installed on a Host.
	page := extension.SettingsPage
	return page != nil && page.Entry != nil && !page.EnabledControl && info.Settings == nil &&
		len(extension.Actions) > 0 && !slices.ContainsFunc(extension.Actions, func(action protocol.PluginAction) bool {
		return action != protocol.PluginActionReadBrowser && action != protocol.PluginActionControlBrowser
	})
}

func desktopPlugin(info protocol.PluginInfo) bool {
	return info.Extension != nil && info.Extension.Scope == protocol.PluginScopeDesktop
}

func pluginSettingsSection(owner *Runtime, info protocol.PluginInfo) settings.Section {
	if localPluginSettings(info) {
		return settings.Section("plugin:" + owner.controller.Target().String() + "/" + info.Summary.Name)
	}
	return settings.Section("plugin:" + info.Summary.Name)
}

func unmountedPlugin(name string) bool {
	return name == "files" || name == "git"
}

func (r *Runtime) projectPluginViews(view *ui.Context) {
	root := r.localRuntime()
	state := root.pluginViews
	workspace := root.selectedRuntime()
	locale := root.packageLocale()
	entries := []pluginNavigationEntry{}
	type navigationSource struct {
		owner *Runtime
		scope protocol.PluginScope
	}
	sources := []navigationSource{{root, protocol.PluginScopeHost}, {root, protocol.PluginScopeDesktop}}
	if workspace != root {
		sources = append(sources, navigationSource{workspace, protocol.PluginScopeHost})
	}
	for _, source := range sources {
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
			if source.scope == protocol.PluginScopeHost {
				owner := root
				if desktop.NavigationOptions.Target == protocol.PluginDesktopNavigationTargetWorktree {
					owner = workspace
				}
				if source.owner != owner {
					continue
				}
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
	root.projectPluginSettings(view)
}

func (r *Runtime) projectPluginSettings(view *ui.Context) {
	state := r.pluginViews
	selected := r.packageOwner()
	var settingsEntries []pluginSettingsEntry
	for _, owner := range []*Runtime{selected, r} {
		for _, info := range r.pluginViewCatalog(owner) {
			local := localPluginSettings(info)
			if !hasPluginSettingsContent(info) || info.Summary.Compatibility.Kind != "compatible" ||
				local && owner != r || !local && owner != selected {
				continue
			}
			section := pluginSettingsSection(owner, info)
			scope := protocol.DefaultPluginContext()
			scope.Package, scope.Surface = packageReference(info.Summary), protocol.PluginDesktopSurface{Kind: "settings"}
			settingsEntries = append(settingsEntries, pluginSettingsEntry{section: section, owner: owner, info: info, scope: scope})
		}
		if selected == r {
			break
		}
	}
	active := r.App.Settings.Section
	if previous, exists := state.settings[active]; exists && !localPluginSettings(previous.info) &&
		!slices.ContainsFunc(settingsEntries, func(entry pluginSettingsEntry) bool { return entry.section == active }) {
		// Keep the open page's identity and label. Its previous declaration
		// is only presentation, never a fallback package or Client.
		settingsEntries = append(settingsEntries, pluginSettingsEntry{
			section: active, info: previous.info, status: pluginSettingsStatus(selected, previous.info.Summary.Name),
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
	locale := r.packageLocale()
	for _, entry := range settingsEntries {
		state.settings[entry.section] = entry
		page := entry.info.Extension.SettingsPage
		pages = append(pages, settings.PluginPage{
			Section: entry.section, Label: pluginNavigation(page.Navigation, locale),
			Icon:        r.pluginNavigationIcon(view, page.Navigation.Icon),
			Group:       "settings_group_" + string(page.Placement.Group),
			Order:       page.Placement.Order,
			ContentOnly: entry.status != "",
			Render: func(current *ui.Context) {
				live, exists := state.settings[entry.section]
				if !exists || live.owner != entry.owner || live.scope.Package != entry.scope.Package || live.status != entry.status {
					return
				}
				r.renderPluginSettings(current, live)
			},
		})
	}
	r.App.Settings.SetPluginPages(pages)
}

func pluginSettingsStatus(owner *Runtime, name string) string {
	if owner == nil || !owner.controller.Connected() {
		return "plugins_view_offline"
	}
	for _, summary := range owner.snapshot().Plugins {
		if summary.Name != name {
			continue
		}
		if removed, exists := owner.packages.removed[name]; exists && summary.Revision <= removed {
			break
		}
		if summary.Compatibility.Kind != "compatible" {
			return "market_details_unsupported"
		}
		if owner.packages.failures[name] == packageVersion(summary) {
			return "plugins_read_failed"
		}
		if info := owner.packages.infos[name]; info.Summary.Digest == summary.Digest {
			return "plugins_settings_none"
		}
		return "plugins_loading"
	}
	return "plugins_not_installed_on_host"
}

func (r *Runtime) pluginSettingsTargetCurrent(entry pluginSettingsEntry) bool {
	if entry.owner == nil {
		return false
	}
	root := r.localRuntime()
	if localPluginSettings(entry.info) {
		return entry.owner == root
	}
	return root.managedSettingsNode() == entry.owner
}

func (r *Runtime) pluginSettingsRetained(mounted *pluginMountedView) bool {
	for _, entry := range r.localRuntime().pluginViews.settings {
		if entry.owner == mounted.owner && r.pluginSettingsTargetCurrent(entry) && entry.info.Summary.Name == mounted.scope.Package.Name &&
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
		if !root.openPluginSettings(entry.owner, entry.info) {
			entry.owner.configurePackage(entry.info.Summary.Name)
		}
		return false
	}
	// The next page has not projected its slots yet. Do not let the Shell
	// invoke the previous page's navigation before that projection runs.
	root.showPluginDetails(nil)
	key := pluginMountKey{scope: pluginScopeIdentity(entry.owner.controller.Target(), entry.scope), role: "navigation", name: entry.scope.Package.Name}
	if previous := root.pluginViews.mounts[key]; previous != nil && previous.failure != nil {
		root.dropPluginView(previous, true)
		delete(root.pluginViews.mounts, key)
		delete(root.pluginViews.failures, key)
	}
	if mounted := root.ensurePluginView(nil, entry.owner, entry.info, entry.scope, "navigation", nil); mounted != nil && mounted.host != nil {
		mounted.host.Enter()
	}
	return true
}

func (r *Runtime) openPluginSettings(owner *Runtime, info protocol.PluginInfo) bool {
	root := r.localRuntime()
	if !hasPluginSettingsContent(info) || !owner.controller.Connected() || info.Summary.Compatibility.Kind != "compatible" ||
		localPluginSettings(info) && owner != root {
		return false
	}
	if !localPluginSettings(info) {
		root.selectPackageSettingsNode(owner)
	}
	section := pluginSettingsSection(owner, info)
	root.projectPluginSettings(nil)
	if root.pluginViews.settings[section].owner != owner {
		return false
	}
	root.App.Settings.Open(section)
	root.App.Navigate(shell.SettingsPage)
	return true
}

func (r *Runtime) selectPackageSettingsNode(owner *Runtime) {
	root := r.localRuntime()
	target := ""
	if owner != root {
		target = owner.controller.Target().String()
	}
	root.App.Settings.ManagedNode = target
}

func (r *Runtime) renderPluginPage(view *ui.Context, id string) {
	root := r.localRuntime()
	if root.pluginViews == nil {
		return
	}
	entry, exists := root.pluginViews.navigation[id]
	if !exists || entry.options.Target == protocol.PluginDesktopNavigationTargetWorktree && entry.scope.Worktree == nil {
		root.showPluginDetails(nil)
		ui.Box(view).Fill()
		return
	}
	mounted := root.ensurePluginView(view, entry.owner, entry.info, entry.scope, "navigation", nil)
	if mounted != nil {
		mounted.details, mounted.navigation = nil, nil
	}
	if !root.renderMountedPlugin(view, mounted) {
		root.showPluginDetails(nil)
		ui.Box(view).Fill()
		return
	}
	root.showPluginDetails(mounted)
}

// showPluginDetails projects the Workspace's slots after drawing its content.
// The left column precedes the page and uses the preceding snapshot; the right
// column follows it and uses this one. Neither child is drawn inside the page.
// Each closure checks its mount and selected feature before using its snapshot.
func (r *Runtime) showPluginDetails(mounted *pluginMountedView) {
	var details, navigation, heading func(*ui.Context)
	label := ""
	feature := r.App.PluginFeature()
	current := func() bool {
		return mounted != nil && mounted.failure == nil && r.App.Page == shell.PluginPage && r.App.PluginFeature() == feature && r.pluginViewCurrent(mounted)
	}
	if mounted != nil && mounted.workspaceLayout != nil {
		layout := mounted.workspaceLayout()
		r.App.SetPluginWorkspace(feature+"/"+layout.ID, layout.MinNavigationWidth, layout.MinDetailsWidth, layout.DefaultDetailsWidth)
		label = layout.DetailsLabel
		if layout != mounted.projectedWorkspace {
			mounted.projectedWorkspace = layout
			r.wake()
		}
	}
	if mounted != nil && mounted.navigation != nil {
		render := mounted.navigation
		navigation = func(view *ui.Context) {
			if !current() {
				return
			}
			target := view.Key(mounted.generation)
			ui.Column(target).Fill().MinWidth(0).MinHeight(0).Disabled(mounted.disconnected).DimDisabled(false).Children(func() {
				if err := render(target); err != nil {
					r.failPluginView(view, mounted, err, "plugin_callback_failed")
				}
			})
		}
	}
	if mounted != nil && mounted.details != nil {
		heading = func(view *ui.Context) {
			if !current() || mounted.detailsHeading == nil {
				return
			}
			if draw := mounted.detailsHeading(); draw != nil {
				if err := draw(view.Key(mounted.generation)); err != nil {
					r.failPluginView(view, mounted, err, "plugin_callback_failed")
				}
			}
		}
		render := mounted.details
		details = func(view *ui.Context) {
			if !current() {
				return
			}
			target := view.Key(mounted.generation)
			ui.Column(target).Fill().MinWidth(0).MinHeight(0).Disabled(mounted.disconnected).DimDisabled(false).Children(func() {
				if err := render(target); err != nil {
					r.failPluginView(view, mounted, err, "plugin_callback_failed")
				}
			})
		}
	}
	if (details == nil) != (r.App.PluginDetails == nil) || (navigation == nil) != (r.App.PluginNavigation == nil) || label != r.App.PluginDetailsLabel {
		r.wake()
	}
	r.App.PluginDetails, r.App.PluginDetailsHeader = details, heading
	r.App.PluginNavigation, r.App.PluginDetailsLabel = navigation, label
}
