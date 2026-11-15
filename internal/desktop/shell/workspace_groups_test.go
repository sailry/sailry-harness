package shell

import (
	"slices"
	"strconv"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func workspaceGroupsFixture(t *testing.T, width int) (*App, *ui.Tester) {
	t.Helper()
	data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}}
	for _, id := range []string{"a", "b", "c", "d", "e"} {
		data.Unassigned = append(data.Unassigned, &viewmodel.Conversation{ID: id, Title: "Conversation " + id})
	}
	a := NewLive(data)
	a.Sidebar.Selected = "a"
	a.focusPane(session("a"))
	if !a.panes.Drop(session("a"), session("b"), panes.Right) ||
		!a.panes.Drop(session("c"), session("d"), panes.Bottom) {
		t.Fatal("cannot create two split groups")
	}
	a.focusPane(session("b"))
	a.focusPane(session("d"))
	tt := ui.NewTesterWith(a.View, width, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	return a, tt
}

func pressWorkspaceGroup(t *testing.T, tt *ui.Tester, number int) {
	t.Helper()
	label := i18n.In("en", "pane_group", "number", number)
	b, ok := headerButton(tt, label)
	if !ok {
		t.Fatalf("no header button %q", label)
	}
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	tt.Frame()
}

// Group tabs, named by their panes' titles, switch groups without rebuilding their trees or losing the
// last focused pane. They also remain reachable from a standalone conversation.
func TestWorkspaceGroupButtons(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 1280)
	first, second := a.panes.Groups[0], a.panes.Groups[1]
	for _, number := range []int{1, 2} {
		b, ok := headerButton(tt, i18n.In("en", "pane_group", "number", number))
		if !ok || b.Checked != (number == 2) || b.Bounds.W <= workspaceGroupSize || b.Bounds.W > groupTabMax || b.Bounds.H != workspaceGroupSize {
			t.Fatalf("group %d button: %+v, exists %v", number, b, ok)
		}
	}
	pressWorkspaceGroup(t, tt, 1)
	if a.panes.Active != session("b") || a.Sidebar.Selected != "b" || a.panes.Displayed() != first {
		t.Fatalf("first group: active %v, selected %q", a.panes.Active, a.Sidebar.Selected)
	}
	pressWorkspaceGroup(t, tt, 2)
	if a.panes.Active != session("d") || a.Sidebar.Selected != "d" || a.panes.Displayed() != second {
		t.Fatalf("second group: active %v, selected %q", a.panes.Active, a.Sidebar.Selected)
	}
	// The number follows the current order; remembered focus follows the group.
	slices.Reverse(a.panes.Groups)
	tt.Frame()
	pressWorkspaceGroup(t, tt, 2)
	if a.panes.Active != session("b") || a.panes.Displayed() != first {
		t.Fatalf("reordered group: active %v", a.panes.Active)
	}
	a.Sidebar.Selected = "e"
	tt.Frame()
	for _, number := range []int{1, 2} {
		b, ok := headerButton(tt, i18n.In("en", "pane_group", "number", number))
		if !ok || b.Checked {
			t.Fatalf("standalone group %d button: %+v", number, b)
		}
	}
	pressWorkspaceGroup(t, tt, 1)
	if a.panes.Active != session("d") || a.panes.Displayed() != second {
		t.Fatalf("returning from standalone: active %v", a.panes.Active)
	}
	// Switching only changes focus: both original trees remain intact.
	if a.panes.Groups[0] != second || a.panes.Groups[1] != first {
		t.Fatal("switching groups replaced a split tree")
	}
	snapshot(t, tt, "workspace-group-buttons")
}

