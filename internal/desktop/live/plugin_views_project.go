package live

import (
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// projectPluginRegistry is the registry of a project's own controls, scoped to
// its main worktree or the one at path, and the Node that owns the project.
func (r *Runtime) projectPluginRegistry(project *viewmodel.Project, path string) (*Runtime, *desktopplugins.Registry) {
	owner := r.projectOverviewRuntime(project)
	if owner == nil || !owner.controller.Connected() {
		return nil, nil
	}
	worktree, exists := owner.projectWorktree(project, path)
	if !exists {
		return nil, nil
	}
	return owner, owner.pluginRegistry(protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "project"}, Worktree: &worktree.ID})
}

// renderProjectControls draws the controls packages add to the project
// overview, after its own actions.
func (r *Runtime) renderProjectControls(view *ui.Context, project *viewmodel.Project, path string) {
	if owner, registry := r.projectPluginRegistry(project, path); registry != nil {
		owner.renderPluginControls(view, registry, protocol.PluginUiSlotProject, composer.ControlStart)
	}
}

// projectPluginMenu merges the project's enabled package entries with native
// Desktop actions in their declared order. Actions supplied by the product
// do not depend on the project's installed packages.
func (r *Runtime) projectPluginMenu(view *ui.Context, menu *ui.Menu, project *viewmodel.Project) {
	owner := r.projectOverviewRuntime(project)
	if owner == nil {
		return
	}
	side := r.localRuntime().App.Sidebar
	actions := side.Actions
	if actions == nil {
		return
	}
	resources, workflows := actions.ProjectResources, actions.ProjectMenus
	native := func(before int) {
		for {
			switch {
			case len(workflows) > 0 && workflows[0].Order <= before && (len(resources) == 0 || workflows[0].Order < resources[0].Order):
				if render := workflows[0].Context; render != nil {
					render(view, menu, project)
				}
				workflows = workflows[1:]
			case len(resources) > 0 && resources[0].Order <= before:
				resources[0].Menu(view, menu, project)
				resources = resources[1:]
			default:
				return
			}
		}
	}
	if _, registry := r.projectPluginRegistry(project, ""); registry != nil {
		for _, entry := range registry.Entries(protocol.PluginUiSlotProjectMenu) {
			if entry.Retry {
				continue
			}
			native(int(entry.Declaration.Order))
			identity := pluginControlIdentity(entry.Key)
			label := pluginEntryLabel(entry, view.Locale())
			switch entry.Declaration.Kind {
			case "button":
				if menu.Item(label).ID(identity).Disabled(!entry.State.Enabled).Chosen() {
					owner.invokePluginControl(registry, entry.Key, "invoke", nil)
				}
			case "menu":
				menu.Submenu(label, func(child *ui.Menu) {
					owner.projectPluginChoices(view, child, registry, entry, identity, pluginEntryChoices(entry), entry.State.Enabled)
				}).ID(identity).Disabled(!entry.State.Enabled)
			}
		}
	}
	native(math.MaxInt)
}

func (r *Runtime) projectPluginChoices(view *ui.Context, menu *ui.Menu, registry *desktopplugins.Registry, entry desktopplugins.ContributionEntry, identity string, choices []protocol.PluginUiChoice, enabled bool) {
	for index, choice := range choices {
		if index > 0 && !sameChoiceGroup(choices[index-1], choice, view.Locale()) {
			menu.Separator()
		}
		label := pluginLabel(choice.Label, view.Locale())
		id := identity + "/" + choice.ID
		on := enabled && choice.Enabled
		if len(choice.Children) > 0 {
			menu.Submenu(label, func(child *ui.Menu) {
				r.projectPluginChoices(view, child, registry, entry, identity, choice.Children, on)
			}).ID(id).Disabled(!on)
			continue
		}
		if menu.Item(label).ID(id).Disabled(!on).Chosen() {
			r.invokePluginControl(registry, entry.Key, "change", choice.ID)
		}
	}
}

func sameChoiceGroup(first, second protocol.PluginUiChoice, locale string) bool {
	if first.Group == nil || second.Group == nil {
		return first.Group == nil && second.Group == nil
	}
	return pluginLabel(*first.Group, locale) == pluginLabel(*second.Group, locale)
}

// projectMenuRegistries keeps the registry of every listed project current
// while an installed package adds an item of its own to the project menu.
// The system's menu is built at the moment it is asked for and cannot wait
// for a package to mount. Without such a package nothing is mounted.
func (r *Runtime) projectMenuRegistries() {
	declared := map[*Runtime]bool{}
	for _, project := range r.App.Data.Projects {
		owner := r.projectOverviewRuntime(project)
		if owner == nil {
			continue
		}
		wanted, known := declared[owner]
		if !known {
			wanted = r.projectMenuDeclared(r.pluginViewCatalog(owner))
			declared[owner] = wanted
		}
		if wanted {
			r.projectPluginRegistry(project, "")
		}
	}
}

// projectMenuDeclared reports whether a mountable package declares a project
// menu item the application does not draw itself.
func (r *Runtime) projectMenuDeclared(catalog []protocol.PluginInfo) bool {
	for _, info := range catalog {
		extension := info.Extension
		if extension == nil || extension.Desktop == nil || extension.Desktop.UiEntry == nil || extension.Scope != protocol.PluginScopeHost ||
			r.pluginViewsRetired(packageReference(info.Summary)) || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
			continue
		}
		for _, declaration := range extension.Ui {
			if declaration.Slot == protocol.PluginUiSlotProjectMenu && r.pluginViewShows(info, declaration, "project") {
				return true
			}
		}
	}
	return false
}
