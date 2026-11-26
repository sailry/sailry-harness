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
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func workspaceGroupsFixture(t *testing.T, width int, assembly Assembly) (*App, *ui.Tester) {
	t.Helper()
	data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}}
	for _, id := range []string{"a", "b", "c", "d", "e"} {
		data.Unassigned = append(data.Unassigned, &viewmodel.Conversation{ID: id, Title: "Conversation " + id})
	}
	a := NewLiveWithAssembly(data, assembly)
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
	a := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{first, second}}, TestHarnessAssembly())
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
	a.OpenConversation(first.ID)
	frame()
	keptTab(first.Title)
	a.OpenConversation(second.ID)
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
	a, tt := workspaceGroupsFixture(t, 1280, Assembly{})
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
	a.OpenConversation("e")
	tt.Frame()
	for _, number := range []int{1, 2} {
		b, ok := headerButton(tt, i18n.In("en", "pane_group", "number", number))
		if !ok || b.Checked {
			t.Fatalf("standalone group %d button: %+v", number, b)
		}
	}
	// The conversation opened alone is a tab, the shown one, and stays when
	// another one opens.
	if b, ok := headerButton(tt, "Conversation e"); !ok || !b.Checked {
		t.Fatalf("standalone conversation has no shown tab: %+v", b)
	}
	snapshot(t, tt, "workspace-group-standalone")
	a.Data.Unassigned = append(a.Data.Unassigned, &viewmodel.Conversation{ID: "f", Title: "Conversation f"})
	a.OpenConversation("f")
	tt.Frame()
	tt.Frame()
	if b, ok := headerButton(tt, "Conversation f"); !ok || !b.Checked {
		t.Fatalf("the opened conversation is not the shown tab: %+v", b)
	}
	if b, ok := headerButton(tt, "Conversation e"); !ok || b.Checked {
		t.Fatalf("the previously opened conversation lost its tab: %+v", b)
	}
	pressWorkspaceGroup(t, tt, 1)
	if a.panes.Active != session("d") || a.panes.Displayed() != second {
		t.Fatalf("returning from standalone: active %v", a.panes.Active)
	}
	if b, ok := headerButton(tt, "Conversation f"); !ok || b.Checked {
		t.Fatalf("the open tab left when a group showed: %+v", b)
	}
	// Switching only changes focus: both original trees remain intact.
	if a.panes.Groups[0] != second || a.panes.Groups[1] != first {
		t.Fatal("switching groups replaced a split tree")
	}
	snapshot(t, tt, "workspace-group-buttons")
}

func TestWorkspaceGroupTerminalFocus(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 1280, TestHarnessAssembly())
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
	a, tt := workspaceGroupsFixture(t, 1280, Assembly{})
	first := a.panes.Groups[0]
	if !a.panes.Detach(session("d")) {
		t.Fatal("cannot dissolve the second group")
	}
	a.focusPane(session("e"))
	tt.Frame()
	// One remaining split still has an entry while a standalone view without a tab shows.
	if _, ok := headerButton(tt, "Conversation e"); ok {
		t.Fatal("an implicit standalone selection opened a tab")
	}
	pressWorkspaceGroup(t, tt, 1)
	if a.panes.Active != session("b") || a.Sidebar.Selected != "b" || a.panes.Displayed() != first {
		t.Fatalf("one saved group: active %v, selected %q, original tree displayed %t", a.panes.Active, a.Sidebar.Selected, a.panes.Displayed() == first)
	}
	// With only the shown split left, its combined title replaces the tab.
	if _, ok := headerButton(tt, i18n.In("en", "pane_group", "number", 1)); ok {
		t.Fatal("one shown split still has a tab")
	}
	if title, ok := tt.Find("Conversation a · Conversation b"); !ok || title.Y+title.H > HeaderHeight {
		t.Fatalf("one shown split lost its title: %+v", title)
	}
	// Remove the remembered pane while the group is inactive.
	if !a.panes.Drop(session("a"), session("c"), panes.Bottom) {
		t.Fatal("cannot add a pane to the remaining group")
	}
	a.focusPane(session("b"))
	a.focusPane(session("e"))
	if !a.panes.Detach(session("b")) {
		t.Fatal("cannot detach the remembered pane")
	}
	tt.Frame()
	pressWorkspaceGroup(t, tt, 1)
	if a.panes.Active != session("a") || a.Sidebar.Selected != "a" || a.panes.Group(a.panes.Active) < 0 {
		t.Fatalf("removed remembered pane kept focus: active %v, selected %q", a.panes.Active, a.Sidebar.Selected)
	}
}