func TestWorkspaceGroupTerminalFocus(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 1280)
	a.panes.Detach(session("c"))
	a.OpenWorkspaceTerminal("term-a", "Terminal A")
	a.OpenWorkspaceTerminal("term-b", "Terminal B")
	first := panes.Target{Kind: panes.Terminal, ID: "term-a"}
	second := panes.Target{Kind: panes.Terminal, ID: "term-b"}
	a.Panel.TerminalActions = &panel.TerminalActions{Pane: func(c *ui.Context, id string) { ui.Text(c, "Terminal screen "+id) }}
	if !a.panes.Drop(first, second, panes.Right) {
		t.Fatal("cannot create terminal group")
	}
	a.focusPane(second)
	tt.Frame()
	pressWorkspaceGroup(t, tt, 1)
	selected := a.Sidebar.Selected
	pressWorkspaceGroup(t, tt, 2)
	if a.panes.Active != second || a.Sidebar.Selected != selected || !tt.HasText("Terminal screen term-b") {
		t.Fatalf("terminal group: active %v, selected %q (was %q)", a.panes.Active, a.Sidebar.Selected, selected)
	}
	if len(a.Panel.Tabs) != 0 {
		t.Fatalf("terminal group created right panel tabs: %v", a.Panel.Tabs)
	}
}

func TestWorkspaceGroupReturnAndRemovedFocus(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 1280)
	a.panes.Detach(session("d"))
	a.focusPane(session("e"))
	tt.Frame()
	// One remaining split still has an entry while a standalone view shows.
	pressWorkspaceGroup(t, tt, 1)
	if a.panes.Active != session("b") {
		t.Fatalf("one saved group returned to %v", a.panes.Active)
	}
	// A shown split keeps its tab, which tells it from a conversation
	// shown alone.
	if b, ok := headerButton(tt, i18n.In("en", "pane_group", "number", 1)); !ok || !b.Checked {
		t.Fatalf("one shown split lost its tab: %+v", b)
	}
	// A focus remembered before membership changed cannot focus a removed pane.
	a.panes.Drop(session("a"), session("c"), panes.Bottom)
	a.focusPane(session("b"))
	a.panes.Detach(session("b"))
	a.focusPane(session("e"))
	tt.Frame()
	pressWorkspaceGroup(t, tt, 1)
	if a.panes.Active != session("a") || a.panes.Group(a.panes.Active) < 0 {
		t.Fatalf("removed remembered pane kept focus: %v", a.panes.Active)
	}
}

func TestWorkspaceGroupsScrollInNarrowHeader(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 640)
	for i := 3; i <= 8; i++ {
		id := "group-" + strconv.Itoa(i)
		for _, suffix := range []string{"-a", "-b"} {
			a.Data.Unassigned = append(a.Data.Unassigned, &viewmodel.Conversation{ID: id + suffix, Title: id + suffix})
		}
		if !a.panes.Drop(session(id+"-a"), session(id+"-b"), panes.Right) {
			t.Fatal("cannot create additional group")
		}
	}
	a.NavOpen = false
	a.focusPane(session("b"))
	tt.Frame()
	settle(tt)
	tt.Frame()
	first, ok := headerButton(tt, i18n.In("en", "pane_group", "number", 1))
	if !ok {
		t.Fatal("no first group button in narrow header")
	}
	tt.Scroll(first.Bounds.X+first.Bounds.W/2, first.Bounds.Y+first.Bounds.H/2, 2000, 0)
	settle(tt)
	tt.Frame()
	last, ok := headerButton(tt, i18n.In("en", "pane_group", "number", 8))
	if !ok || last.Bounds.X < 0 || last.Bounds.X+last.Bounds.W > 640 {
		t.Fatalf("scrolling did not reveal the last group: %+v", last)
	}
	if a.panes.Active != session("b") {
		t.Fatal("scrolling the numbers changed the active group")
	}
	pressWorkspaceGroup(t, tt, 8)
	if a.panes.Active != session("group-8-b") {
		t.Fatalf("last group did not open: %v", a.panes.Active)
	}
	snapshot(t, tt, "workspace-group-buttons-narrow")
}
