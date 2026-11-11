package shell

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The rail's features open pages in the main column, never panel tabs.
func TestRailOpensPages(t *testing.T) {
	t.Run("live plugin navigation", testLivePluginNavigation)
	a, tt := newTester(t)
	clickButton(t, tt, i18n.In("en", "files"))
	tt.Advance(time.Second)
	tt.Frame()
	if a.Page != FilesPage || len(a.Panel.Tabs) != 0 || a.PanelOpen {
		t.Fatalf("Files opened page %d, tabs %v, panel %v", a.Page, a.Panel.Tabs, a.PanelOpen)
	}
	if !tt.HasText("A native desktop Agent workspace.") || !tt.HasText("example.md") {
		t.Fatalf("no files page: %q", tt.Texts())
	}
	snapshot(t, tt, "page-files")
	clickButton(t, tt, i18n.In("en", "git"))
	tt.Frame()
	if a.Page != GitPage || len(a.Panel.Tabs) != 0 || !codeHas(tt, "New text", "workspace: three flat panels") {
		t.Fatalf("Git opened page %d, tabs %v: %q", a.Page, a.Panel.Tabs, tt.Texts())
	}
	snapshot(t, tt, "page-git")
	for _, f := range []struct {
		label string
		page  Page
	}{{"activity", ActivityPage}, {"market_title", MarketPage}, {"shell_sessions", ConversationPage}} {
		clickButton(t, tt, i18n.In("en", f.label))
		tt.Frame()
		if a.Page != f.page {
			t.Errorf("%s opened page %d", f.label, a.Page)
		}
	}
	// Selecting a conversation over a project page goes back to it. The
	// page has no session sidebar, so the selection comes from elsewhere, as
	// from search or a notification.
	a.Navigate(FilesPage)
	tt.Frame()
	if a.Sidebar.Selected == "sailry-go/theme" {
		t.Fatal("the fixture already selects the session")
	}
	a.Sidebar.Selected = "sailry-go/theme"
	tt.Frame()
	if a.Page != ConversationPage {
		t.Errorf("selecting a conversation kept page %d", a.Page)
	}
	t.Run("project overview captures its owner", func(t *testing.T) {
		local := &viewmodel.Project{
			ID: "shared-project", Host: "local-node", Name: "Local project", Path: "/local/project",
			Conversations: []*viewmodel.Conversation{{Node: "local-node", ID: "local-session", Title: "Local session"}},
		}
		remote := &viewmodel.Project{
			ID: "shared-project", Host: "remote-node", Name: "Remote project", Path: "/remote/project", Branch: "main",
			Conversations: []*viewmodel.Conversation{
				{Node: "remote-node", ID: "remote-session", Title: "Remote session", WorktreePath: "/remote/project"},
				{Node: "remote-node", ID: "archived-session", Title: "Archived remote session", Archived: true, WorktreePath: "/remote/worktrees/archive"},
				{Node: "remote-node", ID: "resource-session", Title: "Resource session"},
				{Node: "local-node", ID: "wrong-owner", Title: "Wrong owner"},
			},
		}
		data := &viewmodel.Data{
			Projects: []*viewmodel.Project{local, remote},
			Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{
				{ID: "local-node", Name: "Local host", Local: true},
				{ID: "remote-node", Name: "Remote host"},
			}},
		}
		app := NewLive(data)
		app.NavOpen = false
		app.Sidebar.Selected = "local-session"
		var drafted *viewmodel.Project
		var opened *viewmodel.Conversation
		app.OpenProject(remote, ProjectActions{
			NewSession:     func(project *viewmodel.Project) { drafted = project },
			VisibleSession: func(session *viewmodel.Conversation) bool { return session.ID != "resource-session" },
			OpenSession: func(session *viewmodel.Conversation, done func(bool)) {
				opened = session
				done(false)
			},
		})
		tester := ui.NewTesterWith(app.View, 1280, 820, opaque)
		tester.SetLocale("en")
		tester.Frame()
		if node, project, shown := app.ProjectOwner(); node != remote.Host || project != remote.ID || !shown {
			t.Fatalf("overview owner = %q %q %v", node, project, shown)
		}
		for _, text := range []string{"Remote project", "Remote host", "/remote/project", "main", "Remote session", "Archived remote session"} {
			if !tester.HasText(text) {
				t.Fatalf("overview is missing %q: %q", text, tester.Texts())
			}
		}
		for _, text := range []string{"Local project", "/local/project", "Resource session", "Wrong owner", i18n.In("en", "workspace_preview"), i18n.In("en", "workspace_runtime")} {
			if tester.HasText(text) {
				t.Fatalf("overview shows unrelated content %q: %q", text, tester.Texts())
			}
		}
		clickButton(t, tester, i18n.In("en", "workspace_new_session"))
		if drafted != remote || app.Sidebar.Selected != "local-session" {
			t.Fatalf("new session used %v with selected %q", drafted, app.Sidebar.Selected)
		}
		for _, query := range []string{"ARCHIVED", "WORKTREES/ARCHIVE"} {
			app.overview.search = query
			tester.Frame()
			if !tester.HasText("Archived remote session") || tester.HasText("Remote session") {
				t.Fatalf("query %q has wrong records: %q", query, tester.Texts())
			}
		}
		clickButton(t, tester, "Archived remote session")
		if opened != remote.Conversations[1] || !opened.Archived || app.overview.pending || app.Page != ProjectPage {
			t.Fatalf("record opening lost its capture or failed-state page: opened=%v page=%v", opened, app.Page)
		}
		app.overview.search = "missing"
		tester.Frame()
		if !tester.HasText(i18n.In("en", "connection_search_empty")) || tester.HasText("Archived remote session") {
			t.Fatalf("missing query did not show the empty result: %q", tester.Texts())
		}
		app.Data.Projects = []*viewmodel.Project{local}
		tester.Frame()
		if !tester.HasText(i18n.In("en", "unavailable")) || tester.HasText("Local project") || tester.HasText("/remote/project") {
			t.Fatalf("missing remote project fell back to another owner: %q", tester.Texts())
		}
		if node, project, shown := app.ProjectOwner(); node != remote.Host || project != remote.ID || !shown {
			t.Fatalf("missing project discarded its captured owner: %q %q %v", node, project, shown)
		}
	})
}