func TestWorkspaceGroupsScrollInNarrowHeader(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 640, Assembly{})
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

// A sidebar click opens another conversation's tab while an open one runs:
// the selection follows the click through background updates, and closing
// the running tab, which does not show, keeps the selection, the session and
// its reply.
func TestSidebarSelectionBesideRunningTab(t *testing.T) {
	running := &viewmodel.Conversation{ID: "a", Title: "Conversation a", Status: viewmodel.Running}
	idle := &viewmodel.Conversation{ID: "b", Title: "Conversation b"}
	a := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{running, idle}})
	a.focusPane(session("a"))
	// Open already, as ⌘T, an earlier click or restored tabs leave it.
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
	if !slices.Equal(a.panes.Kept, []panes.Target{session("a"), session("b")}) {
		t.Fatalf("background update changed the open tabs: %v", a.panes.Kept)
	}
	// Closing the open tab that does not show leaves the selection.
	b, _ := headerButton(tt, running.Title)
	tt.RightClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	if err := tt.ChooseMenuItem(i18n.In("en", "close")); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	showsIdle("closing the running tab")
	if !slices.Equal(a.panes.Kept, []panes.Target{session("b")}) {
		t.Fatalf("closed tab still open: %v", a.panes.Kept)
	}
	// The running session has no tab now; the clicked one's stays, alone.
	if _, ok := headerButton(tt, running.Title); ok {
		t.Fatal("the closed running tab is still in the band")
	}
	if b, ok := headerButton(tt, idle.Title); !ok || !b.Checked {
		t.Fatalf("the single open tab is not shown: %+v", b)
	}
}

// sidebarRow returns the sidebar's row of the conversation titled title.
func sidebarRow(t *testing.T, tt *ui.Tester, title string) ui.AccessInfo {
	t.Helper()
	for _, info := range tt.AccessAll() {
		if b := info.Bounds; info.Role == ui.RoleButton && info.Name == title && b.Y >= HeaderHeight && b.X < RailWidth+NavMax {
			return info
		}
	}
	t.Fatalf("no sidebar row %q", title)
	return ui.AccessInfo{}
}

// A click on a sidebar row opens its conversation as a normal tab: one tab
// shows alone, A, B and A again leave both, each once, and a session's tab
// has only Close. Closing a running session's tab leaves its reply running,
// and after the last tab closes neither frames, background updates nor a
// selection the window falls back to open one again.
func TestSidebarClicksOpenTabs(t *testing.T) {
	data := func(status viewmodel.Status, ids ...string) *viewmodel.Data {
		d := &viewmodel.Data{Workspace: &viewmodel.Workspace{}}
		for _, id := range ids {
			cv := &viewmodel.Conversation{ID: id, Title: "Conversation " + id}
			if id == "a" {
				cv.Status = status
			}
			d.Unassigned = append(d.Unassigned, cv)
		}
		return d
	}
	a := NewLive(data(viewmodel.Idle, "a", "b", "c"))
	a.Sidebar.Selected = "a"
	a.focusPane(session("a"))
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("en")
	settle(tt)
	open := func(want ...string) {
		t.Helper()
		var targets []panes.Target
		for _, id := range want {
			targets = append(targets, session(id))
		}
		if !slices.Equal(a.panes.Kept, targets) {
			t.Fatalf("open tabs %v, want %v", a.panes.Kept, targets)
		}
	}
	shownTab := func(id string) {
		t.Helper()
		if b, ok := headerButton(tt, "Conversation "+id); !ok || !b.Checked || a.Sidebar.Selected != id || a.panes.Active != session(id) {
			t.Fatalf("%q is not the shown tab: %+v, selected %q, active %v", id, b, a.Sidebar.Selected, a.panes.Active)
		}
	}
	noTabs := func(when string) {
		t.Helper()
		for _, id := range []string{"a", "b", "c"} {
			if _, ok := headerButton(tt, "Conversation "+id); ok {
				t.Fatalf("%s: a tab for %q", when, id)
			}
		}
	}
	clickRow := func(id string) {
		t.Helper()
		row := sidebarRow(t, tt, "Conversation "+id)
		tt.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
		settle(tt)
	}
	tabMenu := func(id string) []string {
		t.Helper()
		b, ok := headerButton(tt, "Conversation "+id)
		if !ok {
			t.Fatalf("no tab %q", id)
		}
		tt.RightClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		tt.Frame()
		return tt.Menu()
	}
	// The conversation the window started on was not opened: no tab.
	open()
	noTabs("start")
	clickRow("a")
	open("a")
	shownTab("a")
	clickRow("b")
	open("a", "b")
	shownTab("b")
	clickRow("a")
	open("a", "b")
	shownTab("a")
	if b, ok := headerButton(tt, "Conversation b"); !ok || b.Checked {
		t.Fatalf("returning to a lost b's tab: %+v", b)
	}
	if items := tabMenu("b"); !slices.Equal(items, []string{i18n.In("en", "close")}) {
		t.Fatalf("a session tab's menu: %q", items)
	}
	tt.CloseMenu()
	// Closing the shown, running tab shows its neighbour; the reply runs on.
	a.ApplyData(data(viewmodel.Running, "a", "b", "c"))
	settle(tt)
	tabMenu("a")
	if err := tt.ChooseMenuItem(i18n.In("en", "close")); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	open("b")
	shownTab("b")
	if cv := a.Data.Find("a"); cv == nil || cv.Status != viewmodel.Running {
		t.Fatalf("closing the tab changed the running session: %+v", cv)
	}
	// The last tab closes to the plain title and stays closed.
	tabMenu("b")
	if err := tt.ChooseMenuItem(i18n.In("en", "close")); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	for range 3 {
		a.ApplyData(data(viewmodel.Running, "a", "b", "c"))
		settle(tt)
	}
	open()
	noTabs("the last tab closed")
	if a.Sidebar.Selected != "b" {
		t.Fatalf("closing the last tab left the conversation: %q", a.Sidebar.Selected)
	}
	// The shown conversation goes: the window falls back to another, which
	// is not opened.
	a.ApplyData(data(viewmodel.Running, "a", "c"))
	settle(tt)
	if a.Sidebar.Selected == "b" || a.Sidebar.Selected == "" {
		t.Fatalf("no fallback selection: %q", a.Sidebar.Selected)
	}
	open()
	noTabs("fallback selection")
	// A click on the row shown without a tab opens it.
	clickRow(a.Sidebar.Selected)
	open(a.Sidebar.Selected)
	shownTab(a.Sidebar.Selected)
}

