package shell

import (
	"slices"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// WorkspaceTerminal is an independent terminal surface in the main workspace.
// The Node owns its process; this record only keeps its presentation open.
type WorkspaceTerminal struct {
	ID    string `json:"id"`
	Title string `json:"title,omitempty"`
	// project belongs to sample terminals only; live ownership comes from Node.
	project string
}

// OpenWorkspaceTerminal opens or focuses an independent main terminal. It does
// not add a conversation's right-panel tab or start a terminal process.
func (a *App) OpenWorkspaceTerminal(id, title string) {
	if id == "" || !a.HasWorkspacePane(panes.Terminal) {
		return
	}
	w := &a.panes
	if t := a.workspaceTerminal(id); t != nil {
		t.Title = title
	} else {
		w.terminals = append(w.terminals, WorkspaceTerminal{ID: id, Title: title})
	}
	a.OpenWorkspacePane(panes.Terminal, id)
}

// KeepWorkspaceTerminal keeps an open main terminal's tab, as a double click
// on its passing tab does.
func (a *App) KeepWorkspaceTerminal(id string) {
	a.KeepWorkspacePane(panes.Terminal, id)
}

// DropWorkspaceTerminal closes only a main terminal's surface and its pane's
// right panel. Its running process remains in the Node's inventory and can be
// opened from terminal activity.
func (a *App) DropWorkspaceTerminal(id string) {
	a.CloseWorkspacePane(panes.Terminal, id)
}

// HasWorkspaceTerminal reports whether the main workspace owns id's surface.
func (a *App) HasWorkspaceTerminal(id string) bool { return a.workspaceTerminal(id) != nil }

// WorkspaceTerminalIDs lists the open main terminal surfaces in opening order.
func (a *App) WorkspaceTerminalIDs() []string {
	ids := make([]string, len(a.panes.terminals))
	for i, t := range a.panes.terminals {
		ids[i] = t.ID
	}
	return ids
}

func (a *App) workspaceTerminal(id string) *WorkspaceTerminal {
	for i := range a.panes.terminals {
		if a.panes.terminals[i].ID == id {
			return &a.panes.terminals[i]
		}
	}
	return nil
}

// ActiveTerminal is the main terminal pane the conversation page shows
// active, "" while a conversation's is or another page shows. Its sidebar row
// is the selected one, and its right panel works in the terminal's worktree,
// not the selected conversation's.
func (a *App) ActiveTerminal() string {
	if t := a.panes.Active; a.Page == ConversationPage && t.Kind == panes.Terminal && a.HasWorkspaceTerminal(t.ID) {
		return t.ID
	}
	return ""
}

// panelProject is the project of the shown panel's pane in the preview: the
// sample terminal's own, or the selected conversation's.
func (a *App) panelProject() *viewmodel.Project {
	target := a.ActiveWorkspacePane()
	if target.Kind == "" {
		return a.Data.ProjectOf(a.Sidebar.Selected)
	}
	if t := a.workspaceTerminal(target.ID); target.Kind == panes.Terminal && t != nil {
		for _, pr := range a.Data.Projects {
			if pr.ID == t.project {
				return pr
			}
		}
	}
	return nil
}

func (a *App) sampleWorkspaceTerminal(project string) string {
	for n := 1; ; n++ {
		id := "workspace-terminal/" + strconv.Itoa(n)
		if !a.HasWorkspaceTerminal(id) {
			a.OpenWorkspaceTerminal(id, "")
			a.workspaceTerminal(id).project = project
			return id
		}
	}
}

// TerminalWorkspacePane contributes the independent terminal surface to a
// product that provides a terminal service.
func TerminalWorkspacePane() WorkspacePane {
	return WorkspacePane{
		Kind: panes.Terminal, Icon: icons.Terminal,
		Exists: (*App).HasWorkspaceTerminal,
		Title: func(a *App, c *ui.Context, id string) string {
			if terminal := a.workspaceTerminal(id); terminal != nil && terminal.Title != "" {
				return terminal.Title
			}
			return i18n.T(c, "terminal")
		},
		Render: (*App).terminalPane,
		Close: func(a *App, id string) {
			a.panes.terminals = slices.DeleteFunc(a.panes.terminals, func(t WorkspaceTerminal) bool { return t.ID == id })
		},
		Split: func(a *App, id string) (string, bool) {
			if actions := a.Panel.TerminalActions; actions != nil && actions.Split != nil {
				return actions.Split(id)
			}
			project := ""
			if terminal := a.workspaceTerminal(id); terminal != nil {
				project = terminal.project
			}
			return a.sampleWorkspaceTerminal(project), true
		},
	}
}

// terminalPane is a terminal's pane: the screen of the tab's Node terminal,
// which shows here instead of in its tab while the pane is displayed. The
// preview shows a static placeholder, marked as one; no shell starts.
func (a *App) terminalPane(c *ui.Context, id string) {
	p := theme.Of(c)
	if actions := a.Panel.TerminalActions; actions != nil && actions.Pane != nil {
		ui.Column(c.Key("terminal")).Grow(1).MinHeight(0).Background(p.Background).Children(func() { actions.Pane(c, id) })
		return
	}
	if a.Demo() {
		a.Panel.TerminalView(c, id)
		return
	}
	ui.Column(c.Key("terminal")).Grow(1).MinHeight(0).Background(p.Muted).Center().Gap(8).Padding(24).Children(func() {
		ui.Icon(c, icons.Terminal).FontSize(32).TextColor(p.MutedForeground)
		kit.Tag(c, i18n.T(c, "preview"))
		ui.Text(c, i18n.T(c, "workspace_terminal_preview")).FontSize(14).TextAlign(ui.Center).TextColor(p.MutedForeground)
	})
}
