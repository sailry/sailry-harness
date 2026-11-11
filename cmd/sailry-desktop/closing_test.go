package main

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

// guardTest is an exitGuard over windows with the given unsaved documents.
type guardTest struct {
	guard   *exitGuard
	windows []*desktopWindow
	unsaved map[*desktopWindow][]string
	work    bool
	raised  []*desktopWindow
	shut    []*desktopWindow
	quits   int
}

func newGuardTest(documents ...[]string) *guardTest {
	g := &guardTest{unsaved: map[*desktopWindow][]string{}}
	for _, names := range documents {
		window := &desktopWindow{app: shell.New(preview.Load())}
		g.windows = append(g.windows, window)
		g.unsaved[window] = names
	}
	g.guard = &exitGuard{
		windows:   func() []*desktopWindow { return g.windows },
		active:    func() *desktopWindow { return g.windows[len(g.windows)-1] },
		documents: func(window *desktopWindow) []string { return g.unsaved[window] },
		save:      func(*desktopWindow) {},
		saving:    func(*desktopWindow) bool { return false },
		work:      func() bool { return g.work },
		raise:     func(window *desktopWindow) { g.raised = append(g.raised, window) },
		shut:      func(window *desktopWindow) { g.shut = append(g.shut, window) },
		quit:      func() { g.quits++ },
	}
	return g
}

func TestExitGuard(t *testing.T) {
	t.Run("nothing to lose closes and quits without a prompt", func(t *testing.T) {
		g := newGuardTest(nil, nil)
		if g.guard.closing(g.windows[0]) || g.windows[0].app.Closing() {
			t.Fatal("a clean window asked before closing")
		}
		if g.guard.beforeQuit(false) || !g.guard.passing || len(g.raised) != 0 {
			t.Fatal("a clean quit asked")
		}
	})
	t.Run("a window with drafts asks for itself only", func(t *testing.T) {
		g := newGuardTest([]string{"a.go"}, []string{"b.go"})
		first := g.windows[0]
		if !g.guard.closing(first) || !first.app.Closing() || g.windows[1].app.Closing() {
			t.Fatal("the closing window did not ask")
		}
		if !slices.Equal(g.raised, []*desktopWindow{first}) {
			t.Fatalf("raised %v", g.raised)
		}
		if !g.guard.closing(first) || len(g.raised) != 1 {
			t.Fatal("a repeated close asked again")
		}
		g.work = true
		if g.guard.closing(g.windows[1]) != true {
			t.Fatal("the other window's drafts did not ask")
		}
	})
	t.Run("local work does not ask before one of several windows closes", func(t *testing.T) {
		g := newGuardTest(nil, nil)
		g.work = true
		if g.guard.closing(g.windows[0]) {
			t.Fatal("closing a window that stops nothing asked")
		}
	})
	t.Run("the last window closes as a quit", func(t *testing.T) {
		g := newGuardTest(nil)
		g.work = true
		if !g.guard.closing(g.windows[0]) || !g.windows[0].app.Closing() {
			t.Fatal("closing the last window over local work did not ask")
		}
	})
	t.Run("a quit asks once for every window's documents", func(t *testing.T) {
		g := newGuardTest([]string{"a.go"}, []string{"b.go"})
		if !g.guard.beforeQuit(false) || g.guard.passing {
			t.Fatal("the quit did not wait")
		}
		active := g.windows[1]
		if !active.app.Closing() || g.windows[0].app.Closing() || !slices.Equal(g.raised, []*desktopWindow{active}) {
			t.Fatal("the active window alone must ask")
		}
		if !g.guard.beforeQuit(false) || !g.guard.closing(active) || len(g.raised) != 1 {
			t.Fatal("a repeated quit or close of the asking window asked again or passed")
		}
	})
	t.Run("a confirmed quit passes its closes", func(t *testing.T) {
		g := newGuardTest([]string{"a.go"})
		g.guard.pass()
		if g.quits != 1 || g.guard.beforeQuit(false) || g.guard.closing(g.windows[0]) || g.windows[0].app.Closing() {
			t.Fatal("a confirmed quit was asked again")
		}
	})
	t.Run("a confirmed window close passes", func(t *testing.T) {
		g := newGuardTest([]string{"a.go"}, nil)
		g.windows[0].passing = true
		if g.guard.closing(g.windows[0]) {
			t.Fatal("a confirmed close was asked again")
		}
	})
	t.Run("an ending session cannot wait", func(t *testing.T) {
		g := newGuardTest([]string{"a.go"})
		g.work = true
		if g.guard.beforeQuit(true) || !g.guard.passing || g.windows[0].app.Closing() {
			t.Fatal("a session end was held back")
		}
	})
}
