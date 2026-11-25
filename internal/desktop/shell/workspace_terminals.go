package shell

import (
	"slices"
	"strconv"

	"github.com/sailry/sailry-harness/internal/desktop/panes"
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
	if id == "" {
		return
	}
	w := &a.panes
	if t := a.workspaceTerminal(id); t != nil {
		t.Title = title
	} else {
		w.terminals = append(w.terminals, WorkspaceTerminal{ID: id, Title: title})
	}
	a.Navigate(ConversationPage)
	w.seen = a.Sidebar.Selected
	a.focusPane(panes.Target{Kind: panes.Terminal, ID: id})
}

// KeepWorkspaceTerminal keeps an open main terminal's tab, as a double click
// on its passing tab does.
func (a *App) KeepWorkspaceTerminal(id string) {
	if a.workspaceTerminal(id) != nil {
		a.panes.Keep(panes.Target{Kind: panes.Terminal, ID: id})
	}
}

// DropWorkspaceTerminal closes only a main terminal's surface and its pane's
// right panel. Its running process remains in the Node's inventory and can be
// opened from terminal activity.
func (a *App) DropWorkspaceTerminal(id string) {
	w := &a.panes
	t := panes.Target{Kind: panes.Terminal, ID: id}
	w.Detach(t)
	w.terminals = slices.DeleteFunc(w.terminals, func(t WorkspaceTerminal) bool { return t.ID == id })
	delete(w.focus, t)
	delete(w.groupFocus, t)
	a.dropTerminalPanel(id)
	if w.Active == t {
		w.Active = session(a.Sidebar.Selected)
	}
	a.focusPane(w.Active)
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
	if t := a.workspaceTerminal(a.ActiveTerminal()); t != nil {
		for _, pr := range a.Data.Projects {
			if pr.ID == t.project {
				return pr
			}
		}
		return nil
	}
	return a.Data.ProjectOf(a.Sidebar.Selected)
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

// FocusedConversation is the conversation receiving main-workspace actions,
// empty while a terminal or another page is focused.
func (a *App) FocusedConversation() string {
	if a.Page != ConversationPage || a.panes.Active.Kind == panes.Terminal {
		return ""
	}
	if a.panes.Displayed() != nil {
		return a.panes.Active.ID
	}
	return a.Sidebar.Selected
}