func testLivePluginNavigation(t *testing.T) {
	app := NewLive(&viewmodel.Data{
		Unassigned: []*viewmodel.Conversation{{ID: "session", Title: "Listed session"}},
		Workspace:  &viewmodel.Workspace{},
	})
	features := []Feature{
		{ID: "plugin/local/fixture", Label: "Scheduled tasks", DefaultPinned: true},
		{ID: "plugin/remote/fixture", Label: "Remote fixture"},
	}
	app.SetPluginFeatures(features)
	var selected string
	var choices []Choice
	app.Hooks.SelectPlugin = func(id string) bool {
		selected = id
		return true
	}
	app.Hooks.PluginPage = func(context *ui.Context, id string) {
		ui.Text(context, "Mounted "+id)
	}
	app.Hooks.BeforeFrame = func(context *ui.Context) {
		choices = nil
		for _, group := range app.catalog(context, "") {
			choices = append(choices, group.Choices...)
		}
	}
	tester := ui.NewTesterWith(app.View, 1280, 820, opaque)
	tester.SetLocale("en")
	tester.Frame()
	for _, feature := range features {
		if !slices.ContainsFunc(choices, func(choice Choice) bool { return choice.ID == feature.ID }) {
			t.Fatalf("plugin %q is missing from search", feature.ID)
		}
	}
	if !app.unpinned[features[1].ID] || app.unpinned[features[0].ID] || slices.Contains(app.features, "page:activity") {
		t.Fatal("live navigation lost declared pins or retained the preview Activity entry")
	}
	clickButton(t, tester, features[0].Label)
	tester.Frame()
	snapshot(t, tester, "page-plugin")
	if app.Page != PluginPage || selected != features[0].ID || !tester.HasText("Mounted "+features[0].ID) {
		t.Fatal("the rail did not open its captured generic plugin page")
	}
	// A plugin's page has no side column: the sessions and the header's
	// toggle are the conversation's, and ⌘B keeps the saved choice. The
	// page starts at the rail on its first frame.
	toggle := i18n.In("en", "sidebar_toggle")
	if _, ok := headerButton(tester, toggle); ok || sessionsShown(tester) || tester.HasText("Listed session") || app.HasNavigation() {
		t.Fatalf("the plugin page shows the session sidebar: %q", tester.Texts())
	}
	if title, ok := tester.Find(features[0].Label); !ok || title.X != RailWidth+12 {
		t.Fatalf("the plugin page's header starts at %v, want the rail's edge", title)
	}
	if body, ok := tester.Find("Mounted " + features[0].ID); !ok || body.X > RailWidth+1 {
		t.Fatalf("the plugin page starts at %v, want the rail's edge", body)
	}
	tester.Key(ui.Cmd, ui.KeyB)
	settle(tester)
	if !app.NavOpen || sessionsShown(tester) {
		t.Fatal("⌘B changed the sidebar over a plugin page")
	}
	app.SetPluginFeatures(features)
	if !app.unpinned[features[1].ID] {
		t.Fatal("metadata refresh changed the rail pin choice")
	}
	app.SetPluginFeatures(features[1:])
	tester.Frame()
	if app.Page != ConversationPage || app.PluginFeature() != "" {
		t.Fatal("a withdrawn plugin retained its active page")
	}
	if _, ok := headerButton(tester, toggle); !ok || !sessionsShown(tester) || !tester.HasText("Listed session") {
		t.Fatalf("the conversation came back without its sessions: %q", tester.Texts())
	}
	// A feature's rail button draws the glyph its package declares, in the
	// rail's text color as the application's own, and not the puzzle piece;
	// the window at rest asks for no frame.
	var declared []Feature
	for _, feature := range [][3]string{
		{"activity-board", "活动看板", "reicon:ui/grid"}, {"databases", "数据库", "reicon:ui/database"},
		{"reminders", "待办提醒", "reicon:notifications/bell"}, {"scheduled-tasks", "定时任务", "reicon:time/calendar"},
		{"ssh", "SSH", "reicon:it/terminal-square"}, {"statistics", "统计", "reicon:business/chart-bar"},
		{"task-plaza", "任务广场", "reicon:building/buildings"},
	} {
		icon, err := icons.Plugin(feature[2])
		if err != nil || icon == icons.Puzzle {
			t.Fatalf("%s: %v", feature[2], err)
		}
		declared = append(declared, Feature{ID: "plugin/local/" + feature[0], Label: feature[1], Icon: icon, DefaultPinned: true})
	}
	rail := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	rail.SetPluginFeatures(declared)
	tester = ui.NewTesterWith(rail.View, 1280, 820, opaque)
	tester.SetLocale("zh-CN")
	glyph := func(label string) string {
		t.Helper()
		settle(tester)
		if tester.FramePending() {
			t.Fatal("the rail at rest asks for frames")
		}
		button, found := tester.AccessByRole(ui.RoleButton, label)
		if !found {
			t.Fatalf("no rail button %q: %q", label, tester.Texts())
		}
		img, at := tester.Image(), button.Bounds
		var pixels []byte
		for y := int(at.Y); y < int(at.Y+at.H); y++ {
			pixels = append(pixels, img.Pix[img.PixOffset(int(at.X), y):img.PixOffset(int(at.X+at.W), y)]...)
		}
		return string(pixels)
	}
	for _, mode := range []string{"dark", "light"} {
		rail.Mode = map[string]theme.Mode{"dark": theme.Dark, "light": theme.Light}[mode]
		tester.SetDark(mode == "dark")
		settle(tester)
		shot(t, tester, "rail-plugin-features-zh-CN-"+mode)
	}
	drawn, other := glyph("定时任务"), glyph("数据库")
	if len(drawn) != len(other) || drawn == other {
		t.Fatal("two packages' rail buttons draw the same glyph")
	}
	declared[3].Icon = icons.Puzzle
	rail.SetPluginFeatures(declared)
	if glyph("定时任务") == drawn {
		t.Fatal("the rail button draws the placeholder for its declared icon")
	}
}

