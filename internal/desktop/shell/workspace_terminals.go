package shell

import (
	"slices"
	"strconv"

	"github.com/sailry/sailry-harness/internal/desktop/panes"
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

// DropWorkspaceTerminal closes only a main terminal's surface. Its running
// process remains in the Node's inventory and can be opened from terminal activity.
func (a *App) DropWorkspaceTerminal(id string) {
	w := &a.panes
	t := panes.Target{Kind: panes.Terminal, ID: id}
	w.Detach(t)
	w.terminals = slices.DeleteFunc(w.terminals, func(t WorkspaceTerminal) bool { return t.ID == id })
	delete(w.focus, t)
	delete(w.groupFocus, t)
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
