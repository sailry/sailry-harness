package native

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
)

type WorkspacePane = shell.WorkspacePane
type WorkspacePaneKind = panes.Kind
type WorkspacePaneTarget = panes.Target

func TerminalWorkspacePane() WorkspacePane { return shell.TerminalWorkspacePane() }

type PanelResource = panel.Resource
type PanelKind = panel.Kind
type PanelState = panel.State
type PanelTab = panel.Tab
type PanelResourceHost = shell.PanelResourceHost
type CloseConfirmation = panel.CloseConfirmation
type TabRelease = panel.TabRelease
type ProjectResourceAction = sidebar.ProjectResourceAction

const (
	Files    = panel.Files
	Terminal = panel.Terminal

	TabRemoved  = panel.TabRemoved
	TabDetached = panel.TabDetached
)

func RenderProjectPanelResource(c *ui.Context, state *PanelState, tab PanelTab) {
	panel.RenderProjectResource(c, state, tab)
}

// OpenTerminalTab starts a terminal with the bound PTY service, or opens an
// isolated terminal in a product preview.
func OpenTerminalTab(state *PanelState) { panel.OpenTerminalTab(state) }

func RenderTerminalTab(c *ui.Context, state *PanelState, tab PanelTab) {
	panel.RenderTerminalTab(c, state, tab)
}

func FindTerminalTab(state *PanelState, tab PanelTab) bool { return panel.FindTerminalTab(state, tab) }

func ReleaseTerminalTab(state *PanelState, tab PanelTab, reason TabRelease) {
	panel.ReleaseTerminalTab(state, tab, reason)
}