// A new session's tab is its draft's until the Node creates it, then the
// session's, in the same place.
func TestDraftTabBecomesSession(t *testing.T) {
	a := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{
		{ID: "a", Title: "Conversation a"}, {ID: "draft/1"},
	}})
	a.OpenConversation("a")
	a.OpenConversation("draft/1")
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("en")
	settle(tt)
	a.ReplaceConversationID("draft/1", "created")
	settle(tt)
	want := []panes.Target{session("a"), session("created")}
	if !slices.Equal(a.panes.Kept, want) || !slices.Equal(a.panes.order, want) || a.panes.Active != session("created") {
		t.Fatalf("promoted draft: tabs %v, order %v, active %v", a.panes.Kept, a.panes.order, a.panes.Active)
	}
}

// Opened tabs stay when another conversation opens; closing the shown one
// shows its neighbour, a group's menu undoes the split, and the open tabs
// are the window's preferences.
func TestHeaderTabsOpen(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 1280, Assembly{})
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
		a.OpenConversation(id)
		tt.Frame()
		tt.Frame()
	}
	show("e")
	if !slices.Equal(a.panes.Kept, []panes.Target{session("e")}) {
		t.Fatalf("open %v", a.panes.Kept)
	}
	a.Data.Unassigned = append(a.Data.Unassigned, &viewmodel.Conversation{ID: "f", Title: "Conversation f"})
	show("f")
	if _, ok := headerButton(tt, "Conversation e"); !ok {
		t.Fatal("the open tab left when another conversation showed")
	}
	if b, ok := headerButton(tt, "Conversation f"); !ok || !b.Checked {
		t.Fatalf("the opened tab is not shown: %+v", b)
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
	if saved := a.prefs().Tabs; !slices.Equal(saved, []panes.Target{session("e"), session("f")}) {
		t.Fatalf("saved tabs %v", saved)
	}
	restored := NewLive(a.Data)
	restored.SetPrefs(a.prefs())
	if !slices.Equal(restored.panes.Kept, []panes.Target{session("e"), session("f")}) {
		t.Fatalf("restored tabs %v", restored.panes.Kept)
	}
	// The open tab, shown, closes to the next tab.
	show("e")
	menu("Conversation e", "close")
	if !slices.Equal(a.panes.Kept, []panes.Target{session("f")}) || a.panes.Active != session("f") {
		t.Fatalf("closing the shown tab: open %v, active %v", a.panes.Kept, a.panes.Active)
	}
	// ⌘W in a conversation standing alone closes its tab, and the tab before
	// it, a group's, shows; the session stays.
	// Choosing the tab puts the focus in the conversation.
	b, _ := headerButton(tt, "Conversation f")
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	tt.Frame()
	if len(a.panes.Kept) != 0 || a.Data.Find("f") == nil || a.panes.Displayed() == nil {
		t.Fatalf("⌘W on the tab: open %v, active %v", a.panes.Kept, a.panes.Active)
	}
	menu(i18n.In("en", "pane_group", "number", 1), "workspace_tab_unsplit")
	if len(a.panes.Groups) != 1 {
		t.Fatalf("unsplit left %d groups", len(a.panes.Groups))
	}
}

