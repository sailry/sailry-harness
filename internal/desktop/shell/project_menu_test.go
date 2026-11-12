package shell

import (
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
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
