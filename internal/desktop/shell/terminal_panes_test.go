package shell

import (
	"fmt"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
)

// testLiveTerminalPane covers PANE-011, PANE-013 and PANE-016 with stand-in
// terminal actions: a terminal pane builds its tab's terminal through Pane,
// ⌘D starts a terminal beside it through Split, and a pane whose tab is gone
// leaves the split.
func testLiveTerminalPane(t *testing.T) {
	a, tt := newTester(t)
	started := 0
	a.Panel.TerminalActions = &panel.TerminalActions{
		View: func(c *ui.Context, id string) { ui.Text(c, "tab screen "+id) },
		Pane: func(c *ui.Context, id string) { ui.Text(c, "pane screen "+id) },
		Split: func(id string) (string, bool) {
			started++
			tab := panel.Tab{Kind: panel.Terminal, ID: fmt.Sprintf("%s-%d", id, started)}
			a.Panel.Show(tab)
			return tab.ID, true
		},
	}
	term := splitThree(t, a, tt)
	if !tt.HasText("pane screen "+term.ID) || tt.HasText("Terminal UI preview with project and worktree ownership; no PTY or shell is started") {
		t.Fatalf("the pane does not build its terminal: %q", tt.Texts())
	}
	r, ok := tt.Find("pane screen " + term.ID)
	if !ok {
		t.Fatal("no terminal pane")
	}
	tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
	tt.Frame()
	if a.panes.Active != term {
		t.Fatalf("active %v", a.panes.Active)
	}
	tt.Key(ui.Cmd, ui.KeyD)
	tt.Frame()
	beside := panes.Target{Kind: panes.Terminal, ID: term.ID + "-1"}
	if n := len(a.panes.Displayed().Targets()); n != 4 || started != 1 || a.panes.Active != beside {
		t.Fatalf("⌘D: %d panes, %d started, active %v", n, started, a.panes.Active)
	}
	tt.Frame()
	if !tt.HasText("pane screen " + beside.ID) {
		t.Fatalf("the new pane does not build its terminal: %q", tt.Texts())
	}
	// The terminal's tab closes, as when its Node closed it: the pane leaves.
	tab := panel.Tab{Kind: panel.Terminal, ID: term.ID}
	a.Panel.Tabs, a.Panel.Active = panel.Closed(a.Panel.Tabs, tab, a.Panel.Active)
	tt.Frame()
	if g := a.panes.Displayed(); g == nil || slices.Contains(g.Targets(), term) || len(g.Targets()) != 3 {
		t.Fatalf("the closed terminal keeps its pane: %v", a.panes.Groups)
	}
	// A pane leaving the split keeps its terminal's tab.
	if !a.panes.Detach(beside) {
		t.Fatal("detach")
	}
	tt.Frame()
	if !slices.Contains(a.Panel.Tabs, panel.Tab{Kind: panel.Terminal, ID: beside.ID}) {
		t.Fatal("closing a pane closed its terminal's tab")
	}
}
