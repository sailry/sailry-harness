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
	documents  func(*desktopWindow) []shell.CloseDocument
	save       func(*desktopWindow)
	saving     func(*desktopWindow) bool
	flush      func(*desktopWindow, func(bool)) bool // settles pending settings saves
	work       func(*desktopWindow) bool             // the local Node runs a task or a terminal
	raise      func(*desktopWindow)                  // shows the window that asks
	shut       func(*desktopWindow)                  // closes one window again
	quit       func()                                // quits again
	quitPrompt *desktopWindow
	quitSaving bool
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
func (g *exitGuard) ask(entry *desktopWindow, action closeAction, proceed func()) bool {
	if g.quitPrompt != nil {
		if g.quitPrompt.app.Closing() {
			return true
		}
		g.quitPrompt = nil
	}
	if entry.app.Closing() {
		return true
	}
	closing := g.windows
	if action == closeWindow {
		closing = func() []*desktopWindow { return []*desktopWindow{entry} }
	}
	documents := func() []shell.CloseDocument {
		var all []shell.CloseDocument
		for _, window := range closing() {
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
			for _, window := range closing() {
				g.save(window)
			}
		},
		Saving: func() bool {
			for _, window := range closing() {
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

// beforeQuit reports whether the quit waits for settings saves or a prompt.
// An operating-system session end starts the saves and passes immediately.
func (g *exitGuard) beforeQuit(sessionEnding bool) bool {
	windows := g.windows()
	if sessionEnding {
		g.passing = true
		g.quitSaving = false
		if g.flush != nil {
			for _, window := range windows {
				g.flush(window, nil)
			}
		}
		return false
	}
	if g.quitSaving {
		return true
	}
	g.quitSaving = true
	if g.flushWindows(windows) {
		return true
	}
	g.quitSaving = false
	if g.passing {
		return false
	}
	entry := g.active()
	if entry == nil && len(windows) > 0 {
		entry = windows[0]
	}
	if entry != nil && g.ask(entry, quitApplication, g.pass) {
		return true
	}
	g.passing = true
	return false
}

// flushWindows keeps the windows alive until every editor has settled its
// saves. A failed save leaves the draft and its existing retry UI available.
func (g *exitGuard) flushWindows(windows []*desktopWindow) bool {
	if g.flush == nil {
		return false
	}
	remaining := len(windows)
	ready, waiting, started := true, false, false
	resume := func() {
		g.quitSaving = false
		if ready {
			g.quit()
		} else {
			g.passing = false
		}
	}
	done := func(saved bool) {
		ready = ready && saved
		remaining--
		if started && remaining == 0 {
			resume()
		}
	}
	for _, window := range windows {
		if g.flush(window, done) {
			waiting = true
		} else {
			remaining--
		}
	}
	started = true
	if waiting && remaining == 0 {
		resume()
	}
	return waiting
}

// closing reports whether a close waits for settings saves or a prompt.
// Closing the last window quits, so it asks as a quit does.
func (g *exitGuard) closing(entry *desktopWindow) bool {
	if g.quitSaving || entry.closeSaving {
		return true
	}
	if g.passing {
		return false
	}
	if g.flush != nil {
		entry.closeSaving = true
		if g.flush(entry, func(saved bool) {
			entry.closeSaving = false
			if saved && !g.quitSaving && !g.passing {
				g.shut(entry)
			} else if !saved {
				entry.passing = false
			}
		}) {
			return true
		}
		entry.closeSaving = false
	}
	if entry.passing {
		return false
	}
	if len(g.windows()) == 1 {
		return g.ask(entry, quitApplication, g.pass)
	}
	return g.ask(entry, closeWindow, func() {
		entry.passing = true
		g.shut(entry)
	})
}

// pass quits without asking again.
func (g *exitGuard) pass() {
	g.passing = true
	g.quit()
}
