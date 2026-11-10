package shell

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

const themeChat = "sailry-go/theme" // "Port the neutral theme tokens"

// splitThree opens the big conversation with the theme chat on its right
// and a preview terminal below that.
func splitThree(t *testing.T, a *App, tt *ui.Tester) panes.Target {
	t.Helper()
	a.Panel.New(panel.Terminal)
	term := panes.Target{Kind: panes.Terminal, ID: a.Panel.Active.ID}
	if !a.panes.Drop(session(preview.Big), session(themeChat), panes.Right) ||
		!a.panes.Drop(session(themeChat), term, panes.Bottom) {
		t.Fatal("drop")
	}
	a.focusPane(session(themeChat))
	tt.Frame()
	tt.Frame()
	return term
}

// buttons counts the buttons named name.
func buttons(tt *ui.Tester, name string) int {
	n := 0
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleButton && e.Name == name {
			n++
		}
	}
	return n
}

func TestPanesRender(t *testing.T) {
	t.Run("live terminal pane", testLiveTerminalPane)
	a, tt := newTester(t)
	splitThree(t, a, tt)
	snapshot(t, tt, "panes-split")
	// Each pane has its header: the title, the drag handle and close.
	for _, title := range []string{"Virtualized transcript stress test", "Port the neutral theme tokens", "Terminal"} {
		if !tt.HasText(title) {
			t.Errorf("no pane titled %q: %q", title, tt.Texts())
		}
	}
	if n := buttons(tt, "Drag to split"); n != 3 {
		t.Errorf("%d drag handles", n)
	}
	// The band names the page; the terminal is a marked preview.
	if !tt.HasText("Chats") || !tt.HasText("Terminal UI preview with project and worktree ownership; no PTY or shell is started") {
		t.Errorf("header or terminal preview: %q", tt.Texts())
	}
	// The focused pane is the sidebar's selection.
	if a.Sidebar.Selected != themeChat {
		t.Errorf("selected %q", a.Sidebar.Selected)
	}
	// Another session chosen in the sidebar stands alone, the split
	// coming back with one of its panes.
	click(t, tt, "Weeknight dinner ideas")
	if a.panes.Displayed() != nil || tt.HasText("Drag to split") {
		t.Fatal("the split still shows")
	}
	click(t, tt, "Port the neutral theme tokens")
	if a.panes.Displayed() == nil {
		t.Fatal("the split does not come back")
	}
}

func TestPaneFocusFollowsClick(t *testing.T) {
	a, tt := newTester(t)
	splitThree(t, a, tt)
	// The first pane's drag handle, at its header's right.
	r, ok := tt.AccessByRole(ui.RoleButton, "Drag to split")
	if !ok {
		t.Fatal("no header")
	}
	// A click in the pane's body, under its header.
	tt.ClickAt(r.Bounds.X-200, r.Bounds.Y+200)
	tt.Frame()
	if a.panes.Active != session(preview.Big) || a.Sidebar.Selected != preview.Big {
		t.Fatalf("active %v, selected %q", a.panes.Active, a.Sidebar.Selected)
	}
}

func TestPaneResize(t *testing.T) {
	a, tt := newTester(t)
	splitThree(t, a, tt)
	g := a.panes.Displayed()
	before := g.Sizes[0]
	var handle ui.AccessInfo
	for _, e := range tt.AccessAll() {
		// The row's handle is the tallest splitter.
		if e.Role == ui.RoleSplitter && e.Bounds.H > handle.Bounds.H {
			handle = e
		}
	}
	if handle.Bounds.H == 0 {
		t.Fatal("no split handle")
	}
	x, y := handle.Bounds.X+handle.Bounds.W/2, handle.Bounds.Y+handle.Bounds.H/2
	tt.Move(x, y)
	tt.Frame()
	snapshot(t, tt, "panes-handle-hover")
	tt.Press(x, y)
	for i := 1; i <= 10; i++ {
		tt.Move(x+float32(i)*10, y)
		tt.Frame()
	}
	tt.Release(x+100, y)
	tt.Frame()
	if g.Sizes[0] <= before || g.Sizes[0]+g.Sizes[1] < 0.999 || g.Sizes[0]+g.Sizes[1] > 1.001 {
		t.Fatalf("sizes %v from %v", g.Sizes, before)
	}
}