// Dragging a header tab past another moves it there: the strip and the
// saved order follow, and a plain click still shows a tab.
func TestHeaderTabsDragToReorder(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 1280, Assembly{})
	a.Data.Unassigned = append(a.Data.Unassigned, &viewmodel.Conversation{ID: "f", Title: "Conversation f"})
	for _, id := range []string{"e", "f"} {
		a.OpenConversation(id)
		tt.Frame()
		tt.Frame()
	}
	group := func(n int) string { return i18n.In("en", "pane_group", "number", n) }
	order := func() []string {
		t.Helper()
		names := []string{group(1), group(2), "Conversation e", "Conversation f"}
		slices.SortFunc(names, func(x, y string) int {
			bx, okx := headerButton(tt, x)
			by, oky := headerButton(tt, y)
			if !okx || !oky {
				t.Fatalf("missing tab %q or %q", x, y)
			}
			return int(bx.Bounds.X - by.Bounds.X)
		})
		return names
	}
	if got := order(); !slices.Equal(got, []string{group(1), group(2), "Conversation e", "Conversation f"}) {
		t.Fatalf("opening order %q", got)
	}
	drag := func(source, target string, after bool) {
		t.Helper()
		s, _ := headerButton(tt, source)
		d, _ := headerButton(tt, target)
		x, y := s.Bounds.X+s.Bounds.W/2, s.Bounds.Y+s.Bounds.H/2
		to := d.Bounds.X + d.Bounds.W/4
		if after {
			to = d.Bounds.X + d.Bounds.W*3/4
		}
		tt.Press(x, y)
		tt.Frame()
		for _, step := range []float32{x + (to-x)/4, x + (to-x)/2, to} {
			tt.Move(step, y)
			tt.Frame()
		}
		tt.Release(to, y)
		tt.Frame()
		tt.Frame()
	}
	// f before the first group, then the first group past e.
	drag("Conversation f", group(1), false)
	want := []string{"Conversation f", group(1), group(2), "Conversation e"}
	if got := order(); !slices.Equal(got, want) {
		t.Fatalf("after dragging f to the front: %q, want %q", got, want)
	}
	drag(group(1), "Conversation e", true)
	want = []string{"Conversation f", group(2), "Conversation e", group(1)}
	if got := order(); !slices.Equal(got, want) {
		t.Fatalf("after dragging the group past e: %q, want %q", got, want)
	}
	restored := NewLive(a.Data)
	restored.SetPrefs(a.prefs())
	if !slices.Equal(restored.panes.order, a.panes.order) {
		t.Fatalf("restored order %v, want %v", restored.panes.order, a.panes.order)
	}
	// The drags chose no tab; a click still does.
	b, _ := headerButton(tt, "Conversation e")
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	tt.Frame()
	if a.panes.Active != session("e") {
		t.Fatalf("clicking the moved tab: active %v", a.panes.Active)
	}
}

