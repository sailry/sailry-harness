package panel

import "github.com/egoist/mygo/ui"

// TerminalActions connect terminal tabs to their Node terminals. A tab's ID
// is its Node terminal's ID.
type TerminalActions struct {
	// View builds the terminal's screen in its panel tab.
	View func(*ui.Context, string)
	// Pane builds the same terminal's screen in a split pane. A terminal
	// shows in one place at a time: its pane while that is displayed.
	Pane   func(*ui.Context, string)
	Close  func(string)
	Search func(string, string, bool, int) (int, int)
	// Folder is the folder the terminal runs in, false while it is unknown.
	Folder func(string) (string, bool)
	// Split starts a terminal beside the given one, in its worktree, and
	// returns the new terminal's tab ID.
	Split func(string) (string, bool)
}