func TestPaneClose(t *testing.T) {
	a, tt := newTester(t)
	term := splitThree(t, a, tt)
	// A terminal asks first; cancelling keeps it.
	closeButtons := func() []ui.AccessInfo {
		var out []ui.AccessInfo
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && e.Name == "Close" {
				out = append(out, e)
			}
		}
		return out
	}
	// The panes' close buttons in visual order: big, theme, terminal.
	list := closeButtons()
	if len(list) < 3 {
		t.Fatalf("%d close buttons", len(list))
	}
	last := list[len(list)-1].Bounds
	tt.ClickAt(last.X+last.W/2, last.Y+last.H/2)
	tt.Advance(400 * time.Millisecond)
	tt.Frame()
	snapshot(t, tt, "panes-close-terminal")
	if !tt.HasText("Close terminal") {
		t.Fatalf("no confirmation: %q", tt.Texts())
	}
	click(t, tt, "Cancel")
	if a.panes.Group(term) < 0 {
		t.Fatal("cancelled, yet closed")
	}
	tt.ClickAt(last.X+last.W/2, last.Y+last.H/2)
	tt.Frame()
	// The dialog's Close is the one button named so that was not there.
	for _, e := range closeButtons() {
		if !slices.ContainsFunc(list, func(o ui.AccessInfo) bool { return o.Bounds == e.Bounds }) {
			tt.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
		}
	}
	tt.Frame()
	if a.panes.Group(term) >= 0 {
		t.Fatal("the terminal pane stays")
	}
	// The terminal keeps its tab: the preview closes nothing.
	if !slices.Contains(a.Panel.Tabs, panel.Tab{Kind: panel.Terminal, ID: term.ID}) {
		t.Fatal("the terminal tab is gone")
	}
	// A session leaves the split at once; one pane left stands alone.
	list = closeButtons()
	first := list[0].Bounds
	tt.ClickAt(first.X+first.W/2, first.Y+first.H/2)
	tt.Frame()
	if len(a.panes.Groups) != 0 || tt.HasText("Drag to split") {
		t.Fatalf("groups %d", len(a.panes.Groups))
	}
	if a.Sidebar.Selected != themeChat {
		t.Fatalf("selected %q", a.Sidebar.Selected)
	}
}

// dragTo drags from the centre of the element named from to (x, y) in
// steps, a frame each, calling during before letting go.
func dragTo(t *testing.T, tt *ui.Tester, from ui.AccessInfo, x, y float32, during func()) {
	t.Helper()
	sx, sy := from.Bounds.X+from.Bounds.W/2, from.Bounds.Y+from.Bounds.H/2
	tt.Press(sx, sy)
	for i := 1; i <= 12; i++ {
		f := float32(i) / 12
		tt.Move(sx+(x-sx)*f, sy+(y-sy)*f)
		tt.Frame()
	}
	tt.Advance(300 * time.Millisecond)
	tt.Frame()
	if during != nil {
		during()
	}
	tt.Release(x, y)
	tt.Frame()
	tt.Frame()
}

// TestPaneDrags groups the cases of one behavior; each keeps its own fixture.
func TestPaneDrags(t *testing.T) {
	t.Run("DragSessionSplits", testDragSessionSplits)
	t.Run("DragTerminalTab", testDragTerminalTab)
	t.Run("DragPaneIntoEarlierPane", testDragPaneIntoEarlierPane)
}

