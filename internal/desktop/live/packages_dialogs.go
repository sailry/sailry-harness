package live

import (
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type cliLauncher struct {
	owner                  *Runtime
	worktree               protocol.Worktree
	tools                  []protocol.TerminalToolsToolInfo
	open, reading, pending bool
	launching              protocol.TerminalToolsTool // the tool whose terminal is being opened
}

func (r *Runtime) packageDialogs(c *ui.Context) {
	r.skillSelectionDialog(c)
	r.cliFrame(c)
}

func (r *Runtime) skillSelectionDialog(c *ui.Context) {
	actions := r.App.Settings.PackageActions
	if actions == nil || actions.Selection == nil || !actions.Selection.Open {
		return
	}
	selection := actions.Selection
	title := i18n.T(c, "skills_install")
	width, height := c.Size()
	kit.Dialog(c.Key("skill-selection"), &selection.Open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("candidates")).MaxHeight(min(400, height*0.55)).Gap(8).Children(func() {
			for index := range selection.Candidates {
				candidate := &selection.Candidates[index]
				ui.Row(c.Key(index)).Padding(8).Gap(8).Children(func() {
					ui.Checkbox(c.Key("selected"), &candidate.Selected, candidate.Name).Disabled(selection.Pending)
					ui.Text(c, candidate.Description).FontSize(14).TextColor(theme.Of(c).MutedForeground).Grow(1).Shrink(1)
				})
			}
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(selection.Pending).OnClick(func() { selection.Open = false })
			var indices []int
			for index, candidate := range selection.Candidates {
				if candidate.Selected {
					indices = append(indices, index)
				}
			}
			kit.PrimaryBusy(c.Key("install"), i18n.T(c, "plugins_install"), len(indices) == 0, selection.Pending).OnClick(func() {
				selection.Pending = true
				selection.Install(indices, func(ok bool) {
					selection.Pending = false
					if ok {
						selection.Open = false
					}
				})
			})
		})
	}).Width(min(560, width-48)).Label(title)
	if !selection.Open && !selection.Pending && selection.Cancel != nil {
		selection.Cancel()
		selection.Cancel = nil
	}
}

func (r *Runtime) openCLILauncher(project *viewmodel.Project, selectedPath string) {
	root := r.localRuntime()
	owner := root.projectOverviewRuntime(project)
	if owner == nil {
		return
	}
	worktree, ok := owner.projectWorktree(project, selectedPath)
	if !ok {
		return
	}
	launcher := &cliLauncher{owner: owner, worktree: worktree, open: true, reading: true}
	root.packages.cli = launcher
	owner.async(func() {
		out, err := owner.controller.Do(owner.ctx, protocol.Command{Kind: "list_terminal_tools", Data: protocol.CommandListTerminalToolsPayload{Worktree: worktree.ID}})
		tools, valid := detail[[]protocol.TerminalToolsToolInfo](out.Data)
		if err == nil && (!valid || out.Kind != "terminal_tools") {
			err = errors.New("unexpected terminal tools response")
		}
		owner.enqueue(func(c *ui.Context) {
			launcher.reading = false
			launcher.tools = tools
			owner.packageReport(c, err)
		})
	})
}

func (r *Runtime) cliFrame(c *ui.Context) {
	if c == nil || r.packages == nil || r.packages.cli == nil || !r.packages.cli.open {
		return
	}
	state := r.packages.cli
	title := i18n.T(c, "cli_launch")
	width, height := c.Size()
	kit.Dialog(c.Key("cli-launcher"), &state.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Text(c, state.worktree.Path).FontSize(14).SingleLine().TextColor(theme.Of(c).MutedForeground)
		ui.Scroll(c.Key("profiles")).MaxHeight(min(300, height*0.5)).Gap(4).Children(func() {
			shown := 0
			for _, candidate := range state.tools {
				if !candidate.Available {
					continue
				}
				shown++
				label := string(candidate.Tool)
				kit.Ghost(c.Key(label), label, false).FillWidth().Padding(12).Gap(12).Justify(ui.Start).Disabled(state.pending).Children(func() {
					ui.Icon(c, icons.Terminal).FontSize(16)
					ui.Text(c, label).FontSize(14).Grow(1)
					if state.pending && state.launching == candidate.Tool {
						kit.Busy(c)
					} else {
						ui.Icon(c, icons.ChevronRight).FontSize(16)
					}
				}).OnClick(func() { r.launchCLI(c, state, candidate.Tool) })
			}
			if state.reading {
				kit.Busy(c).Label(title)
			} else if shown == 0 {
				kit.Result(c, kit.ResultProps{Icon: icons.Terminal, Title: i18n.T(c, "unavailable")})
			}
		})
		kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { state.open = false })
	}).Width(min(480, width-48)).Label(title)
}

func (r *Runtime) launchCLI(c *ui.Context, state *cliLauncher, tool protocol.TerminalToolsTool) {
	if state.pending {
		return
	}
	state.pending, state.launching = true, tool
	palette := theme.Of(c)
	color := func(value ui.Color) protocol.TerminalRgb {
		return protocol.TerminalRgb{Red: value.R, Green: value.G, Blue: value.B}
	}
	appearance := protocol.TerminalAppearance{Foreground: color(palette.Foreground), Background: color(palette.Background), ColorScheme: protocol.TerminalColorSchemeLight}
	if palette.Dark {
		appearance.ColorScheme = protocol.TerminalColorSchemeDark
	}
	for index, value := range []ui.Color{palette.Background, palette.Danger, palette.Green, palette.Yellow, palette.Blue, palette.Magenta, palette.Info, palette.Foreground, palette.MutedForeground, palette.Danger, palette.Green, palette.Yellow, palette.Blue, palette.Magenta, palette.Info, palette.Foreground} {
		appearance.Palette[index] = color(value)
	}
	launch := protocol.TerminalLaunch{Worktree: state.worktree.ID, Access: protocol.ProcessAccessUnrestricted, Viewport: protocol.TerminalViewport{Columns: 100, Rows: 30}, Appearance: appearance}
	state.owner.executeScoped("cli/"+state.worktree.ID.String(), protocol.Command{Kind: "open_tool_terminal", Data: protocol.CommandOpenToolTerminalPayload{Tool: tool, Launch: launch}}, func(c *ui.Context, out protocol.Output, err error) {
		state.pending = false
		state.owner.packageReport(c, err)
		if err == nil {
			if info, valid := detail[protocol.TerminalInfo](out.Data); valid && out.Kind == "terminal" {
				state.open = false
				r.openLiveTerminal(state.owner, info)
				r.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: info.ID.String()})
				r.App.PanelOpen = true
			}
		}
	})
}

func (r *Runtime) pluginReferences() []composer.Reference {
	if r.packages == nil {
		r.packages = newPackageState()
	}
	var rows []composer.Reference
	for _, summary := range r.snapshot().Plugins {
		if !summary.Enabled || summary.Compatibility.Kind != "compatible" {
			continue
		}
		if info, exists := r.packages.infos[summary.Name]; exists {
			if info.Extension == nil || info.Extension.Scope == protocol.PluginScopeDesktop {
				continue
			}
			view := packageView(info, r.packageLocale())
			rows = append(rows, composer.Reference{ID: summary.Name, Kind: composer.Plugin, Label: view.Name, Description: view.Description, Group: "reference_group_plugins", Icon: icons.Puzzle})
		} else if !r.packages.reads[summary.Name] && r.packages.failures[summary.Name] != packageVersion(summary) {
			r.readPackage(summary.Name, packageVersion(summary), nil)
		}
	}
	return rows
}
