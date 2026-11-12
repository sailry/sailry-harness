package panel

import (
	"github.com/sailry/sailry-harness/internal/desktop/kit/kittest"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// TestTabsModel groups the pure tab and history model; each case keeps its own fixture.
func TestTabsModel(t *testing.T) {
	t.Run("OpenedAddsOnce", testOpenedAddsOnce)
	t.Run("ClosedChoosesNext", testClosedChoosesNext)
	t.Run("FlatTabs", testFlatTabs)
	t.Run("ItemTabs", testItemTabs)
	t.Run("BrowserHistory", testBrowserHistory)
	t.Run("SessionResources", testSessionResources)
	t.Run("EmptyStates", testEmptyStates)
}

// The launcher and the "+" menu list the reference's four kinds, Browser,
// Terminal, Files and Git, in the preview, the demo and the live window:
// never History or Documents, which keep their tabs and open from a turn and
// from a document's card. Documents without a document
// opens a tab that takes the first document once there is one, and a change
// named as the one to show first opens no diff.
func testSessionResources(t *testing.T) {
	kinds := func(s *State) []Kind {
		var out []Kind
		for _, r := range Resources {
			if s.lists(r.Kind) {
				out = append(out, r.Kind)
			}
		}
		return out
	}
	four := []Kind{Browser, Terminal, Files, Changes}
	s := New(&viewmodel.Workspace{})
	if got := kinds(s); !slices.Equal(got, four) {
		t.Fatalf("the preview lists %v", got)
	}
	s.Demo = true
	if got := kinds(s); !slices.Equal(got, four) {
		t.Fatalf("the demo lists %v", got)
	}
	s.Demo = false
	s.Live = true
	// A terminal needs a worktree: an unsent chat without a project has
	// none, so Terminal is neither listed nor opened by its shortcut there.
	if got := kinds(s); !slices.Equal(got, []Kind{Browser, Files, Changes}) || s.Offers(Terminal) {
		t.Fatalf("a live draft without a worktree lists %v", got)
	}
	if s.Open(Terminal); len(s.Tabs) != 0 {
		t.Fatalf("Terminal opened without a worktree: %v", s.Tabs)
	}
	s.Worktree = true
	if got := kinds(s); !slices.Equal(got, []Kind{Browser, Terminal, Files, Changes}) {
		t.Fatalf("a live draft lists %v", got)
	}
	// Terminal starts another terminal every time; Browser shows its page.
	s.Open(Terminal)
	s.Open(Browser)
	s.Open(Terminal)
	s.Open(Browser)
	if want := []Tab{{Terminal, "1"}, {Browser, "2"}, {Terminal, "3"}}; !slices.Equal(s.Tabs, want) || s.Active != want[1] {
		t.Fatalf("opened %v, active %v", s.Tabs, s.Active)
	}
	// Closing a live terminal's tab asks first, then ends its shell: the
	// tab leaves when its Node has closed the terminal.
	var closed []string
	s.TerminalActions = &TerminalActions{Close: func(id string) { closed = append(closed, id) }}
	tt := ui.NewTester(func(c *ui.Context) { View(c, s, nil, kit.KeyText) }, 560, 600)
	tt.SetLocale("en")
	if s.Close(Tab{Terminal, "3"}); len(s.Tabs) != 3 || len(closed) != 0 {
		t.Fatalf("a terminal tab closed unasked: %v, %v", s.Tabs, closed)
	}
	tt.Frame()
	if !tt.HasText("Close terminal") || tt.Click("Cancel") != nil {
		t.Fatalf("no confirmation: %q", tt.Texts())
	}
	tt.Frame()
	if len(closed) != 0 || tt.HasText("Close terminal") {
		t.Fatalf("Cancel closed %v", closed)
	}
	s.Close(Tab{Terminal, "3"})
	tt.Frame()
	if err := tt.Click("Close"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !slices.Equal(closed, []string{"3"}) || len(s.Tabs) != 3 {
		t.Fatalf("confirmed: closed %v, tabs %v", closed, s.Tabs)
	}
	// Another conversation's tabs take the strip, and come back.
	mine, shown := slices.Clone(s.Tabs), s.Active
	s.Mount([]Tab{{Browser, "9"}}, Tab{}, false)
	if !slices.Equal(s.Tabs, []Tab{{Browser, "9"}}) || s.Active != (Tab{Browser, "9"}) {
		t.Fatalf("mounted %v, active %v", s.Tabs, s.Active)
	}
	if s.New(Browser); s.Active != (Tab{Browser, "10"}) {
		t.Fatalf("a new page beside a mounted one is %v", s.Active)
	}
	s.Mount(mine, shown, false)
	if !slices.Equal(s.Tabs, mine) || s.Active != shown {
		t.Fatalf("mounted back %v, active %v", s.Tabs, s.Active)
	}
	s.Mount(nil, Tab{}, false)
	s.TerminalActions = nil
	s.Session = true
	if got := kinds(s); !slices.Equal(got, four) || s.Offers(History) || s.Offers(Documents) {
		t.Fatalf("a live session lists %v", got)
	}
	s.Open(Documents)
	blank := Tab{Kind: Documents}
	if !slices.Equal(s.Tabs, []Tab{blank}) || s.Active != blank {
		t.Fatalf("Documents without a document: tabs %v, active %v", s.Tabs, s.Active)
	}
	s.Open(History)
	s.Data.Documents = []viewmodel.Document{{ID: "one", Title: "One"}, {ID: "two", Title: "Two"}}
	s.Show(blank)
	s.nameDocument()
	first := Tab{Documents, "one"}
	if !slices.Equal(s.Tabs, []Tab{first, {Kind: History}}) || s.Active != first {
		t.Fatalf("the first document did not take the tab: %v, active %v", s.Tabs, s.Active)
	}
	// The blank tab beside the first document's leaves.
	s.Tabs = append(s.Tabs, blank)
	s.nameDocument()
	if !slices.Equal(s.Tabs, []Tab{first, {Kind: History}}) {
		t.Fatalf("a second tab for the first document: %v", s.Tabs)
	}
	if s.Close(first); slices.Contains(s.Tabs, first) || slices.Contains(s.Tabs, blank) {
		t.Fatalf("the closed document tab is still open: %v", s.Tabs)
	}

	pr := NewLiveProject("worktree", &viewmodel.Workspace{}, &Actions{})
	pr.Git.First("a.go")
	pr.Git.First("b.go")
	if pr.Git.Change() != "a.go" || len(pr.Git.Open()) != 0 {
		t.Fatalf("the first change: shown %q, open %v", pr.Git.Change(), pr.Git.Open())
	}
	s = New(pr.Data)
	s.Sync(pr)
	if len(s.Tabs) != 0 {
		t.Fatalf("a change to show first opened a tab: %v", s.Tabs)
	}
	s.Open(Changes)
	s.Sync(pr)
	diff := Tab{Changes, "a.go"}
	if !slices.Equal(s.Tabs, []Tab{diff}) {
		t.Fatalf("Git did not open on the first change: %v", s.Tabs)
	}
	s.Close(diff)
	pr.Git.First("a.go")
	s.Sync(pr)
	if len(s.Tabs) != 0 {
		t.Fatalf("the closed diff came back: %v", s.Tabs)
	}
}

// A live tab without content shows the standard empty state, or a spinner
// while its content is read: never a blank body.
func testEmptyStates(t *testing.T) {
	s := New(&viewmodel.Workspace{})
	s.Live, s.Session = true, true
	loading := false
	s.Actions = &Actions{Loading: func(Kind) bool { return loading }}
	live := NewLiveProject("worktree", &viewmodel.Workspace{Files: &viewmodel.FileNode{Name: "worktree"}}, &Actions{})
	var pr *Project
	tt := ui.NewTester(func(c *ui.Context) { View(c, s, pr, kit.KeyText) }, 560, 600)
	text := func(key string) string { return i18n.In("en", key) }
	for _, row := range []struct {
		name    string
		tab     Tab
		project *Project
		want    string
	}{
		{"History", Tab{Kind: History}, nil, "panel_history_empty"},
		{"Documents", Tab{Kind: Documents}, nil, "panel_documents_empty"},
		{"a document that is gone", Tab{Documents, "gone"}, nil, "panel_documents_empty"},
		{"Files without a project", Tab{Kind: Files}, nil, "resource_project_empty"},
		{"Git without a project", Tab{Kind: Changes}, nil, "resource_project_empty"},
		{"Files without a file", Tab{Kind: Files}, live, "file_empty"},
		{"a browser without a page owner", Tab{Browser, "1"}, nil, "unavailable"},
		{"an artifact that is gone", Tab{Artifact, "gone"}, nil, "artifact_preview_failed"},
		{"a subagent without a renderer", Tab{Subagent, "gone"}, nil, "chat_child_unavailable"},
	} {
		s.Tabs, s.Active, pr = []Tab{row.tab}, row.tab, row.project
		tt.Frame()
		tt.Frame()
		if !tt.HasText(text(row.want)) {
			t.Errorf("%s shows no %q: %q", row.name, row.want, tt.Texts())
		}
		// A tab with nothing but its empty state centres it in the panel.
		if row.project == nil {
			kittest.Centred(t, tt, text(row.want), ui.Rect{W: 560, H: 600})
		}
	}
	// While the session's checkpoints are read the tab shows a spinner.
	loading, pr = true, nil
	s.Tabs, s.Active = []Tab{{Kind: History}}, Tab{Kind: History}
	tt.Frame()
	tt.Frame()
	if _, spinner := tt.AccessByRole(ui.RoleProgress, text("git_view_history")); !spinner || tt.HasText(text("panel_history_empty")) {
		t.Errorf("loading History shows no spinner: %q", tt.Texts())
	}
}

func testOpenedAddsOnce(t *testing.T) {
	files, changes := Tab{Kind: Files}, Tab{Kind: Changes}
	tabs := Opened(Opened(nil, files), changes)
	if tabs = Opened(tabs, files); !slices.Equal(tabs, []Tab{files, changes}) {
		t.Errorf("tabs %v", tabs)
	}
}

func testClosedChoosesNext(t *testing.T) {
	files, changes, page := Tab{Kind: Files}, Tab{Kind: Changes}, Tab{Browser, "1"}
	tabs := []Tab{files, changes, page}
	if out, active := Closed(tabs, page, files); active != files || len(out) != 2 {
		t.Errorf("closing another kept %v, %v", active, out)
	}
	if _, active := Closed(tabs, changes, changes); active != files {
		t.Errorf("closing the active chose %v, want the first", active)
	}
	if out, active := Closed([]Tab{files}, files, files); len(out) != 0 || active != (Tab{}) {
		t.Errorf("closing the last left %v %v", out, active)
	}
}

// Each open item is its own tab: Files and Git once without a project,
// browser pages and terminals each new; a kind the panel no longer opens
// is not shown.
func testFlatTabs(t *testing.T) {
	s := New(preview.Load().Workspace)
	s.Open(Files)
	s.Open(Changes)
	s.Show(Tab{"file", "a.go"})
	s.Open(Browser)
	s.New(Browser)
	s.Open(Files)
	if len(s.Tabs) != 4 || s.Active != (Tab{Kind: Files}) {
		t.Fatalf("tabs %v, active %v", s.Tabs, s.Active)
	}
	if s.Tabs[2] == s.Tabs[3] || s.Tabs[3].Kind != Browser {
		t.Errorf("a new browser page reused a tab: %v", s.Tabs)
	}
}

// The file a project shows first is its own tree's README: the demo's
// Files tab opens on a file the demo has.
func TestFirstFileIsTheWorkspaceReadme(t *testing.T) {
	w := preview.Demo("en", "local").Workspace
	pr := NewProjects(w).Of("harbor-api")
	if got := pr.Files.File(); got != "harbor-api/README.md" || findFile(w.Files, got) == nil {
		t.Fatalf("the demo's first file is %q", got)
	}
	if got := NewProjects(&preview.Workspace{}).Of("empty").Files.File(); got != "" {
		t.Fatalf("a workspace without a tree shows %q first", got)
	}
}

// Each open file and diff is its own tab, the same items as the page's
// strip; another project's items replace them.
func testItemTabs(t *testing.T) {
	w := preview.Load().Workspace
	ps := NewProjects(w)
	one := ps.Of("one")
	s := New(w)
	s.Sync(one)
	s.Open(Files)
	s.Show(Tab{Files, "a.go"})
	s.Show(Tab{Files, "b.go"})
	s.Open(Changes)
	s.Sync(one)
	readme, diff := Tab{Files, "sailry-go/README.md"}, Tab{Changes, w.Changes[0].Path}
	if want := []Tab{readme, {Files, "a.go"}, {Files, "b.go"}, diff}; !slices.Equal(s.Tabs, want) || s.Active != diff {
		t.Fatalf("tabs %v, active %v", s.Tabs, s.Active)
	}
	if !slices.Equal(one.Files.Open(), []string{"sailry-go/README.md", "a.go", "b.go"}) || one.Files.File() != "b.go" {
		t.Fatalf("the project's files %v, shown %q", one.Files.Open(), one.Files.File())
	}
	// Closing a tab closes the item; one closed on the page drops its tab.
	s.Close(Tab{Files, "a.go"})
	one.Files.Close("b.go")
	s.Sync(one)
	if want := []Tab{readme, diff}; !slices.Equal(s.Tabs, want) || one.Files.File() != "sailry-go/README.md" {
		t.Fatalf("after closing: tabs %v, shown %q", s.Tabs, one.Files.File())
	}
	// Another project swaps the item tabs for its own.
	two := ps.Of("two")
	two.Files.Show("c.go")
	s.Sync(two)
	if want := []Tab{{Files, "c.go"}}; !slices.Equal(s.Tabs, want) || s.Active != want[0] {
		t.Fatalf("project two: tabs %v, active %v", s.Tabs, s.Active)
	}
}

func testBrowserHistory(t *testing.T) {
	b := &page{at: -1}
	if b.Go("  ") || b.at != -1 {
		t.Fatal("an empty address went")
	}
	b.Go("sailry.dev")
	b.Go("https://example.com/a")
	b.Step(-1)
	if b.address != "https://sailry.dev" {
		t.Fatalf("back to %q", b.address)
	}
	b.Go("example.org")
	b.Step(1)
	if len(b.history) != 2 || b.address != "https://example.org" {
		t.Fatalf("history %v at %q", b.history, b.address)
	}
}
