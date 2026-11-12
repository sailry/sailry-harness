package shell

import (
	"os"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestProjectMenu covers the sidebar's right-click menus over the demo: every
// item does what it says on the demo's data, in memory, and none of them
// tells a preview's notice or is left dead.
func TestProjectMenu(t *testing.T) {
	en := func(key string, args ...any) string { return i18n.In("en", key, args...) }
	demo := func(t *testing.T) (*App, *ui.Tester) {
		t.Helper()
		a := New(preview.Load())
		a.Settings.Model.DemoMode = true
		tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
		tt.SetLocale("en")
		tt.SetDark(true)
		tt.SetScale(2)
		tt.Frame()
		settle(tt)
		return a, tt
	}
	const name = "harbor-api"
	// menu right-clicks the sidebar's row named row: the header names the
	// project and the session too.
	menu := func(t *testing.T, tt *ui.Tester, row string) []string {
		t.Helper()
		for _, info := range tt.AccessAll() {
			if info.Name == row && info.Bounds.X < 280 && info.Bounds.Y > HeaderHeight {
				tt.RightClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
				return tt.Menu()
			}
		}
		t.Fatalf("no sidebar row %q: %q", row, tt.Texts())
		return nil
	}
	choose := func(t *testing.T, tt *ui.Tester, row string, path ...string) {
		t.Helper()
		menu(t, tt, row)
		if err := tt.ChooseMenuItem(path...); err != nil {
			t.Fatalf("%v: %q", err, tt.Menu())
		}
		settle(tt)
		for _, key := range []string{"preview_notice", "preview", "unavailable"} {
			if tt.HasText(en(key)) {
				t.Fatalf("%v says %q", path, en(key))
			}
		}
	}

	// The reference's project menu: Project overview, Edit project | New
	// session, then the project_menu slot (the commands package's New
	// terminal and the worktrees package's entry) | Copy path | Remove
	// project. No AI CLI, no trust, no records.
	t.Run("Items", func(t *testing.T) {
		for _, mode := range []string{"preview", "demo"} {
			a, tt := newTester(t)
			row := a.Data.Projects[0].Name
			if mode == "demo" {
				a, tt = demo(t)
				row = name
			}
			menu := menu(t, tt, row)
			want := []string{en("workspace_project"), en("project_edit"), en("workspace_new_session"), en("workspace_new_terminal"),
				en("worktree_manage"), en("workspace_copy_path"), en("project_remove")}
			var top []string
			for _, item := range menu {
				if slices.Contains(want, item) {
					top = append(top, item)
				}
			}
			if !slices.Equal(top, want) {
				t.Fatalf("%s: the project menu is %q", mode, menu)
			}
			for _, gone := range []string{"Launch AI CLI", "CLI", "Manage project trust", "Session records"} {
				if slices.Contains(menu, gone) {
					t.Fatalf("%s: the project menu offers %q: %q", mode, gone, menu)
				}
			}
			tt.Key(0, ui.KeyEscape)
		}
	})

	t.Run("Overview", func(t *testing.T) {
		a, tt := demo(t)
		choose(t, tt, name, en("workspace_project"))
		if node, id, shown := a.ProjectOwner(); !shown || id != name || node == "" && a.overviewProject() == nil {
			t.Fatalf("Project overview shows page %d, project %q", a.Page, id)
		}
		// The page's records open their session.
		pr := a.overviewProject()
		title := pr.Conversations[len(pr.Conversations)-1].Title
		for _, info := range tt.AccessAll() {
			if info.Name == title && info.Bounds.X > 300 {
				tt.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
				break
			}
		}
		settle(tt)
		if a.Page != ConversationPage || a.Sidebar.Selected != pr.Conversations[len(pr.Conversations)-1].ID {
			t.Fatalf("the record did not open: page %d, selected %q", a.Page, a.Sidebar.Selected)
		}
	})

	// The page's records are the list Settings pages use: one filled card,
	// its rows across it at the row metrics and a hairline apart, the empty
	// state inside the card.
	t.Run("Records", func(t *testing.T) {
		a, tt := demo(t)
		choose(t, tt, name, en("workspace_project"))
		pr := a.overviewProject()
		rows := func() []ui.AccessInfo {
			var out []ui.AccessInfo
			for _, cv := range pr.Conversations {
				for _, info := range tt.AccessAll() {
					if info.Name == cv.Title && info.Role == ui.RoleButton && info.Bounds.X > 300 {
						out = append(out, info)
					}
				}
			}
			slices.SortFunc(out, func(x, y ui.AccessInfo) int { return int(x.Bounds.Y - y.Bounds.Y) })
			return out
		}
		list := rows()
		if len(list) != len(pr.Conversations) {
			t.Fatalf("%d record rows for %d sessions", len(list), len(pr.Conversations))
		}
		for i, row := range list {
			if row.Bounds.H < 56 || row.Bounds.X != list[0].Bounds.X || row.Bounds.W != list[0].Bounds.W {
				t.Fatalf("row %d is %+v, the first %+v", i, row.Bounds, list[0].Bounds)
			}
			// The card's hairlines are drawn over the rows' shared edge and
			// take no room (kit.Card).
			if i > 0 && row.Bounds.Y != list[i-1].Bounds.Y+list[i-1].Bounds.H {
				t.Fatalf("rows %d and %d are not adjacent: %+v %+v", i-1, i, list[i-1].Bounds, row.Bounds)
			}
		}
		img := tt.Image()
		edge := int(list[1].Bounds.Y * 2)
		x := int(list[0].Bounds.X+list[0].Bounds.W/2) * 2
		if img.At(x, edge) == img.At(x, edge+12) && img.At(x, edge-1) == img.At(x, edge+12) {
			t.Fatalf("no hairline between the first two rows")
		}
		// The card is filled: a row's corner is not the page behind it.
		inside := img.At(int(list[0].Bounds.X+4)*2, int(list[0].Bounds.Y+4)*2)
		outside := img.At(int(list[0].Bounds.X-12)*2, int(list[0].Bounds.Y+4)*2)
		if inside == outside {
			t.Fatalf("the records' card has no fill: %v inside and outside", inside)
		}
		if os.Getenv("SAILRY_SNAPSHOTS") != "" {
			for _, mode := range []string{"dark", "light"} {
				for _, backdrop := range []bool{false, true} {
					b := New(preview.Load())
					b.Settings.Model.DemoMode = true
					b.Mode = map[string]theme.Mode{"dark": theme.Dark, "light": theme.Light}[mode]
					options, file := opaque, "project-page-"+mode
					if backdrop {
						packages := theme.Packages()
						b.Theme = packages[len(packages)-1].ID
						options, file = ui.TesterOptions{}, "project-page-backdrop-"+mode
					}
					bt := ui.NewTesterWith(b.View, 1280, 820, options)
					bt.SetLocale("zh-CN")
					bt.SetDark(mode == "dark")
					bt.SetScale(2)
					bt.Frame()
					settle(bt)
					b.Sidebar.OpenOverview(nil, b.Data.Projects[0])
					settle(bt)
					snapshot(t, bt, file)
				}
			}
		}
		// A search filters the rows; one that matches nothing shows the
		// empty state inside the card, and so does a tab without records.
		a.overview.search = pr.Conversations[0].Title
		settle(tt)
		if got := rows(); len(got) != 1 || got[0].Name != pr.Conversations[0].Title {
			t.Fatalf("the search left %d rows", len(got))
		}
		a.overview.search = "no such record"
		settle(tt)
		if len(rows()) != 0 || !tt.HasText(en("connection_search_empty")) {
			t.Fatalf("no empty search state: %q", tt.Texts())
		}
		a.overview.search, a.overview.tab = "", 1
		settle(tt)
		if !tt.HasText(en("workspace_no_terminal")) {
			t.Fatalf("no empty terminals state: %q", tt.Texts())
		}
		// A terminal made from the page lists in the Terminals tab.
		clickButton(t, tt, en("workspace_new_terminal"))
		settle(tt)
		a.Navigate(ProjectPage)
		settle(tt)
		if tt.HasText(en("workspace_no_terminal")) || !tt.HasText(pr.Path) {
			t.Fatalf("the new terminal is not listed: %q", tt.Texts())
		}
	})

	t.Run("Edit", func(t *testing.T) {
		a, tt := demo(t)
		choose(t, tt, name, en("project_edit"))
		if d := a.Sidebar.Draft(); !d.Open {
			t.Fatalf("Edit project opened no editor: %q", tt.Texts())
		}
	})

	t.Run("NewSession", func(t *testing.T) {
		a, tt := demo(t)
		pr := a.Data.ProjectOf(name + "/flaky-test")
		n := len(pr.Conversations)
		choose(t, tt, name, en("workspace_new_session"))
		if len(pr.Conversations) != n+1 || a.Data.ProjectOf(a.Sidebar.Selected) != pr || a.Page != ConversationPage {
			t.Fatalf("New session: %d sessions, selected %q", len(pr.Conversations), a.Sidebar.Selected)
		}
	})

	t.Run("NewTerminal", func(t *testing.T) {
		a, tt := demo(t)
		choose(t, tt, name, en("workspace_new_terminal"))
		if !a.PanelOpen || a.Panel.Active.Kind != panel.Terminal || a.Data.ProjectOf(a.Sidebar.Selected).Name != name {
			t.Fatalf("New terminal: panel open %v, active %v, selected %q", a.PanelOpen, a.Panel.Active, a.Sidebar.Selected)
		}
		if !slices.ContainsFunc(tt.Texts(), func(s string) bool { return strings.Contains(s, "go test ./internal/...") }) {
			t.Fatalf("the terminal shows no screen: %q", tt.Texts())
		}
		if tt.HasText(en("workspace_terminal_preview")) {
			t.Fatal("the demo's terminal shows the preview's placeholder")
		}
		snapshot(t, tt, "demo-terminal")
	})

	t.Run("CopyPath", func(t *testing.T) {
		a, tt := demo(t)
		choose(t, tt, name, en("workspace_copy_path"))
		if got := tt.Clipboard(); got != a.Data.ProjectOf(name+"/flaky-test").Path {
			t.Fatalf("Copy path copied %q", got)
		}
	})

	t.Run("Worktrees", func(t *testing.T) {
		a, tt := demo(t)
		pr := a.Data.ProjectOf(name + "/flaky-test")
		trees, sessions := len(pr.Worktrees), len(pr.Conversations)
		choose(t, tt, name, en("worktree_manage"), en("worktree_create"))
		if len(pr.Worktrees) <= trees || len(pr.Conversations) != sessions+1 || a.Data.ProjectOf(a.Sidebar.Selected) != pr {
			t.Fatalf("Create worktree: %d worktrees, %d sessions", len(pr.Worktrees), len(pr.Conversations))
		}
		made := pr.Worktrees[len(pr.Worktrees)-1]
		// A worktree of the list opens the project's page.
		choose(t, tt, name, en("worktree_manage"), made.Branch)
		if _, id, shown := a.ProjectOwner(); !shown || id != pr.ID {
			t.Fatalf("the worktree opened page %d", a.Page)
		}
		// Removing names the worktree and asks once.
		n := len(pr.Worktrees)
		choose(t, tt, name, en("worktree_manage"), en("worktree_remove"), made.Branch)
		if !tt.HasText(en("worktree_remove_confirm")) || len(pr.Worktrees) != n {
			t.Fatalf("Remove worktree did not ask: %q", tt.Texts())
		}
		clickButton(t, tt, en("worktree_remove"))
		settle(tt)
		if len(pr.Worktrees) != n-1 || slices.Contains(pr.Worktrees, made) {
			t.Fatalf("the worktree was not removed: %+v", pr.Worktrees)
		}
	})

	t.Run("Remove", func(t *testing.T) {
		a, tt := demo(t)
		pr := a.Data.ProjectOf(name + "/flaky-test")
		n := len(a.Data.Projects)
		choose(t, tt, name, en("project_remove"))
		if !tt.HasText(en("project_remove_named", "name", pr.Path)) {
			t.Fatalf("Remove project did not ask: %q", tt.Texts())
		}
		clickButton(t, tt, en("project_remove"))
		settle(tt)
		if len(a.Data.Projects) != n-1 || slices.Contains(a.Data.Projects, pr) || a.Data.Find(a.Sidebar.Selected) == nil {
			t.Fatalf("the project was not removed: %d projects, selected %q", len(a.Data.Projects), a.Sidebar.Selected)
		}
	})

	// A session's menu in the demo: forks are made in memory; Export, Open
	// folder and New window, which need a Node or the system, are not
	// offered rather than left dead.
	t.Run("Session", func(t *testing.T) {
		a, tt := demo(t)
		cv := a.Data.Find(name + "/flaky-test")
		pr := a.Data.ProjectOf(cv.ID)
		menu := menu(t, tt, cv.Title)
		for _, key := range []string{"conversation_export", "files_open", "session_new_window"} {
			if slices.Contains(menu, en(key)) {
				t.Fatalf("the demo's session menu offers %q: %q", en(key), menu)
			}
		}
		tt.Key(0, ui.KeyEscape)
		settle(tt)
		n, trees := len(pr.Conversations), len(pr.Worktrees)
		choose(t, tt, cv.Title, en("session_fork"), en("session_fork_here"))
		fork := a.Data.Find(a.Sidebar.Selected)
		if len(pr.Conversations) != n+1 || fork == cv || fork.Title != cv.Title || len(fork.Turns) != len(cv.Turns) {
			t.Fatalf("Fork conversation: %d sessions, selected %q", len(pr.Conversations), a.Sidebar.Selected)
		}
		a.Sidebar.Selected = cv.ID
		settle(tt)
		a.Sidebar.SetCollapsed(nil)
		choose(t, tt, cv.Title, en("session_fork"), en("session_fork_worktree"))
		fork = a.Data.Find(a.Sidebar.Selected)
		if len(pr.Conversations) != n+2 || len(pr.Worktrees) <= trees || fork.Worktree != pr.Worktrees[len(pr.Worktrees)-1].Branch {
			t.Fatalf("Fork in new worktree: %d sessions, %d worktrees, fork %+v", len(pr.Conversations), len(pr.Worktrees), fork.Worktree)
		}
	})

	// last clicks the topmost button of a name: a dialog's over a row's.
	last := func(t *testing.T, tt *ui.Tester, label string) {
		t.Helper()
		all := tt.AccessAll()
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Role == ui.RoleButton && all[i].Name == label {
				b := all[i].Bounds
				tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
				settle(tt)
				return
			}
		}
		t.Fatalf("no button %q: %q", label, tt.Texts())
	}

	// Every other item of a session's menu changes the demo's memory: pin,
	// read state, the copies, rename, and archive and delete after their
	// confirmation.
	t.Run("SessionItems", func(t *testing.T) {
		a, tt := demo(t)
		cv := a.Data.Find(name + "/flaky-test")
		pr := a.Data.ProjectOf(cv.ID)
		title := cv.Title
		choose(t, tt, title, en("workspace_pin"))
		if !cv.Pinned || cv.PinOrder == nil {
			t.Fatal("Pin left the session unpinned")
		}
		choose(t, tt, title, en("workspace_unpin"))
		if cv.Pinned {
			t.Fatal("Unpin left the session pinned")
		}
		cv.Status = preview.Idle
		settle(tt)
		choose(t, tt, title, en("session_mark_unread"))
		if cv.Status != preview.Unread {
			t.Fatalf("Mark unread left status %d", cv.Status)
		}
		choose(t, tt, title, en("session_mark_read"))
		if cv.Status != preview.Idle {
			t.Fatalf("Mark read left status %d", cv.Status)
		}
		for key, want := range map[string]string{"workspace_copy_title": title, "session_copy_id": cv.ID, "workspace_copy_path": pr.Path} {
			choose(t, tt, title, en("content_copy"), en(key))
			if got := tt.Clipboard(); got != want {
				t.Fatalf("%s copied %q, want %q", key, got, want)
			}
		}
		choose(t, tt, title, en("workspace_rename"))
		tt.Key(ui.Cmd, ui.KeyA)
		tt.Type("Stabilize the suite")
		tt.Key(0, ui.KeyEnter)
		settle(tt)
		if cv.Title != "Stabilize the suite" {
			t.Fatalf("Rename left %q", cv.Title)
		}
		title = cv.Title
		choose(t, tt, title, en("workspace_archive"))
		if !tt.HasText(en("session_archive_named", "name", title)) || cv.Archived {
			t.Fatalf("Archive did not ask: %q", tt.Texts())
		}
		last(t, tt, en("workspace_archive"))
		if !cv.Archived || a.Data.Find(a.Sidebar.Selected) == nil {
			t.Fatalf("Archive: archived %v, selected %q", cv.Archived, a.Sidebar.Selected)
		}
		other := pr.Conversations[0]
		if other == cv {
			other = pr.Conversations[1]
		}
		n := len(pr.Conversations)
		choose(t, tt, other.Title, en("session_delete"))
		if !tt.HasText(en("session_delete_named", "name", other.Title)) || len(pr.Conversations) != n {
			t.Fatalf("Delete did not ask: %q", tt.Texts())
		}
		last(t, tt, en("session_delete"))
		if len(pr.Conversations) != n-1 || slices.Contains(pr.Conversations, other) {
			t.Fatalf("Delete left %d of %d sessions", len(pr.Conversations), n)
		}
	})

	// The groups' headers: the chats' "+", the projects' sort menu and Add
	// project, and a project's rows dragged into another order.
	t.Run("Groups", func(t *testing.T) {
		a, tt := demo(t)
		press := func(label string) {
			t.Helper()
			found := sidebarNamed(tt, label)
			if len(found) == 0 {
				t.Fatalf("the sidebar has no %q: %q", label, tt.Texts())
			}
			b := found[0].Bounds
			tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
			settle(tt)
		}
		chats := len(a.Data.Unassigned)
		press(en("workspace_new_session"))
		if len(a.Data.Unassigned) != chats+1 || a.Sidebar.Selected != a.Data.Unassigned[0].ID {
			t.Fatalf("the chats' + made %d chats, selected %q", len(a.Data.Unassigned)-chats, a.Sidebar.Selected)
		}
		press(en("sidebar_sort"))
		clickItem(t, tt, en("session_order_name"))
		settle(tt)
		if *a.Sidebar.Sort != int(sidebar.SortName) {
			t.Fatalf("the sort menu left order %d", *a.Sidebar.Sort)
		}
		projects := len(a.Data.Projects)
		press(en("project_add"))
		if !a.Sidebar.Draft().Open {
			t.Fatal("Add project opened no editor")
		}
		clickField(t, tt, en("project_path"))
		tt.Type("~/Work/demo-tool")
		tt.Frame()
		last(t, tt, en("settings_save"))
		if len(a.Data.Projects) != projects+1 || a.Data.Projects[projects].Name != "demo-tool" {
			t.Fatalf("the editor added %d projects", len(a.Data.Projects)-projects)
		}
		// Under the creation order a project's row drags to another place.
		*a.Sidebar.Sort = int(sidebar.SortCreated)
		pr := a.Data.ProjectOf(name + "/flaky-test")
		var rows []*preview.Conversation
		for _, cv := range pr.Conversations {
			if !cv.Archived && !cv.Pinned {
				rows = append(rows, cv)
			}
		}
		a.Sidebar.Reveal(pr.ID)
		// Tall enough for the project's rows to be clear of the window's edge.
		tt.SetSize(1280, 1400)
		settle(tt)
		from, to := sidebarNamed(tt, rows[0].Title), sidebarNamed(tt, rows[1].Title)
		if len(from) == 0 || len(to) == 0 {
			t.Fatalf("rows not shown: %q", tt.Texts())
		}
		dragTo(t, tt, from[0], from[0].Bounds.X+from[0].Bounds.W/2, to[0].Bounds.Y+to[0].Bounds.H-4, nil)
		if at := slices.Index(pr.Conversations, rows[0]); at != slices.Index(pr.Conversations, rows[1])+1 {
			t.Fatalf("the drag left %q at %d", rows[0].ID, at)
		}
	})

	// The rail's host menu over the demo: Host information and Add project
	// work; Edit, which is the controller's, is absent where the preview
	// would only tell its notice.
	t.Run("HostMenu", func(t *testing.T) {
		open := func(tt *ui.Tester) []string {
			t.Helper()
			info, ok := tt.AccessByRole(ui.RoleMenuButton, en("hosts"))
			if !ok {
				t.Fatalf("no host picker: %q", tt.Texts())
			}
			tt.RightClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
			tt.Frame()
			return tt.Menu()
		}
		a, tt := demo(t)
		items := open(tt)
		if slices.Contains(items, en("connection_action_edit")) {
			t.Fatalf("the demo's host menu offers Edit: %q", items)
		}
		for _, key := range []string{"workspace_host", "project_add", "port_mappings"} {
			if !slices.Contains(items, en(key)) {
				t.Fatalf("the demo's host menu lacks %q: %q", en(key), items)
			}
		}
		if err := tt.ChooseMenuItem(en("workspace_host")); err != nil {
			t.Fatal(err)
		}
		settle(tt)
		if a.Page != HostPage {
			t.Fatalf("Host information showed page %d", a.Page)
		}
		open(tt)
		if err := tt.ChooseMenuItem(en("project_add")); err != nil {
			t.Fatal(err)
		}
		settle(tt)
		if !a.Sidebar.Draft().Open {
			t.Fatal("Add project opened no editor")
		}
		_, pt := newTester(t)
		if items := open(pt); !slices.Contains(items, en("connection_action_edit")) {
			t.Fatalf("the preview's host menu lost Edit: %q", items)
		}
	})

	// The search palette's project opens the demo project's page, as the
	// menu's Project overview does.
	t.Run("Search", func(t *testing.T) {
		a, tt := demo(t)
		tt.Key(ui.Cmd, ui.KeyK)
		tt.Frame()
		tt.Frame()
		tt.Key(0, ui.KeyDown) // Projects, the second folder
		tt.Frame()
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		if a.search.folder != "projects" {
			t.Fatalf("the search is in folder %q", a.search.folder)
		}
		tt.Type("lumen")
		tt.Frame()
		tt.Key(0, ui.KeyEnter)
		settle(tt)
		if _, id, shown := a.ProjectOwner(); !shown || id != "lumen-web" || a.search.open {
			t.Fatalf("the project choice showed page %d, project %q", a.Page, id)
		}
	})

	// The launcher lists Browser, Terminal, Files and Git in the demo, each
	// opens, and the "+" menu lists the same.
	t.Run("Launcher", func(t *testing.T) {
		a, tt := demo(t)
		a.Sidebar.Selected = name + "/flaky-test"
		settle(tt)
		a.PanelOpen = true
		settle(tt)
		for _, r := range panel.Resources[4:] {
			if tt.HasText(en(r.Label)) {
				t.Fatalf("the launcher lists %q", en(r.Label))
			}
		}
		snapshot(t, tt, "demo-launcher")
		for _, r := range panel.Resources[:4] {
			for len(a.Panel.Tabs) > 0 {
				a.Panel.Close(a.Panel.Tabs[0])
			}
			settle(tt)
			if !tt.HasText(en(r.Label)) {
				t.Fatalf("the launcher lacks %q: %q", en(r.Label), tt.Texts())
			}
			// The rail names Files and Git too: the launcher's row is in the panel.
			for _, info := range tt.AccessAll() {
				if info.Name == en(r.Label) && info.Bounds.X > 1280-a.PanelWidth {
					tt.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
					break
				}
			}
			settle(tt)
			if a.Panel.Active.Kind != r.Kind {
				t.Fatalf("%s opened %v", r.Label, a.Panel.Active)
			}
			switch r.Kind {
			case panel.Browser:
				// The tab is named after the page, which shows under the address.
				if !tt.HasText(a.Data.Workspace.Page.Title) || !slices.ContainsFunc(tt.Texts(), func(s string) bool { return strings.Contains(s, "fifteen minutes") }) ||
					tt.HasText(en("resource_browser_preview")) {
					t.Fatalf("the demo's browser shows no page: %q", tt.Texts())
				}
				snapshot(t, tt, "demo-browser")
			case panel.Terminal:
				if tt.HasText(en("workspace_terminal_preview")) {
					t.Fatal("the demo's terminal shows the preview's placeholder")
				}
			}
		}
	})
}