// The Files and Git page and tab of a project share one state; another
// project keeps its own.
func TestPageAndTabShareProjectState(t *testing.T) {
	a, tt := newTester(t)
	first := a.project()
	if first == nil || a.project() != first {
		t.Fatal("the selected project has no single state")
	}
	a.Navigate(FilesPage)
	tt.Frame()
	clickTree(t, tt, "go.mod")
	tt.Frame()
	if first.Files.File() != "sailry-go/go.mod" {
		t.Fatalf("the page showed %q", first.Files.File())
	}
	// The panel's file tabs are the page's open files, the one the page
	// chose shown beside the tree.
	a.Navigate(ConversationPage)
	a.openPanel(panel.Files)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if row, ok := tt.AccessByRole(ui.RoleTreeItem, "go.mod"); !ok || !row.Selected {
		t.Fatalf("the tree does not select the page's file: %q", tt.Texts())
	}
	if !codeHas(tt, "Code editor", "require github.com/egoist/mygo v0.8.3") {
		t.Fatalf("the Files tab does not show the page's file: %q", tt.Texts())
	}
	if want := []panel.Tab{{Kind: panel.Files, ID: "sailry-go/README.md"}, {Kind: panel.Files, ID: "sailry-go/go.mod"}}; !slices.Equal(a.Panel.Tabs, want) {
		t.Fatalf("the panel's tabs %v, want the page's files", a.Panel.Tabs)
	}
	snapshot(t, tt, "tab-files-shared")
	// And the reverse: a change chosen in the Git tab shows on the page.
	a.openPanel(panel.Changes)
	tt.Frame()
	clickTree(t, tt, "example.md")
	tt.Frame()
	if a.Panel.Active != (panel.Tab{Kind: panel.Changes, ID: "docs/example.md"}) || len(a.Panel.Tabs) != 4 || !codeHas(tt, "New text", "search: command palette") {
		t.Fatalf("the change does not show in the Git tab: %v %q", a.Panel.Tabs, tt.Texts())
	}
	a.Navigate(GitPage)
	tt.Frame()
	if first.Git.Change() != "docs/example.md" || !codeHas(tt, "New text", "search: command palette") {
		t.Fatalf("the page does not show the tab's change %q: %q", first.Git.Change(), tt.Texts())
	}
	// Closing a diff on the page's strip closes its panel tab.
	if len(first.Git.Open()) != 2 {
		t.Fatalf("the page's diffs %v", first.Git.Open())
	}
	first.Git.Close("docs/example.md")
	a.Panel.Sync(first)
	if slices.Contains(a.Panel.Tabs, panel.Tab{Kind: panel.Changes, ID: "docs/example.md"}) {
		t.Fatalf("the closed diff kept its tab: %v", a.Panel.Tabs)
	}
	first.Git.Show("docs/example.md")
	// Another project starts from its own state and leaves this one's.
	other := a.Data.Projects[1]
	a.Sidebar.Selected = other.Conversations[0].ID
	a.Navigate(FilesPage)
	tt.Frame()
	second := a.project()
	if second == first || second.ID != other.ID || second.Files.File() == "sailry-go/go.mod" {
		t.Fatalf("project %q shares the state of %q", other.ID, first.ID)
	}
	if !tt.HasText("A native desktop Agent workspace.") {
		t.Errorf("the other project does not show its own file: %q", tt.Texts())
	}
	if first.Files.File() != "sailry-go/go.mod" || first.Git.Change() != "docs/example.md" {
		t.Errorf("the first project lost its state: %q %q", first.Files.File(), first.Git.Change())
	}
}

// The first frame of a page already lays the side list out at its width
// (220), with no frame at the split's minimum.
func TestSplitFirstFrame(t *testing.T) {
	a, tt := newTester(t)
	a.Navigate(FilesPage)
	tt.Frame()
	row, ok := tt.AccessByRole(ui.RoleTreeItem, "sailry-go")
	if !ok {
		t.Fatalf("no tree: %q", tt.Texts())
	}
	// The row fills the list inside its 8 DIP padding, at the window's
	// right edge.
	if got := 1280 - row.Bounds.X + 8; got < 219 || got > 221 || row.Bounds.W < 203 || row.Bounds.W > 205 {
		t.Errorf("the first frame's side list is %v wide, row %v", got, row.Bounds)
	}
	// Both columns fill the page's height from under the tab strip: the
	// list's first row and the editor's toolbar sit at its top.
	save, _ := tt.AccessByRole(ui.RoleButton, i18n.In("en", "files_save"))
	if row.Bounds.Y > 140 || save.Bounds.Y > 140 {
		t.Errorf("the split's columns do not start at the top: tree row %v, Save %v", row.Bounds, save.Bounds)
	}
}
