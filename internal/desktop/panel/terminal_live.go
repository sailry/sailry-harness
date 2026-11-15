package panel

import "github.com/egoist/mygo/ui"

// TerminalActions connect terminal tabs to their Node terminals. A tab's ID
// is its Node terminal's ID.
type TerminalActions struct {
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
