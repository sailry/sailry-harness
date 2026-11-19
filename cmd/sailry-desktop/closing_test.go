package main

import (
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

// guardTest is an exitGuard over windows with the given unsaved documents.
type guardTest struct {
	guard   *exitGuard
	windows []*desktopWindow
	unsaved map[*desktopWindow][]shell.CloseDocument
	work    bool
	raised  []*desktopWindow
	shut    []*desktopWindow
	quits   int
}

func newGuardTest(documents ...[]string) *guardTest {
	g := &guardTest{unsaved: map[*desktopWindow][]shell.CloseDocument{}}
	for _, names := range documents {
		g.openWindow(names...)
	}
	g.guard = &exitGuard{
		windows:   func() []*desktopWindow { return g.windows },
		active:    func() *desktopWindow { return g.windows[len(g.windows)-1] },
		documents: func(window *desktopWindow) []shell.CloseDocument { return g.unsaved[window] },
		save:      func(*desktopWindow) {},
		saving:    func(*desktopWindow) bool { return false },
		work:      func(*desktopWindow) bool { return g.work },
		raise:     func(window *desktopWindow) { g.raised = append(g.raised, window) },
		shut:      func(window *desktopWindow) { g.shut = append(g.shut, window) },
		quit:      func() { g.quits++ },
	}
	return g
}

func (g *guardTest) openWindow(names ...string) *desktopWindow {
	window := &desktopWindow{app: shell.New(preview.Load())}
	g.windows = append(g.windows, window)
	project := &panel.Project{}
	for _, name := range names {
		g.unsaved[window] = append(g.unsaved[window], shell.CloseDocument{Project: project, Name: name})
	}
	return window
}

func TestQuitRechecksEveryOpenWindow(t *testing.T) {
	g := newGuardTest(nil, nil)
	entry := g.windows[1]
	var prompts []kit.Confirmation
	var answer func(int)
	tester := ui.NewTesterWith(func(c *ui.Context) {
		kit.BindConfirmations(c, func(request kit.Confirmation, done func(int)) {
			prompts, answer = append(prompts, request), done
		})
		entry.app.View(c)
	}, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	if !g.guard.beforeQuit(false) {
		t.Fatal("a clean quit did not ask")
	}
	tester.Frame()
	if len(prompts) != 1 || prompts[0].Title != i18n.In("en", "app_exit_confirm") {
		t.Fatalf("initial prompts: %+v", prompts)
	}
	const name = "project/notes.md"
	g.unsaved[g.windows[0]] = []shell.CloseDocument{{Project: &panel.Project{}, Name: name}}
	confirm := func(choice int) {
		answer(choice)
		tester.Frame()
		tester.Frame()
	}
	checkDraftPrompt := func(count int) {
		t.Helper()
		if len(prompts) != count || prompts[count-1].Title != i18n.In("en", "files_unsaved") || !strings.Contains(prompts[count-1].Message, name) || g.quits != 0 || g.guard.passing {
			t.Fatalf("quit did not wait for the current drafts: quits=%d prompts=%+v", g.quits, prompts)
		}
	}
	confirm(1)
	checkDraftPrompt(2)
	// The original draft was saved. Another window now owns a same-named draft.
	delete(g.unsaved, g.windows[0])
	added := g.openWindow(name)
	confirm(1)
	checkDraftPrompt(3)
	var saved []*desktopWindow
	g.guard.save = func(window *desktopWindow) {
		saved = append(saved, window)
		delete(g.unsaved, window)
	}
	confirm(2)
	if !slices.Contains(saved, added) || g.quits != 1 || !g.guard.passing || len(prompts) != 3 {
		t.Fatalf("Save all missed the new window or asked again: saved=%v quits=%d prompts=%d", saved, g.quits, len(prompts))
	}
}

func TestExitGuard(t *testing.T) {
	t.Run("a clean non-last window closes without a prompt", func(t *testing.T) {
		g := newGuardTest(nil, nil)
		if g.guard.closing(g.windows[0]) || g.windows[0].app.Closing() {
			t.Fatal("a clean window asked before closing")
		}
	})
	t.Run("a clean quit asks in the active window", func(t *testing.T) {
		g := newGuardTest(nil, nil)
		if !g.guard.beforeQuit(false) || g.guard.passing || !g.windows[1].app.Closing() || g.quits != 0 {
			t.Fatal("a clean quit did not wait for confirmation")
		}
		g.guard.active = func() *desktopWindow { return g.windows[0] }
		if !g.guard.beforeQuit(false) || len(g.raised) != 1 || g.windows[0].app.Closing() {
			t.Fatal("a change of active window opened a second quit prompt")
		}
	})
	t.Run("a confirmed quit waits for every settings save", func(t *testing.T) {
		g := newGuardTest(nil, nil)
		g.guard.passing = true
		waiting := map[*desktopWindow]func(bool){}
		g.guard.flush = func(window *desktopWindow, done func(bool)) bool {
			waiting[window] = done
			return true
		}
		if !g.guard.beforeQuit(false) || len(waiting) != 2 || !g.guard.closing(g.windows[0]) {
			t.Fatal("quit allowed an open window to close during its save")
		}
		waiting[g.windows[0]](true)
		if g.quits != 0 {
			t.Fatal("quit resumed with another save pending")
		}
		waiting[g.windows[1]](true)
		g.guard.flush = nil
		if g.quits != 1 || g.guard.beforeQuit(false) || g.guard.closing(g.windows[1]) {
			t.Fatal("quit did not proceed after both saves settled")
		}
	})
	t.Run("a settings failure leaves the closing window open", func(t *testing.T) {
		g := newGuardTest(nil, nil)
		var settle func(bool)
		g.guard.flush = func(_ *desktopWindow, done func(bool)) bool {
			settle = done
			return true
		}
		window := g.windows[0]
		if !g.guard.closing(window) || settle == nil || !window.closeSaving {
			t.Fatal("a window closed with a pending save")
		}
		settle(false)
		if len(g.shut) != 0 || window.closeSaving {
			t.Fatal("a failed save closed or kept waiting on the window")
		}
		if !g.guard.closing(window) {
			t.Fatal("the window did not wait for a new save")
		}
		settle(true)
		if !slices.Equal(g.shut, []*desktopWindow{window}) {
			t.Fatal("the saved window did not close")
		}
	})
	t.Run("a session end closes a window during its save", func(t *testing.T) {
		g := newGuardTest(nil, nil)
		var settle func(bool)
		g.guard.flush = func(_ *desktopWindow, done func(bool)) bool {
			if done != nil {
				settle = done
			}
			return true
		}
		window := g.windows[0]
		if !g.guard.closing(window) {
			t.Fatal("a window closed with a pending save")
		}
		if g.guard.beforeQuit(true) || g.guard.closing(window) {
			t.Fatal("the window's save held back the session end")
		}
		settle(true)
		if len(g.shut) != 0 || !g.guard.passing {
			t.Fatal("the late save outcome changed the ending session")
		}
	})
	t.Run("a repeated close stops waiting for a save", func(t *testing.T) {
		g := newGuardTest(nil, nil)
		var settle func(bool)
		g.guard.flush = func(_ *desktopWindow, done func(bool)) bool {
			settle = done
			return true
		}
		window := g.windows[0]
		if !g.guard.closing(window) || g.guard.closing(window) || window.closeSaving {
			t.Fatal("a repeated close kept waiting for the save")
		}
		settle(true)
		if len(g.shut) != 0 {
			t.Fatal("a superseded save closed the window again")
		}
	})
	t.Run("a repeated quit stops waiting for a save", func(t *testing.T) {
		g := newGuardTest(nil)
		g.guard.passing = true
		var settle func(bool)
		g.guard.flush = func(_ *desktopWindow, done func(bool)) bool {
			settle = done
			return true
		}
		if !g.guard.beforeQuit(false) || g.guard.beforeQuit(false) || g.guard.quitSaving {
			t.Fatal("a repeated confirmed quit kept waiting for the save")
		}
		settle(false)
		if g.quits != 0 || !g.guard.passing {
			t.Fatal("a superseded save changed the quit")
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
		if !g.guard.closing(g.windows[0]) || !g.windows[0].app.Closing() {
			t.Fatal("closing the last window did not wait for confirmation")
		}
		if !g.guard.beforeQuit(false) || len(g.raised) != 1 {
			t.Fatal("quitting while waiting for the last window did not reuse its prompt")
		}
	})
	t.Run("a profile restart only asks when there is something to protect", func(t *testing.T) {
		g := newGuardTest(nil)
		entry := g.windows[0]
		proceed := func() { t.Fatal("asked before deciding whether a prompt is needed") }
		if g.guard.ask(entry, restartProfile, proceed) || entry.app.Closing() {
			t.Fatal("an idle profile restart added a quit prompt")
		}
		g.work = true
		if !g.guard.ask(entry, restartProfile, proceed) || !entry.app.Closing() {
			t.Fatal("a profile restart over local work did not ask")
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
	t.Run("a startup failure can quit without a window", func(t *testing.T) {
		g := newGuardTest()
		g.guard.active = func() *desktopWindow { return nil }
		if g.guard.beforeQuit(false) || !g.guard.passing {
			t.Fatal("quit without windows was blocked")
		}
	})
}
