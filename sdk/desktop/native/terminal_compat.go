package native

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
)

// RegisterTerminal installs the historical Terminal workspace pane, panel and project action.
//
// Deprecated: register TerminalWorkspacePane and the panel resource in the
// product module that supplies these entry points.
func RegisterTerminal(assembly *Assembly) {
	assembly.RegisterWorkspacePane(TerminalWorkspacePane())
	assembly.RegisterPanelResource(PanelResource{
		Kind: Terminal, Label: "terminal", Icon: icons.Terminal, Order: 20,
		Mods: ui.Ctrl, Key: ui.KeyBackquote, Numbered: true,
		Available: func(state *PanelState) bool { return state.Worktree && state.TerminalActions != nil },
		Open: func(state *PanelState) {
			if actions := state.TerminalActions; actions != nil && actions.Create != nil {
				actions.Create()
			}
		},
		Render: RenderTerminalTab, Find: FindTerminalTab, Release: ReleaseTerminalTab,
		ConfirmClose: func(state *PanelState, tab PanelTab) CloseConfirmation {
			if actions := state.TerminalActions; actions != nil && actions.Close != nil {
				return CloseConfirmation{Title: "workspace_close_terminal", Close: func() {
					if state.OnTerminalClose != nil {
						state.OnTerminalClose(tab)
					}
					actions.Close(tab.ID)
				}}
			}
			return CloseConfirmation{}
		},
	}, func(host PanelResourceHost) {
		if host.BindTerminal != nil {
			host.App.RegisterProjectResourceAction(ProjectResourceAction{
				Label: "workspace_new_terminal", Icon: icons.Terminal, Order: 10, Open: host.BindTerminal(),
			})
		}
	})
}