func testDragSessionSplits(t *testing.T) {
	a, tt := newTester(t)
	row, ok := tt.AccessByRole(ui.RoleButton, "Port the neutral theme tokens")
	if !ok {
		t.Fatal("no row")
	}
	// Onto the right of the conversation shown alone.
	dragTo(t, tt, row, 1150, 400, func() {
		snapshot(t, tt, "panes-drag-right")
	})
	g := a.panes.Displayed()
	if g == nil || g.Axis != panes.Row || !slices.Equal(g.Targets(), []panes.Target{session(preview.Big), session(themeChat)}) {
		t.Fatalf("split: %+v", a.panes.Groups)
	}
	if a.Sidebar.Selected != themeChat {
		t.Fatalf("focus %q", a.Sidebar.Selected)
	}
	// Then another below the first pane, by its left half's lower part.
	row, _ = tt.AccessByRole(ui.RoleButton, "Weeknight dinner ideas")
	dragTo(t, tt, row, 520, 700, nil)
	if got := len(a.panes.Displayed().Targets()); got != 3 || a.panes.Displayed().Children[0].Axis != panes.Column {
		t.Fatalf("%d panes: %+v", got, a.panes.Displayed().Children[0])
	}
	// A pane's header handle moves it: the dinner pane to the centre of
	// the theme pane trades their places.
	var handles []ui.AccessInfo
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleButton && e.Name == "Drag to split" {
			handles = append(handles, e)
		}
	}
	if len(handles) != 3 {
		t.Fatalf("%d handles", len(handles))
	}
	dinner := a.panes.Displayed().Targets()[1]
	var from ui.AccessInfo
	for _, h := range handles {
		if h.Bounds.Y > 400 {
			from = h
		}
	}
	dragTo(t, tt, from, 1000, 450, nil)
	if got := a.panes.Displayed().Targets(); got[2] != dinner || got[1] != session(themeChat) {
		t.Fatalf("swap: %v", got)
	}
}

func testDragTerminalTab(t *testing.T) {
	a, tt := newTester(t)
	a.PanelOpen = true
	a.Panel.New(panel.Terminal)
	tt.Frame()
	tt.Advance(400 * time.Millisecond)
	tt.Frame()
	tab, ok := tt.AccessByRole(ui.RoleButton, "Terminal")
	if !ok {
		t.Fatalf("no terminal tab: %q", tt.Texts())
	}
	// Onto the bottom of the conversation.
	dragTo(t, tt, tab, 500, 700, func() {
		snapshot(t, tt, "panes-drag-terminal")
	})
	g := a.panes.Displayed()
	if g == nil || g.Axis != panes.Column || g.Targets()[1].Kind != panes.Terminal {
		t.Fatalf("split: %+v", a.panes.Groups)
	}
	// The selection stays a session: the band shows the split.
	if a.Sidebar.Selected != preview.Big || !tt.HasText("Terminal UI preview with project and worktree ownership; no PTY or shell is started") {
		t.Fatalf("selected %q", a.Sidebar.Selected)
	}
}

func TestPanesPersist(t *testing.T) {
	a, tt := newTester(t)
	term := splitThree(t, a, tt)
	a.panes.Displayed().Sizes[0], a.panes.Displayed().Sizes[1] = 0.3, 0.7
	dir := t.TempDir()
	if err := SavePrefs(dir, a.Prefs()); err != nil {
		t.Fatal(err)
	}
	b := New(preview.Load())
	b.SetPrefs(LoadPrefs(dir))
	tb := ui.NewTesterWith(b.View, 1280, 820, opaque)
	tb.SetLocale("en")
	tb.Frame()
	// The selected session's group comes back with its shares.
	g := b.panes.Displayed()
	if g == nil || !slices.Equal(g.Targets(), []panes.Target{session(preview.Big), session(themeChat), term}) || g.Sizes[0] != 0.3 {
		t.Fatalf("restored: %+v", b.panes.Groups)
	}
	// A terminal whose tab is gone leaves its group.
	p := LoadPrefs(dir)
	p.PanelTabs, p.PanelTab = nil, panel.Tab{}
	c := New(preview.Load())
	c.SetPrefs(p)
	if got := c.panes.Members(session(themeChat)); len(got) != 2 {
		t.Fatalf("pruned: %v", got)
	}
}

func TestTerminalSplitKeys(t *testing.T) {
	a, tt := newTester(t)
	term := splitThree(t, a, tt)
	// A click in the terminal pane's body gives it the focus.
	r, ok := tt.Find("Terminal UI preview with project and worktree ownership; no PTY or shell is started")
	if !ok {
		t.Fatal("no terminal pane")
	}
	tt.ClickAt(r.X+r.W/2, r.Y-60)
	tt.Frame()
	if a.panes.Active != term {
		t.Fatalf("active %v", a.panes.Active)
	}
	tt.Key(ui.Cmd, ui.KeyD)
	tt.Frame()
	if n := len(a.panes.Displayed().Targets()); n != 4 || a.panes.Active.Kind != panes.Terminal || a.panes.Active == term {
		t.Fatalf("⌘D: %d panes, active %v", n, a.panes.Active)
	}
	right := a.panes.Active
	tt.Frame()
	tt.Key(ui.Cmd|ui.Shift, ui.KeyD)
	tt.Frame()
	if n := len(a.panes.Displayed().Targets()); n != 5 {
		t.Fatalf("⌘⇧D: %d panes", n)
	}
	if got := a.panes.Members(right); got[len(got)-1] != a.panes.Active {
		t.Fatalf("below: %v", got)
	}
	snapshot(t, tt, "panes-terminal-keys")
}

