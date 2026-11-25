package native

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
)

type PanelResource = panel.Resource
type PanelKind = panel.Kind
type PanelState = panel.State
type PanelTab = panel.Tab
type PanelResourceHost = shell.PanelResourceHost
type ProjectResourceAction = sidebar.ProjectResourceAction

const (
	Files    = panel.Files
	Terminal = panel.Terminal
)

func RenderProjectPanelResource(c *ui.Context, state *PanelState, tab PanelTab) {
	panel.RenderProjectResource(c, state, tab)
}

// RegisterTerminal contributes the shared terminal renderer and entry points.
// Products can supply a settings page separately.
func RegisterTerminal(assembly *Assembly) {
	assembly.RegisterPanelResource(PanelResource{
		Kind: Terminal, Label: "terminal", Icon: icons.Terminal, Order: 20,
		Mods: ui.Ctrl, Key: ui.KeyBackquote,
		Available: func(state *PanelState) bool { return state.Worktree && state.TerminalActions != nil },
		Open: func(state *PanelState) {
			if actions := state.TerminalActions; actions != nil && actions.Create != nil {
				actions.Create()
			}
		},
		Render: func(c *ui.Context, state *PanelState, tab PanelTab) { state.TerminalView(c, tab.ID) },
	}, func(host PanelResourceHost) {
		if host.BindTerminal != nil {
			host.App.RegisterProjectResourceAction(ProjectResourceAction{
				Label: "workspace_new_terminal", Icon: icons.Terminal, Order: 10, Open: host.BindTerminal(),
			})
		}
	})
}
