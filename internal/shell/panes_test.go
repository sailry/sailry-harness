package shell

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/panel"
	"github.com/sailry/sailry-go/internal/panes"
	"github.com/sailry/sailry-go/internal/preview"
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
