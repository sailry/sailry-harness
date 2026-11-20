package shell

import (
	"slices"
	"strconv"
	"testing"
	"time"

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

func TestSingleWorkspaceTitleAndTabTransitions(t *testing.T) {
	first := &viewmodel.Conversation{ID: "a", Title: "工具测试与问候"}
	second := &viewmodel.Conversation{ID: "b", Title: "Another conversation"}
	a := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{first, second}})
	a.focusPane(session(first.ID))
	a.Composer(first.ID).Draft = "Preserved draft"
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("zh-CN")
	tt.SetDark(true)
	frame := func() { tt.Frame(); tt.Frame() }
	plainTitle := func(title string) {
		t.Helper()
		if _, ok := headerButton(tt, title); ok {
			t.Fatalf("a single entry still has a tab: %q", title)
		}
		if bounds, ok := tt.Find(title); !ok || bounds.Y+bounds.H > HeaderHeight {
			t.Fatalf("single entry has no header title: %q at %+v", title, bounds)
		}
	}
	keptTab := func(title string) ui.AccessInfo {
		t.Helper()
		tab, ok := headerButton(tt, title)
		if !ok || !tab.Checked {
			t.Fatalf("the kept tab %q is not visible and selected: %+v", title, tab)
		}
		return tab
	}
	frame()
	plainTitle(first.Title)
	snapshot(t, tt, "workspace-single-title")
	a.panes.Keep(session(first.ID))
	frame()
	keptTab(first.Title)
	a.Sidebar.Selected = second.ID
	frame()
	for _, title := range []string{first.Title, second.Title} {
		if _, ok := headerButton(tt, title); !ok {
			t.Fatalf("multiple entries lost the tab %q", title)
		}
	}
	snapshot(t, tt, "workspace-multiple-tabs")
	button, _ := headerButton(tt, second.Title)
	tt.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyW)
	frame()
	kept := keptTab(first.Title)
	if a.Sidebar.Selected != first.ID || a.Composer(first.ID).Draft != "Preserved draft" || !slices.Equal(a.panes.Kept, []panes.Target{session(first.ID)}) {
		t.Fatal("returning to the kept tab changed the selection, draft or kept tab")
	}
	// A lone kept tab stays visible so it can be closed. It then reverts to
	// a plain title without leaving the conversation or clearing the draft.
	tt.RightClickAt(kept.Bounds.X+kept.Bounds.W/2, kept.Bounds.Y+kept.Bounds.H/2)
	if err := tt.ChooseMenuItem(i18n.In("zh-CN", "close")); err != nil {
		t.Fatal(err)
	}
	frame()
	plainTitle(first.Title)
	if len(a.panes.Kept) != 0 || a.Sidebar.Selected != first.ID || a.Composer(first.ID).Draft != "Preserved draft" {
		t.Fatal("closing the last kept tab changed the open conversation or draft")
	}
	// A project conversation uses its existing breadcrumb title.
	a.Data.Unassigned = []*viewmodel.Conversation{second}
	a.Data.Projects = []*viewmodel.Project{{ID: "project", Name: "Project", Conversations: []*viewmodel.Conversation{first}}}
	frame()
	for _, title := range []string{"Project", first.Title} {
		if bounds, ok := tt.Find(title); !ok || bounds.Y+bounds.H > HeaderHeight {
			t.Fatalf("single project conversation lost its breadcrumb: %q", title)
		}
	}
	snapshot(t, tt, "workspace-single-project-title")
	a.OpenWorkspaceTerminal("terminal", "Standalone terminal")
	frame()
	plainTitle("Standalone terminal")
	snapshot(t, tt, "workspace-single-terminal-title")
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
	// The conversation shown alone is the first tab, the shown one, and
	// follows the selection rather than staying.
	if b, ok := headerButton(tt, "Conversation e"); !ok || !b.Checked {
		t.Fatalf("standalone conversation has no shown tab: %+v", b)
	}
	snapshot(t, tt, "workspace-group-standalone")
	a.Data.Unassigned = append(a.Data.Unassigned, &viewmodel.Conversation{ID: "f", Title: "Conversation f"})
	a.Sidebar.Selected = "f"
	tt.Frame()
	tt.Frame()
	if _, ok := headerButton(tt, "Conversation f"); !ok {
		t.Fatal("the standalone tab did not follow the selection")
	}
	if _, ok := headerButton(tt, "Conversation e"); ok {
		t.Fatal("the previous standalone conversation kept a tab")
	}
	pressWorkspaceGroup(t, tt, 1)
	if a.panes.Active != session("d") || a.panes.Displayed() != second {
		t.Fatalf("returning from standalone: active %v", a.panes.Active)
	}
	if _, ok := headerButton(tt, "Conversation f"); ok {
		t.Fatal("the standalone tab stayed after a group showed")
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
	// With only the shown split left, its combined title replaces the tab.
	if _, ok := headerButton(tt, i18n.In("en", "pane_group", "number", 1)); ok {
		t.Fatal("one shown split still has a tab")
	}
	if title, ok := tt.Find("Conversation a · Conversation b"); !ok || title.Y+title.H > HeaderHeight {
		t.Fatalf("one shown split lost its title: %+v", title)
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

// A sidebar click shows another conversation while a kept one runs: the
// selection follows the click through background updates, a running session
// is no tab of its own, and closing a tab that does not show keeps the
// selection and the session.
func TestSidebarSelectionBesideRunningTab(t *testing.T) {
	running := &viewmodel.Conversation{ID: "a", Title: "Conversation a", Status: viewmodel.Running}
	idle := &viewmodel.Conversation{ID: "b", Title: "Conversation b"}
	a := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{running, idle}})
	a.focusPane(session("a"))
	// Kept explicitly, as ⌘T, a double click or restored tabs keep it.
	a.panes.Keep(session("a"))
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("en")
	settle(tt)
	row, ok := ui.AccessInfo{}, false
	for _, info := range tt.AccessAll() {
		if b := info.Bounds; info.Role == ui.RoleButton && info.Name == idle.Title && b.Y >= HeaderHeight && b.X < RailWidth+NavMax {
			row, ok = info, true
		}
	}
	if !ok {
		t.Fatal("no sidebar row for the idle conversation")
	}
	tt.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
	tt.Frame()
	tt.Frame()
	showsIdle := func(when string) {
		t.Helper()
		if a.Sidebar.Selected != "b" || a.panes.Active != session("b") || a.FocusedConversation() != "b" {
			t.Fatalf("%s: selected %q, active %v", when, a.Sidebar.Selected, a.panes.Active)
		}
		if cv := a.Data.Find("a"); cv == nil || cv.Status != viewmodel.Running {
			t.Fatalf("%s: the running session changed: %+v", when, cv)
		}
	}
	showsIdle("sidebar click")
	if b, ok := headerButton(tt, idle.Title); !ok || !b.Checked {
		t.Fatalf("the clicked conversation is not the shown tab: %+v", b)
	}
	if b, ok := headerButton(tt, running.Title); !ok || b.Checked {
		t.Fatalf("the kept running tab: %+v", b)
	}
	// Background projections replace the data while the reply runs.
	for range 3 {
		next := *running
		next.Updated = next.Updated.Add(time.Second)
		other := *idle
		a.ApplyData(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{&next, &other}})
		settle(tt)
	}
	showsIdle("background update")
	if !slices.Equal(a.panes.Kept, []panes.Target{session("a")}) {
		t.Fatalf("background update changed the kept tabs: %v", a.panes.Kept)
	}
	// Closing the kept tab that does not show leaves the selection.
	b, _ := headerButton(tt, running.Title)
	tt.RightClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	if err := tt.ChooseMenuItem(i18n.In("en", "close")); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	showsIdle("closing the running tab")
	if len(a.panes.Kept) != 0 {
		t.Fatalf("closed tab still kept: %v", a.panes.Kept)
	}
	// Running alone is no tab: the shown conversation has the plain title.
	for _, title := range []string{running.Title, idle.Title} {
		if _, ok := headerButton(tt, title); ok {
			t.Fatalf("a single shown conversation still has tab %q", title)
		}
	}
	if bounds, ok := tt.Find(idle.Title); !ok || bounds.Y+bounds.H > HeaderHeight {
		t.Fatalf("the shown conversation has no header title: %+v", bounds)
	}
}

