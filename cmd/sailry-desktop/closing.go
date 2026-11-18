package main

import (
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

// exitGuard confirms application quits and protects unsaved documents and
// local work. The prompt's answer repeats the close with the guard passing.
// Everything runs on the main thread.
type exitGuard struct {
	windows    func() []*desktopWindow
	active     func() *desktopWindow
	documents  func(*desktopWindow) []string // unsaved documents of a window
	save       func(*desktopWindow)
	saving     func(*desktopWindow) bool
	work       func(*desktopWindow) bool // the local Node runs a task or a terminal
	raise      func(*desktopWindow)      // shows the window that asks
	shut       func(*desktopWindow)      // closes one window again
	quit       func()                    // quits again
	quitPrompt *desktopWindow
	// passing is a quit that was confirmed, needs no prompt or cannot be
	// cancelled: its closes go through.
	passing bool
}

type closeAction int

const (
	closeWindow closeAction = iota
	quitApplication
	restartProfile
)

// ask opens the prompt in entry for the windows that are about to close. It
// reports whether the action must wait for a prompt.
func (g *exitGuard) ask(entry *desktopWindow, closing []*desktopWindow, action closeAction, proceed func()) bool {
	if g.quitPrompt != nil {
		if g.quitPrompt.app.Closing() {
			return true
		}
		g.quitPrompt = nil
	}
	if entry.app.Closing() {
		return true
	}
	documents := func() []string {
		var all []string
		for _, window := range closing {
			all = append(all, g.documents(window)...)
		}
		return all
	}
	stopsLocalWork := action != closeWindow && g.work(entry)
	if action != quitApplication && !stopsLocalWork && len(documents()) == 0 {
		return false
	}
	if action == quitApplication {
		g.quitPrompt = entry
	}
	entry.app.ConfirmClose(shell.CloseRequest{
		Documents:      documents,
		StopsLocalWork: stopsLocalWork,
		Save: func() {
			for _, window := range closing {
				g.save(window)
			}
		},
		Saving: func() bool {
			for _, window := range closing {
				if g.saving(window) {
					return true
				}
			}
			return false
		},
		Proceed: proceed,
	})
	g.raise(entry)
	return true
}

// beforeQuit reports whether the quit must wait for the prompt. A session
// that ends cannot wait: its quit passes as it is.
func (g *exitGuard) beforeQuit(sessionEnding bool) bool {
	if sessionEnding || g.passing {
		g.passing = true
		return false
	}
	windows := g.windows()
	entry := g.active()
	if entry == nil && len(windows) > 0 {
		entry = windows[0]
	}
	if entry != nil && g.ask(entry, windows, quitApplication, g.pass) {
		return true
	}
	g.passing = true
	return false
}

// closing reports whether the window's close must wait for the prompt.
// Closing the last window quits, so it asks as a quit does.
func (g *exitGuard) closing(entry *desktopWindow) bool {
	if g.passing || entry.passing {
		return false
	}
	if windows := g.windows(); len(windows) == 1 {
		return g.ask(entry, windows, quitApplication, g.pass)
	}
	return g.ask(entry, []*desktopWindow{entry}, closeWindow, func() {
		entry.passing = true
		g.shut(entry)
	})
}

// pass quits without asking again.
func (g *exitGuard) pass() {
	g.passing = true
	g.quit()
}
