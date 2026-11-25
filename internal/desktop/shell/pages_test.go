package shell

import (
	"path"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The rail's features open pages in the main column, never panel tabs.
func TestRailOpensPages(t *testing.T) {
	t.Run("live plugin navigation", testLivePluginNavigation)
	t.Run("live project pages", testLiveProjectPages)
	t.Run("search opens the project", testSearchOpensProject)
	a, tt := newTester(t)
	clickButton(t, tt, i18n.In("en", "files"))
	tt.Advance(time.Second)
	tt.Frame()
	if a.Page != FilesPage || len(a.Panel.Tabs) != 0 || a.PanelOpen {
		t.Fatalf("Files opened page %q, tabs %v, panel %v", a.Page, a.Panel.Tabs, a.PanelOpen)
	}
	const selectFile = "Open fixture go.mod"
	selection, found := tt.AccessByRole(ui.RoleButton, selectFile)
	if !tt.HasText("A native desktop Agent workspace.") || !found || selection.Bounds.X < 1280-320 || selection.Bounds.W <= 0 {
		t.Fatalf("the Files page did not host the document and right-hand selection: %+v, %q", selection, tt.Texts())
	}
	snapshot(t, tt, "page-files")
	clickButton(t, tt, selectFile)
	pr := a.project()
	if a.Page != FilesPage || pr == nil || pr.Files.File() != "sailry-go/go.mod" || !codeHas(tt, "Code editor", "require github.com/egoist/mygo v0.8.3") || len(a.Panel.Tabs) != 0 || a.PanelOpen {
		t.Fatalf("the fixture's selection did not open in its own page: page %q, panel %v, %q", a.Page, a.Panel.Tabs, tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "git"))
	tt.Frame()
	if a.Page != GitPage || len(a.Panel.Tabs) != 0 || !codeHas(tt, "New text", "workspace: three flat panels") {
		t.Fatalf("Git opened page %q, tabs %v: %q", a.Page, a.Panel.Tabs, tt.Texts())
	}
	snapshot(t, tt, "page-git")
	for _, f := range []struct {
		label string
		page  Page
	}{{"activity", ActivityPage}, {"market_title", MarketPage}, {"shell_sessions", ConversationPage}} {
		clickButton(t, tt, a.translations.In("en", f.label))
		tt.Frame()
		if a.Page != f.page {
			t.Errorf("%s opened page %q", f.label, a.Page)
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
		t.Errorf("selecting a conversation kept page %q", a.Page)
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
		for _, text := range []string{"Remote project", "Remote host", "/remote/project", "main", "Remote session"} {
			if !tester.HasText(text) {
				t.Fatalf("overview is missing %q: %q", text, tester.Texts())
			}
		}
		for _, text := range []string{"Local project", "/local/project", "Archived remote session", "Resource session", "Wrong owner", i18n.In("en", "workspace_preview"), "Runtime"} {
			if tester.HasText(text) {
				t.Fatalf("overview shows unrelated content %q: %q", text, tester.Texts())
			}
		}
		clickButton(t, tester, i18n.In("en", "workspace_new_session"))
		if drafted != remote || app.Sidebar.Selected != "local-session" {
			t.Fatalf("new session used %v with selected %q", drafted, app.Sidebar.Selected)
		}
		click(t, tester, i18n.In("en", "project_archived"))
		tester.Frame()
		if !tester.HasText("Archived remote session") || tester.HasText("Remote session") || tester.HasText("Wrong owner") || tester.HasText("Resource session") {
			t.Fatalf("the archive tab has wrong records: %q", tester.Texts())
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
		// An archived record's menu restores it; a session's own menu is
		// the sidebar row's.
		var restored *viewmodel.Conversation
		app.Sidebar.Actions = &sidebar.Actions{Unarchive: func(session *viewmodel.Conversation) { restored = session }}
		app.Sidebar.Actions.RegisterProjectMenu(sidebar.ProjectMenuAction{
			BeforeCopy: func(c *ui.Context, m *kit.Menu, _ *viewmodel.Project) { m.Item(i18n.T(c, "worktree_create")) },
			AfterCopy:  func(c *ui.Context, m *kit.Menu, _ *viewmodel.Project) { m.Item(i18n.T(c, "worktree_remove")) },
		})
		if err := tester.RightClick("Archived remote session"); err != nil {
			t.Fatal(err)
		}
		unarchive := i18n.In("en", "backup_unarchive")
		if menu := tester.Menu(); !slices.Contains(menu, unarchive) || slices.Contains(menu, i18n.In("en", "workspace_archive")) || slices.Contains(menu, i18n.In("en", "workspace_rename")) {
			t.Fatalf("archived record menu = %q", menu)
		}
		if err := tester.ChooseMenuItem(unarchive); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		if restored != remote.Conversations[1] {
			t.Fatalf("Unarchive restored %v", restored)
		}
		app.overview.search = ""
		settle(tester)
		click(t, tester, i18n.In("en", "workspace_sessions"))
		settle(tester)
		if err := tester.RightClick("Remote session"); err != nil {
			t.Fatal(err)
		}
		if menu := tester.Menu(); !slices.Contains(menu, i18n.In("en", "workspace_archive")) || slices.Contains(menu, unarchive) {
			t.Fatalf("record menu = %q", menu)
		}
		// The header's More menu acts on the page's project.
		click(t, tester, i18n.In("en", "more"))
		tester.Frame()
		for _, key := range []string{"project_edit", "worktree_create", "workspace_copy_path", "worktree_remove", "project_remove"} {
			if !tester.HasText(i18n.In("en", key)) {
				t.Fatalf("More lacks %q: %q", i18n.In("en", key), tester.Texts())
			}
		}
		// Launch AI CLI is neither an item of the menu nor a button of the
		// page (AGENTS.md, Removed UI).
		for _, info := range tester.AccessAll() {
			if info.Name == "Launch AI CLI" {
				t.Fatalf("the project page offers Launch AI CLI: %+v", info)
			}
		}
		click(t, tester, i18n.In("en", "workspace_copy_path"))
		tester.Frame()
		if got := tester.Clipboard(); got != remote.Path {
			t.Fatalf("Copy path wrote %q", got)
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

// A live window's Files and Git pages show the selected conversation's
// project, through the same adapter the panel's tabs use; without a project
// they offer New project.
func testLiveProjectPages(t *testing.T) {
	const file, selectFile = "main.go", "Open fixture main.go"
	project := &viewmodel.Project{ID: "project", Host: "node", Name: "Live project", Path: "/live/project",
		Conversations: []*viewmodel.Conversation{{Node: "node", ID: "session", Title: "Live session"}}}
	other := &viewmodel.Project{ID: "other-project", Host: "other-node", Name: "Other project", Path: "/other/project"}
	assembly := TestHarnessAssembly()
	filesPage := slices.IndexFunc(assembly.WorkspacePages, func(p NativePage) bool { return p.Page == FilesPage })
	if filesPage < 0 {
		t.Fatal("the test assembly did not register the Files page")
	}
	assembly.WorkspacePages[filesPage].RenderDetails = func(c *ui.Context, _ *App, owner *panel.Project) {
		ui.Button(c.Key("open-fixture"), selectFile).OnClick(func() { owner.Files.Show(file) })
	}
	app := NewLiveWithAssembly(&viewmodel.Data{
		Projects:   []*viewmodel.Project{project, other},
		Unassigned: []*viewmodel.Conversation{{Node: "node", ID: "loose", Title: "Loose session"}},
		Workspace:  &viewmodel.Workspace{},
	}, assembly)
	app.Sidebar.Selected = "session"
	var read []string
	workspace := func(id, text string, data *viewmodel.Workspace) *panel.Project {
		var owner *panel.Project
		owner = panel.NewLiveProject(id, data, &panel.Actions{ReadFile: func(path string) {
			if !owner.FileLoaded(path) {
				read = append(read, id+"/"+path)
				owner.SetFile(path, text)
			}
		}})
		return owner
	}
	live := workspace("worktree", "package live", &viewmodel.Workspace{
		Changes: []viewmodel.FileChange{{Path: file, Status: "M", Unstaged: "M", Added: 1}},
	})
	live.SetGitState(panel.GitState{Known: true, Kind: "repository", Head: true})
	switched := workspace("other-worktree", "package other", &viewmodel.Workspace{})
	var shown *panel.Project
	app.Hooks.WorkspaceProject = func() *panel.Project { return shown }
	otherTarget := ResourceTarget{Node: other.Host, Project: other.ID}
	app.Hooks.ResourceWorkspace = func(target ResourceTarget) *panel.Project {
		if target == otherTarget {
			return switched
		}
		return nil
	}
	tester := ui.NewTesterWith(app.View, 1280, 820, opaque)
	tester.SetLocale("en")
	tester.Frame()
	unavailable, prompt, create := i18n.In("en", "unavailable"), i18n.In("en", "resource_project_empty"), i18n.In("en", "project_new")

	// The project is known and its worktree is not yet.
	clickButton(t, tester, i18n.In("en", "files"))
	tester.Frame()
	if _, hasSelection := tester.AccessByRole(ui.RoleButton, selectFile); app.Page != FilesPage || !tester.HasText(unavailable) || tester.HasText(prompt) || hasSelection || len(read) != 0 {
		t.Fatalf("a project without a worktree showed a fixture or read a file: read %v, %q", read, tester.Texts())
	}

	shown = live
	tester.Frame()
	if tester.HasText(unavailable) {
		t.Fatalf("the live Files page is unavailable: %q", tester.Texts())
	}
	clickButton(t, tester, selectFile)
	tester.Frame()
	if live.Files.File() != file || !slices.Equal(read, []string{"worktree/" + file}) || !codeHas(tester, "Code editor", "package live") {
		t.Fatalf("the page did not route the fixture's selection to its project: shown %q, read %v, %q", live.Files.File(), read, tester.Texts())
	}

	app.SetResourceTarget(otherTarget)
	tester.Frame()
	if app.ResourceProject() != other || switched.Files.File() != "" || codeHas(tester, "Code editor", "package live") || len(read) != 1 {
		t.Fatalf("selecting a project retained the previous document or read an unopened one: project %v, read %v", app.ResourceProject(), read)
	}
	clickButton(t, tester, selectFile)
	tester.Frame()
	wantReads := []string{"worktree/" + file, "other-worktree/" + file}
	if switched.Files.File() != file || live.Files.File() != file || !slices.Equal(read, wantReads) || !codeHas(tester, "Code editor", "package other") {
		t.Fatalf("identical file paths did not use distinct project owners: read %v, %q", read, tester.Texts())
	}
	app.SetResourceTarget(ResourceTarget{})
	tester.Frame()
	if app.ResourceProject() != project || !codeHas(tester, "Code editor", "package live") || !slices.Equal(read, wantReads) {
		t.Fatalf("returning to the first project did not restore its document: read %v, %q", read, tester.Texts())
	}
	if len(app.Panel.Tabs) != 0 || app.PanelOpen {
		t.Fatalf("the page opened panel tabs %v, panel %v", app.Panel.Tabs, app.PanelOpen)
	}
	clickButton(t, tester, i18n.In("en", "git"))
	tester.Frame()
	if _, ok := tester.AccessByRole(ui.RoleTreeItem, "main.go"); app.Page != GitPage || !ok || tester.HasText(unavailable) {
		t.Fatalf("the live Git page does not list the change: %q", tester.Texts())
	}
	// The panel's tabs are the page's items once the conversation shows.
	app.Navigate(ConversationPage)
	app.openPanel(panel.Files)
	tester.Frame()
	if !slices.Contains(app.Panel.Tabs, panel.Tab{Kind: panel.Files, ID: "main.go"}) {
		t.Fatalf("the panel lost the page's file: %v", app.Panel.Tabs)
	}

	// Without a project either page asks for one.
	app.PanelOpen = false
	app.Sidebar.Selected, shown = "loose", nil
	for _, page := range []Page{FilesPage, GitPage} {
		app.Navigate(page)
		tester.Frame()
		if !tester.HasText(prompt) || tester.HasText(unavailable) || app.Sidebar.Editing() {
			t.Fatalf("page %q without a project: %q", page, tester.Texts())
		}
	}
	clickButton(t, tester, create)
	tester.Frame()
	if !app.Sidebar.Editing() || !tester.HasText(i18n.In("en", "project_name")) {
		t.Fatalf("New project did not open the project editor: %q", tester.Texts())
	}
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
	// The tester's window has no traffic lights, as Windows: the app's name
	// stays over the rail and the page's header starts past it.
	name, named := tester.Find(i18n.In("en", "app"))
	if title, ok := tester.Find(features[0].Label); !named || !ok || title.X != max(RailWidth, name.W+24)+12 {
		t.Fatalf("the plugin page's header starts at %v, want past the name %v", title, name)
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
	// A feature's rail button draws the outline glyph its package declares
	// for navigation, in the rail's text color as the application's own, and
	// not the puzzle piece;
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
	// This test keeps both documents to compare their shared state across
	// hosts; an ordinary click otherwise replaces the passing tab.
	keepShown := func(label string) {
		t.Helper()
		for _, info := range tt.AccessAll() {
			if info.Role == ui.RoleButton && info.Checked && info.Name == label {
				tt.RightClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
				tt.Frame()
				if err := tt.ChooseMenuItem(i18n.In("en", "workspace_tab_keep")); err != nil {
					t.Fatal(err)
				}
				tt.Frame()
				return
			}
		}
		t.Fatalf("no shown document tab %q: %q", label, tt.Texts())
	}
	keepShown("README.md")
	clickButton(t, tt, "Open fixture go.mod")
	tt.Frame()
	if first.Files.File() != "sailry-go/go.mod" {
		t.Fatalf("the page showed %q", first.Files.File())
	}
	// The panel's file tabs reuse the page's open files and active document.
	a.Navigate(ConversationPage)
	a.openPanel(panel.Files)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if a.Panel.Active != (panel.Tab{Kind: panel.Files, ID: "sailry-go/go.mod"}) {
		t.Fatalf("the panel did not show the page's active file: %v", a.Panel.Active)
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
	keepShown(path.Base(first.Git.Change()))
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

// On the first frame the shell gives the resource its full details column,
// without flashing at the minimum width or omitting the column.
func TestSplitFirstFrame(t *testing.T) {
	const (
		width, height, detailsWidth = 1280, 820, 320
		documentLabel               = "Fixture document region"
		detailsLabel                = "Fixture details region"
	)
	type region struct {
		frame int
		box   ui.Rect
	}
	var document, details region
	frame := 0
	probe := func(c *ui.Context, label string, first *region) {
		ui.Column(c.Key(label)).Grow(1).MinHeight(0).FillWidth().Role(ui.RoleRegion).Label(label).
			Draw(func(_ *ui.Painter, r ui.Rect) {
				if first.frame == 0 {
					*first = region{frame, r}
				}
			}).Children(func() { ui.Text(c, label) })
	}
	assembly := Assemble(func(a *Assembly) {
		a.WorkspacePages = append(a.WorkspacePages, NativePage{
			Page: FilesPage, Resource: panel.Files,
			Feature: Feature{ID: string(FilesPage), Icon: icons.Folder, Label: "Files layout fixture"},
			Render: func(c *ui.Context, app *App) {
				app.RenderResource(c, func(*panel.Project) { probe(c, documentLabel, &document) })
			},
			RenderDetails: func(c *ui.Context, _ *App, _ *panel.Project) { probe(c, detailsLabel, &details) },
		})
	})
	a := NewWithAssembly(preview.Load(), assembly)
	tt := ui.NewTesterWith(func(c *ui.Context) {
		if a.Page == FilesPage {
			frame++
		}
		a.View(c)
	}, width, height, opaque)
	a.Navigate(FilesPage)
	tt.Frame()
	// The tester can run more than one frame to settle invalidations.
	if document.frame != 1 || details.frame != 1 {
		t.Fatalf("region first paints: document frame %d, details frame %d, want both at 1", document.frame, details.frame)
	}
	main, side := document.box, details.box
	if side.X < width-detailsWidth-1 || side.X > width-detailsWidth+1 || side.W < detailsWidth-1 || side.W > detailsWidth+1 || side.X+side.W < width-1 || side.X+side.W > width+1 {
		t.Errorf("the first frame's details did not fill the right-hand %d DIP column: %v", detailsWidth, side)
	}
	if main.X < RailWidth || main.X > RailWidth+1 || main.W <= 0 || main.X+main.W < side.X-2 || main.X+main.W > side.X+1 {
		t.Errorf("the document did not fill its column to the left of details: document %v, details %v", main, side)
	}
	if main.Y > 140 || side.Y > 140 || main.H <= 0 || side.H <= 0 || main.Y+main.H < height-1 || side.Y+side.H < height-1 {
		t.Errorf("the split's columns do not fill the page's height: document %v, details %v", main, side)
	}
	for _, label := range []string{documentLabel, detailsLabel} {
		count := 0
		for _, entry := range tt.AccessAll() {
			if entry.Role == ui.RoleRegion && entry.Name == label {
				count++
			}
		}
		if count != 1 {
			t.Errorf("the shell mounted %d copies of %q, want one", count, label)
		}
	}
}

// The search's project opens the project's page where the window has one,
// as the sidebar's Project does, and reveals its rows in the preview.
func testSearchOpensProject(t *testing.T) {
	choose := func(a *App, tt *ui.Tester, name string) {
		t.Helper()
		a.search = search{open: true, folder: "projects"}
		tt.Frame()
		tt.Type(name)
		tt.Frame()
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		if a.search.open {
			t.Fatalf("the search stayed open: %q", tt.Texts())
		}
	}
	project := &viewmodel.Project{ID: "project", Host: "node", Name: "Live project", Path: "/live/project"}
	app := NewLive(&viewmodel.Data{Projects: []*viewmodel.Project{project}, Workspace: &viewmodel.Workspace{}})
	var opened *viewmodel.Project
	app.Sidebar.Actions = &sidebar.Actions{Overview: func(pr *viewmodel.Project) { opened = pr }}
	tester := ui.NewTesterWith(app.View, 1280, 820, opaque)
	tester.SetLocale("en")
	tester.Frame()
	choose(app, tester, "Live project")
	if opened != project {
		t.Fatalf("the search opened %+v", opened)
	}
	a, tt := newTester(t)
	a.Navigate(ActivityPage)
	a.NavOpen = false
	tt.Frame()
	choose(a, tt, a.Data.Projects[0].Name)
	if a.Page != ConversationPage || !a.NavOpen {
		t.Fatalf("the preview's project: page %q, sidebar %v", a.Page, a.NavOpen)
	}
}