// splitTwo shows the big conversation and the theme chat side by side,
// returning the bounds of the first pane under its header: as wide as
// the second, which runs from the split handle to the window's edge.
func splitTwo(t *testing.T, a *App, tt *ui.Tester) ui.Rect {
	t.Helper()
	if !a.panes.Drop(session(preview.Big), session(themeChat), panes.Right) {
		t.Fatal("drop")
	}
	tt.Frame()
	tt.Frame()
	var handle, split ui.AccessInfo
	for _, e := range tt.AccessAll() {
		switch {
		case e.Role == ui.RoleButton && e.Name == "Drag to split":
			handle = e
		case e.Role == ui.RoleSplitter && e.Bounds.H > split.Bounds.H:
			split = e
		}
	}
	if handle.Name == "" || split.Bounds.H == 0 {
		t.Fatal("no split")
	}
	top := handle.Bounds.Y + handle.Bounds.H
	w := 1280 - (split.Bounds.X + split.Bounds.W)
	return ui.Rect{X: split.Bounds.X - w, Y: top, W: w, H: split.Bounds.Y + split.Bounds.H - top}
}

// Splitting A | B and dragging B's header into A, at its centre or any
// edge: the drop applies while the panes build, and the tree the build
// walks stays as it was, so B is neither built twice nor indexed past.
func testDragPaneIntoEarlierPane(t *testing.T) {
	big, theme := session(preview.Big), session(themeChat)
	for _, c := range []struct {
		name   string
		fx, fy float32
		want   func(*panes.Node) bool
	}{
		{"centre", 0.5, 0.5, func(g *panes.Node) bool {
			return g.Axis == panes.Row && slices.Equal(g.Targets(), []panes.Target{theme, big})
		}},
		{"left", 0.1, 0.5, func(g *panes.Node) bool {
			return g.Axis == panes.Row && slices.Equal(g.Targets(), []panes.Target{theme, big})
		}},
		{"right", 0.9, 0.5, func(g *panes.Node) bool {
			return g.Axis == panes.Row && slices.Equal(g.Targets(), []panes.Target{big, theme})
		}},
		{"top", 0.5, 0.1, func(g *panes.Node) bool {
			return g.Axis == panes.Column && slices.Equal(g.Targets(), []panes.Target{theme, big})
		}},
		{"bottom", 0.5, 0.9, func(g *panes.Node) bool {
			return g.Axis == panes.Column && slices.Equal(g.Targets(), []panes.Target{big, theme})
		}},
	} {
		t.Run(c.name, func(t *testing.T) {
			a, tt := newTester(t)
			r := splitTwo(t, a, tt)
			var from ui.AccessInfo
			for _, e := range tt.AccessAll() {
				if e.Role == ui.RoleButton && e.Name == "Drag to split" && e.Bounds.X > r.X+r.W {
					from = e
				}
			}
			dragTo(t, tt, from, r.X+r.W*c.fx, r.Y+r.H*c.fy, nil)
			if g := a.panes.Displayed(); g == nil || !c.want(g) {
				t.Fatalf("groups %+v", a.panes.Groups[0].Targets())
			}
			tt.Frame()
		})
	}
}

// ⌘W in the first of two panes closes it while the panes build; the tree the
// build walks keeps both children, so it does not index past one, and the
// focus goes to the other pane.
func TestCloseEarlierPaneByKey(t *testing.T) {
	a, tt := newTester(t)
	r := splitTwo(t, a, tt)
	tt.ClickAt(r.X+r.W/2, r.Y+200)
	tt.Frame()
	if a.panes.Active != session(preview.Big) {
		t.Fatalf("active %v", a.panes.Active)
	}
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	if len(a.panes.Groups) != 0 || a.panes.Active != session(themeChat) || a.Sidebar.Selected != themeChat {
		t.Fatalf("groups %v, active %v, selected %q", a.panes.Groups, a.panes.Active, a.Sidebar.Selected)
	}
}
