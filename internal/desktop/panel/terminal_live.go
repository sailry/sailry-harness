package panel

import "github.com/egoist/mygo/ui"

type TerminalActions struct {
	View   func(*ui.Context, string)
	Close  func(string)
	Search func(string, string, bool, int) (int, int)
}