// A passing tab is kept from its menu and stays when another conversation
// shows; closing the shown one shows its neighbour, a group's menu undoes
// the split, and the kept tabs are the window's preferences.
func TestHeaderTabsKeep(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 1280)
	menu := func(title, item string) {
		t.Helper()
		b, ok := headerButton(tt, title)
		if !ok {
			t.Fatalf("no tab %q", title)
		}
		tt.RightClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		tt.Frame()
		if err := tt.ChooseMenuItem(i18n.In("en", item)); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		tt.Frame()
	}
	show := func(id string) {
		t.Helper()
		a.Sidebar.Selected = id
		tt.Frame()
		tt.Frame()
	}
	show("e")
	menu("Conversation e", "workspace_tab_keep")
	if !slices.Equal(a.panes.Kept, []panes.Target{session("e")}) {
		t.Fatalf("kept %v", a.panes.Kept)
	}
	a.Data.Unassigned = append(a.Data.Unassigned, &viewmodel.Conversation{ID: "f", Title: "Conversation f"})
	show("f")
	if _, ok := headerButton(tt, "Conversation e"); !ok {
		t.Fatal("the kept tab left when another conversation showed")
	}
	if b, ok := headerButton(tt, "Conversation f"); !ok || !b.Checked {
		t.Fatalf("the passing tab is not shown: %+v", b)
	}
	// The tabs are in the order they opened: the groups, then e, then f.
	group := func(n int) string { return i18n.In("en", "pane_group", "number", n) }
	order := []string{group(1), group(2), "Conversation e", "Conversation f"}
	x := -1.0
	for _, name := range order {
		b, ok := headerButton(tt, name)
		if !ok || float64(b.Bounds.X) <= x {
			t.Fatalf("the tabs are not in the order %q: %q at %+v", order, name, b.Bounds)
		}
		x = float64(b.Bounds.X)
	}
	if saved := a.prefs().TabOrder; len(saved) != 4 || saved[2] != session("e") || saved[3] != session("f") {
		t.Fatalf("saved order %v", saved)
	}
	if saved := a.prefs().Tabs; !slices.Equal(saved, []panes.Target{session("e")}) {
		t.Fatalf("saved tabs %v", saved)
	}
	restored := NewLive(a.Data)
	restored.SetPrefs(a.prefs())
	if !slices.Equal(restored.panes.Kept, []panes.Target{session("e")}) {
		t.Fatalf("restored tabs %v", restored.panes.Kept)
	}
	// The kept tab, shown, closes to the next tab: the passing one goes with
	// it, so the first group shows.
	show("e")
	menu("Conversation e", "close")
	if len(a.panes.Kept) != 0 || a.panes.Displayed() == nil {
		t.Fatalf("closing the shown tab: kept %v, active %v", a.panes.Kept, a.panes.Active)
	}
	// ⌘W in a conversation standing alone closes its tab and stops keeping
	// it; the session stays.
	a.panes.Keep(session("e"))
	show("e")
	// Choosing the tab puts the focus in the conversation.
	b, _ := headerButton(tt, "Conversation e")
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	tt.Frame()
	if len(a.panes.Kept) != 0 || a.Data.Find("e") == nil {
		t.Fatalf("⌘W on the tab: kept %v", a.panes.Kept)
	}
	menu(i18n.In("en", "pane_group", "number", 1), "workspace_tab_unsplit")
	if len(a.panes.Groups) != 1 {
		t.Fatalf("unsplit left %d groups", len(a.panes.Groups))
	}
}