func TestHeaderTabsDragToSplit(t *testing.T) {
	a := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{
		{ID: "a", Title: "Conversation a"}, {ID: "b", Title: "Conversation b"},
	}}, TestHarnessAssembly())
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.SetScale(0.25)
	frame := func() { tt.Frame(); tt.Frame() }
	header := func(label string) ui.AccessInfo {
		t.Helper()
		tab, ok := headerButton(tt, label)
		if !ok {
			t.Fatalf("missing header tab %q", label)
		}
		return tab
	}
	term := panes.Target{Kind: panes.Terminal, ID: "terminal"}
	a.OpenWorkspaceTerminal(term.ID, "Shell")
	a.KeepWorkspaceTerminal(term.ID)
	for _, id := range []string{"a", "b", "a"} {
		a.OpenConversation(id)
		frame()
	}

	order := slices.Clone(a.panes.order)
	tab := header("Conversation a")
	dragTo(t, tt, visibleSidebarRow(t, tt, "Conversation b"), tab.Bounds.X+tab.Bounds.W/4, tab.Bounds.Y+tab.Bounds.H/2, nil)
	if !slices.Equal(a.panes.order, order) || a.panes.Active != session("a") {
		t.Fatalf("sidebar drop on header changed order or focus: %v, %v", a.panes.order, a.panes.Active)
	}
	dragTo(t, tt, header("Conversation b"), 1150, 400, nil)
	if got := a.panes.Members(session("a")); !slices.Equal(got, []panes.Target{session("a"), session("b")}) {
		t.Fatalf("dropped conversation tab: %v", got)
	}
	a.OpenWorkspaceTerminal(term.ID, "Shell")
	frame()
	dragTo(t, tt, header(i18n.In("en", "pane_group", "number", 1)), 1150, 400, nil)
	if a.panes.Group(term) >= 0 || len(a.panes.Groups) != 1 || a.panes.Active != term {
		t.Fatalf("group tab dropped onto terminal: groups %v, active %v", a.panes.Groups, a.panes.Active)
	}
	pressWorkspaceGroup(t, tt, 1)
	dragTo(t, tt, header("Shell"), 1150, 400, nil)
	if got := a.panes.Members(term); !slices.Equal(got, []panes.Target{session("a"), session("b"), term}) || a.panes.Active != term {
		t.Fatalf("dropped terminal tab: group %v, active %v", got, a.panes.Active)
	}
}

// A session shown without a tab leaves ⌘W to the window, while its pane keys
// still work. Closing its tab before that leaves the session as it was.
func TestCloseWithoutTabKeepsPaneKeys(t *testing.T) {
	a := New(preview.Load())
	reached := 0
	tt := ui.NewTesterWith(func(c *ui.Context) {
		a.View(c)
		if c.Shortcut(CloseFocused.Mods, CloseFocused.Key) {
			reached++
		}
	}, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	shown := session(preview.Big)
	a.panes.Kept = nil
	a.OpenConversation(preview.Big)
	tt.Frame()
	status := a.Data.Find(preview.Big).Status
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	if len(a.panes.Kept) != 0 || reached != 0 || a.panes.Active != shown || a.Data.Find(preview.Big) == nil || a.Data.Find(preview.Big).Status != status {
		t.Fatalf("⌘W on the tab: kept %v, reached the window %d, active %v", a.panes.Kept, reached, a.panes.Active)
	}
	if !a.standsAlone() || a.tabClosable() {
		t.Fatalf("without a tab: stands alone %v, closable %v", a.standsAlone(), a.tabClosable())
	}
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	if reached != 1 || a.Page != ConversationPage || a.panes.Active != shown {
		t.Fatalf("⌘W without a tab: reached the window %d, page %q, active %v", reached, a.Page, a.panes.Active)
	}
	tt.Key(ui.Cmd, ui.KeyT)
	tt.Frame()
	if created := a.panes.Active; created == shown || !slices.Contains(a.panes.Kept, created) {
		t.Fatalf("pane keys without a tab: active %v, kept %v", created, a.panes.Kept)
	}
}

// A session that is only absent, as of a host offline, keeps its tab and its
// place in a saved split; a confirmed deletion takes both.
func TestForgetSessionOnlyOnDeletion(t *testing.T) {
	a, tt := workspaceGroupsFixture(t, 1280, Assembly{})
	a.OpenConversation("e")
	tt.Frame()
	a.Data.Unassigned = slices.DeleteFunc(a.Data.Unassigned, func(cv *viewmodel.Conversation) bool { return cv.ID == "e" || cv.ID == "b" })
	a.OpenConversation("a")
	tt.Frame()
	tt.Frame()
	saved := func() []panes.Target {
		var out []panes.Target
		for _, g := range a.prefs().Panes {
			out = append(out, g.Targets()...)
		}
		return out
	}
	if p := a.prefs(); !slices.Contains(p.Tabs, session("e")) || !slices.Contains(p.TabOrder, session("e")) || !slices.Contains(saved(), session("b")) {
		t.Fatalf("an absent session lost its place: tabs %v, order %v, panes %v", p.Tabs, p.TabOrder, saved())
	}
	a.ForgetSession("e")
	a.ForgetSession("b")
	tt.Frame()
	if p := a.prefs(); slices.Contains(p.Tabs, session("e")) || slices.Contains(p.TabOrder, session("e")) || slices.Contains(saved(), session("b")) {
		t.Fatalf("a deleted session kept its place: tabs %v, order %v, panes %v", p.Tabs, p.TabOrder, saved())
	}
}
