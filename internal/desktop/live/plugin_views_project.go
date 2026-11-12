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
	if owner == nil {
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

// The orders the bundled packages give their project menu entries the
// application draws itself: the commands package's New terminal and the
// worktrees package's Manage worktrees.
const (
	projectMenuTerminalOrder  = 10
	projectMenuWorktreesOrder = 30
)

// projectPluginMenu fills a project menu's project_menu slot with the
// entries of the packages enabled on the project's Node, in their order: a
// button is an item, a menu a submenu of its choices. The commands package's
// New terminal and the worktrees package's entry are the application's own
// controls, as it owns terminals and worktrees, shown while their package is
// enabled. The commands package's CLI submenu and the git package's Manage
// branches are not shown (AGENTS.md, Removed UI): the project_menu slot is
// not mounted.
func (r *Runtime) projectPluginMenu(view *ui.Context, menu *ui.Menu, project *viewmodel.Project) {
	owner := r.projectOverviewRuntime(project)
	if owner == nil {
		return
	}
	side := r.localRuntime().App.Sidebar
	terminal, worktrees := owner.packageEnabled("commands"), owner.packageEnabled(pluginWorktreesPackage)
	native := func(before int) {
		if terminal && projectMenuTerminalOrder <= before {
			side.TerminalItem(view, menu, project)
			terminal = false
		}
		if worktrees && projectMenuWorktreesOrder <= before {
			side.WorktreeMenu(view, menu, project)
			worktrees = false
		}
	}
	if _, registry := r.projectPluginRegistry(project, ""); registry != nil {
		for _, entry := range registry.Entries(protocol.PluginUiSlotProjectMenu) {
			if entry.Retry {
				continue
			}
			native(int(entry.Declaration.Order))
			label := pluginLabel(entry.Declaration.Label, view.Locale())
			switch entry.Declaration.Kind {
			case "button":
				if menu.Item(label).Disabled(!entry.State.Enabled).Chosen() {
					owner.invokePluginControl(registry, entry.Key, "invoke", nil)
				}
			case "menu":
				menu.Submenu(label, func(child *ui.Menu) {
					owner.projectPluginChoices(view, child, registry, entry, pluginEntryChoices(entry), entry.State.Enabled)
				})
			}
		}
	}
	native(math.MaxInt)
}

// packageEnabled reports whether the Node has the package enabled and can
// run it.
func (r *Runtime) packageEnabled(name string) bool {
	for _, summary := range r.snapshot().Plugins {
		if summary.Name == name {
			return summary.Enabled && summary.Compatibility.Kind == "compatible"
		}
	}
	return false
}

func (r *Runtime) projectPluginChoices(view *ui.Context, menu *ui.Menu, registry *desktopplugins.Registry, entry desktopplugins.ContributionEntry, choices []protocol.PluginUiChoice, enabled bool) {
	for index, choice := range choices {
		if index > 0 && !sameChoiceGroup(choices[index-1], choice, view.Locale()) {
			menu.Separator()
		}
		label := pluginLabel(choice.Label, view.Locale())
		if len(choice.Children) > 0 {
			menu.Submenu(label, func(child *ui.Menu) {
				r.projectPluginChoices(view, child, registry, entry, choice.Children, enabled && choice.Enabled)
			}).Disabled(!enabled || !choice.Enabled)
			continue
		}
		if menu.Item(label).Disabled(!enabled || !choice.Enabled).Chosen() {
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
			wanted = projectMenuDeclared(r.pluginViewCatalog(owner))
			declared[owner] = wanted
		}
		if wanted {
			r.projectPluginRegistry(project, "")
		}
	}
}

// projectMenuDeclared reports whether a mountable package declares a project
// menu item the application does not draw itself.
func projectMenuDeclared(catalog []protocol.PluginInfo) bool {
	for _, info := range catalog {
		extension := info.Extension
		if extension == nil || extension.Desktop == nil || extension.Desktop.UiEntry == nil || extension.Scope != protocol.PluginScopeHost ||
			unmountedPlugin(info.Summary.Name) || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
			continue
		}
		for _, declaration := range pluginViewDeclarations(info.Summary.Name, extension.Ui, "project") {
			if declaration.Slot == protocol.PluginUiSlotProjectMenu {
				return true
			}
		}
	}
	return false
}
