package panel

import "github.com/egoist/mygo/ui"

// TerminalActions connect terminal tabs to their Node terminals. A tab's ID
// is its Node terminal's ID.
type TerminalActions struct {
	// Create starts a terminal in the selected workspace and opens it in the panel.
	Create func()
	// View builds the terminal's screen in its panel tab.
	View func(*ui.Context, string)
	// Pane builds the terminal's screen in a main workspace pane. Its ID is
	// owned by that workspace rather than a conversation's panel tabs.
	Pane func(*ui.Context, string)
	// Close ends the terminal's shell; its tab leaves once the Node has.
	Close  func(string)
	Search func(string, string, bool, int) (int, int)
	// Folder is the folder the terminal runs in, false while it is unknown.
	Folder func(string) (string, bool)
	// Split starts a terminal beside the given one, in its worktree, and
	// returns the new workspace terminal's ID.
	Split func(string) (string, bool)
}

// OpenTerminalTab starts a live terminal or opens the isolated preview view.
// The product decides when and where to offer this action.
func OpenTerminalTab(state *State) {
	if !state.Live {
		state.AddNumberedTab(Terminal, true)
		return
	}
	if actions := state.TerminalActions; actions != nil && actions.Create != nil {
		actions.Create()
	}
}

func RenderTerminalTab(c *ui.Context, state *State, tab Tab) { state.TerminalView(c, tab.ID) }

func FindTerminalTab(state *State, tab Tab) bool {
	state.termOf(tab.ID).find = true
	return true
}

// ReleaseTerminalTab keeps find state when a shell is detached, but forgets
// it when the shell exits or a preview tab is removed.
func ReleaseTerminalTab(state *State, tab Tab, reason TabRelease) {
	if reason != TabDetached || state.TerminalActions == nil || state.TerminalActions.Close == nil {
		delete(state.terms, tab.ID)
	}
}

func fixtureTerminalClose(state *State, tab Tab) CloseConfirmation {
	if actions := state.TerminalActions; actions != nil && actions.Close != nil {
		return CloseConfirmation{Title: "workspace_close_terminal", Close: func() { actions.Close(tab.ID) }}
	}
	return CloseConfirmation{}
}
