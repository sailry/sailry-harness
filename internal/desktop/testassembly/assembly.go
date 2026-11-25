// Package testassembly supplies native contribution fixtures for core Desktop tests.
// These pages exercise shell routing and settings binding, not a shipped product's UI.
package testassembly

import (
	"github.com/egoist/mygo/ui"
	harness "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

func Full() shell.Assembly {
	assembly := harness.Harness()
	RegisterExecutionFixture(&assembly)
	RegisterWorktreeFixture(&assembly)
	RegisterTerminalFixture(&assembly)
	Register(&assembly)
	if len(assembly.WorkspacePages) == 3 {
		assembly.WorkspacePages[1], assembly.WorkspacePages[2] = assembly.WorkspacePages[2], assembly.WorkspacePages[1]
	}
	assembly.SettingsPages = append(assembly.SettingsPages, settings.NativePage{
		Section: settings.Terminal, Group: "settings_group_tools", Icon: icons.Terminal, Order: 100, Render: terminal,
	})
	assembly.Locales = append(assembly.Locales, Locales)
	return assembly
}

func RegisterTerminalFixture(assembly *shell.Assembly) {
	native.RegisterTerminal(assembly)
	assembly.PluginControlReplacements = append(assembly.PluginControlReplacements,
		shell.PluginControlReplacement{Package: "commands", Slot: protocol.PluginUiSlotProject, ID: "terminal-cli"},
		shell.PluginControlReplacement{Package: "commands", Slot: protocol.PluginUiSlotProjectMenu, ID: "terminal-menu-new"},
		shell.PluginControlReplacement{Package: "commands", Slot: protocol.PluginUiSlotProjectMenu, ID: "terminal-menu-cli"},
	)
}

// Register contributes a change-resource probe with both central and details surfaces.
func Register(assembly *shell.Assembly) {
	assembly.RegisterPanelResource(panel.Resource{
		Kind: panel.Changes, Label: "git", Icon: icons.Network, Order: 40,
		Mods: ui.Ctrl | ui.Shift, Key: ui.KeyG, Render: panel.RenderProjectResource,
	})
	assembly.WorkspacePages = append(assembly.WorkspacePages, shell.NativePage{
		Page: shell.GitPage, Feature: shell.Feature{ID: "page:git", Icon: icons.Network, Label: "git", DefaultPinned: true},
		Usage: protocol.FeatureUsePageGit, Resource: panel.Changes, CheckpointHistory: true,
		Render: func(c *ui.Context, app *shell.App) {
			app.RenderResource(c, func(project *panel.Project) {
				column := ui.Column(c.Key(panel.Changes)).Grow(1).MinWidth(0).MinHeight(0)
				width := column.LayoutBounds().W
				column.Children(func() {
					project.Git.Show(project.Git.Change())
					project.ActionDialog(c)
					project.DocumentTabs(c, panel.Changes)
					project.GitDiff(c, project.Git.Change(), width)
					project.GitDialogs(c)
				})
			})
		},
		RenderDetails: func(c *ui.Context, app *shell.App, project *panel.Project) {
			ui.Column(c.Key(panel.Changes)).Grow(1).MinWidth(0).MinHeight(0).Children(func() {
				project.GitRepository(c, true, false, project.Git.Show, app.RevealResourceFile())
			})
		},
		BindKeys: func(c *ui.Context, project *panel.Project, scope ui.Element) { project.GitKeys(c, scope) },
	})
}
