package main

import (
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

// exitGuard asks before a window close or a quit drops unsaved documents or
// stops local work. MyGo lets Window.OnClose and Application.OnBeforeQuit
// prevent the close; the prompt's answer then repeats it with the guard
// passing. Everything runs on the main thread.
type exitGuard struct {
	windows   func() []*desktopWindow
	active    func() *desktopWindow
	documents func(*desktopWindow) []string // unsaved documents of a window
	save      func(*desktopWindow)
	saving    func(*desktopWindow) bool
	work      func() bool          // the local Node runs a task or a terminal
	raise     func(*desktopWindow) // shows the window that asks
	shut      func(*desktopWindow) // closes one window again
	quit      func()               // quits again
	// passing is a quit that was confirmed, needs no prompt or cannot be
	// cancelled: its closes go through.
	passing bool
}

// ask opens the prompt in entry for the windows that are about to close. It
// reports whether one shows, which keeps the windows open.
func (g *exitGuard) ask(entry *desktopWindow, closing []*desktopWindow, quit bool, proceed func()) bool {
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
	if len(documents()) == 0 && !(quit && g.work()) {
		return false
	}
	entry.app.ConfirmClose(shell.CloseRequest{
		Documents: documents,
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
	if entry != nil && g.ask(entry, windows, true, g.pass) {
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
		return g.ask(entry, windows, true, g.pass)
	}
	return g.ask(entry, []*desktopWindow{entry}, false, func() {
		entry.passing = true
		g.shut(entry)
	})
}

// pass quits without asking again.
func (g *exitGuard) pass() {
	g.passing = true
	g.quit()
}
