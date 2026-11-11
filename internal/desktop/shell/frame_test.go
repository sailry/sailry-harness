package shell

import (
	"maps"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestPanelOpensWithShortcutsAndTabs(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd|ui.Alt, ui.KeyB)
	tt.Advance(time.Second)
	tt.Frame()
	if !a.PanelOpen || !tt.HasText(i18n.In("en", "resource_browser")) {
		t.Fatalf("⌘⌥B did not open the launcher: %q", tt.Texts())
	}
	snapshot(t, tt, "panel-launcher")
	tt.Key(ui.Cmd, ui.KeyP)
	tt.Advance(time.Second)
	tt.Frame()
	if a.Panel.Active.Kind != panel.Files || !tt.HasText("README.md") {
		t.Fatalf("⌘P did not show the file tree: %q", tt.Texts())
	}
	// The tab shows the project's file beside the tree; choosing another
	// opens it in its own tab, the tree staying beside it.
	if !tt.HasText("A native desktop Agent workspace.") {
		t.Fatalf("the Files tab does not show the file: %q", tt.Texts())
	}
	clickTree(t, tt, "go.mod")
	tt.Frame()
	if len(a.Panel.Tabs) != 2 || a.Panel.Active != (panel.Tab{Kind: panel.Files, ID: "sailry-go/go.mod"}) || !codeHas(tt, "Code editor", "require github.com/egoist/mygo v0.8.3") {
		t.Fatalf("the file did not show in the Files tab: %v %q", a.Panel.Tabs, tt.Texts())
	}
	snapshot(t, tt, "panel-files")
	tt.Key(ui.Ctrl|ui.Shift, ui.KeyG)
	tt.Frame()
	clickTree(t, tt, "example.md")
	tt.Frame()
	if len(a.Panel.Tabs) != 4 || a.Panel.Active != (panel.Tab{Kind: panel.Changes, ID: "docs/example.md"}) ||
		!codeHas(tt, "New text", "search: command palette") {
		t.Fatalf("no diff tab: %v %q", a.Panel.Tabs, tt.Texts())
	}
	// Choosing a tab shows its item and its kind's side list, whose
	// selection stays the tree's own, as the files plugin's explorer.
	click(t, tt, "README.md")
	tt.Frame()
	if _, ok := tt.AccessByRole(ui.RoleTreeItem, "README.md"); !ok || !tt.HasText("A native desktop Agent workspace.") {
		t.Fatalf("the README tab does not show beside the tree: %q", tt.Texts())
	}
	snapshot(t, tt, "panel-changes")
	for _, k := range []panel.Kind{panel.History, panel.Documents, panel.Browser, panel.Terminal} {
		a.Panel.Open(k)
	}
	// Two files, two diffs and those four: one flat strip.
	if len(a.Panel.Tabs) != 8 {
		t.Fatalf("tabs %v", a.Panel.Tabs)
	}
	tt.Frame()
	if !tt.HasText(i18n.In("en", "workspace_terminal_preview")) {
		t.Errorf("no terminal placeholder: %q", tt.Texts())
	}
	if _, ok := tt.Access(i18n.In("en", "navigation_tabs")); !ok {
		t.Error("eight tabs show no overflow menu")
	}
	// KEY-013: ⌘F from the conversation's input opens the conversation's
	// find bar.
	input, found := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	if !found {
		t.Fatalf("no composer input: %q", tt.Texts())
	}
	tt.ClickAt(input.Bounds.X+input.Bounds.W/2, input.Bounds.Y+input.Bounds.H/2)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyF)
	tt.Frame()
	if !a.Messages.SearchOpen() {
		t.Fatalf("⌘F outside the terminal opened no conversation find bar: %q", tt.Texts())
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if a.Messages.SearchOpen() {
		t.Fatal("Escape left the conversation's find bar open")
	}
	// ⌘F in the terminal opens its find bar, not the conversation's.
	click(t, tt, i18n.In("en", "workspace_terminal_preview"))
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyF)
	tt.Frame()
	if !tt.HasText(i18n.In("en", "terminal_search_placeholder")) || a.Messages.SearchOpen() {
		t.Errorf("no terminal find bar, or the conversation's opened: %q", tt.Texts())
	}
	snapshot(t, tt, "panel-terminal")
	// ⌘W in the panel closes the active tab, then the others, then the
	// panel.
	for range 10 {
		tt.Key(ui.Cmd, ui.KeyW)
		tt.Frame()
	}
	if len(a.Panel.Tabs) != 0 || a.PanelOpen {
		t.Errorf("⌘W left %v, open %v", a.Panel.Tabs, a.PanelOpen)
	}
}

// The launcher is titled "details" and lists the six resources in order
// from the top of the panel, each row opening its resource.
func TestPanelLauncherStartsAtTop(t *testing.T) {
	for _, lang := range []string{"en", "zh-CN"} {
		_, tt := newTester(t)
		tt.SetLocale(lang)
		tt.Key(ui.Cmd|ui.Alt, ui.KeyB)
		tt.Advance(time.Second)
		tt.Frame()
		title, ok := tt.Find(i18n.In(lang, "details"))
		if !ok {
			t.Fatalf("%s: no launcher title: %q", lang, tt.Texts())
		}
		top := title.Y
		for i, r := range panel.Resources {
			row, ok := tt.AccessByRole(ui.RoleButton, i18n.In(lang, r.Label))
			if !ok {
				t.Fatalf("%s: no launcher row %q", lang, r.Label)
			}
			if row.Bounds.Y <= top {
				t.Fatalf("%s: row %d %q at %v is not below %v", lang, i, r.Label, row.Bounds.Y, top)
			}
			top = row.Bounds.Y
		}
		// Six 40 DIP rows 8 apart from under the 40 DIP header: the last
		// row ends far above a vertically centred list's.
		if top > HeaderHeight+40+5*48+8 {
			t.Fatalf("%s: the rows do not start at the top: last row at %v", lang, top)
		}
	}
	for _, r := range panel.Resources {
		a, tt := newTester(t)
		tt.Key(ui.Cmd|ui.Alt, ui.KeyB)
		tt.Advance(time.Second)
		tt.Frame()
		row, _ := tt.AccessByRole(ui.RoleButton, i18n.In("en", r.Label))
		tt.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
		tt.Frame()
		if a.Panel.Active.Kind != r.Kind {
			t.Errorf("the %q row opened %v", r.Label, a.Panel.Active)
		}
	}
}

func TestSidebarToggleAndPrefs(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeyB)
	tt.Advance(time.Second)
	tt.Frame()
	if a.NavOpen || tt.HasText("Show more") {
		t.Fatalf("⌘B did not close the sidebar")
	}
	snapshot(t, tt, "sidebar-closed")
	a.NavWidth, a.PanelOpen = 300, true
	a.Panel.Open(panel.Changes)
	a.Sidebar.SetCollapsed([]string{"notes"})
	dir := t.TempDir()
	if err := SavePrefs(dir, a.Prefs()); err != nil {
		t.Fatal(err)
	}
	b := New(a.Data)
	b.SetPrefs(LoadPrefs(dir))
	if b.NavOpen || b.NavWidth != 300 || !b.PanelOpen || b.Panel.Active.Kind != panel.Changes {
		t.Errorf("preferences did not come back: %+v", b.Prefs())
	}
	if got := b.Sidebar.Collapsed(); len(got) != 1 || got[0] != "notes" {
		t.Errorf("collapsed projects %v", got)
	}
	// A file's preview and a subagent's session are their conversation's:
	// their tabs are not kept, neither from a file that has them.
	t.Run("conversation tabs", func(t *testing.T) {
		a := New(preview.Load())
		a.Panel.Open(panel.Changes)
		a.Panel.ShowChild("child", "Child")
		a.Panel.ShowArtifact(conversation.File{ID: "file", Path: "notes.md"})
		p := a.Prefs()
		if want := []panel.Tab{{Kind: panel.Changes}}; !slices.Equal(p.PanelTabs, want) || p.PanelTab != want[0] {
			t.Errorf("saved tabs %v active %v, want %v", p.PanelTabs, p.PanelTab, want)
		}
		p.PanelTabs = append(p.PanelTabs, panel.Tab{Kind: panel.Artifact, ID: "file"}, panel.Tab{Kind: panel.Subagent, ID: "child"})
		p.PanelTab = panel.Tab{Kind: panel.Artifact, ID: "file"}
		b := New(preview.Load())
		b.SetPrefs(p)
		if want := []panel.Tab{{Kind: panel.Changes}}; !slices.Equal(b.Panel.Tabs, want) || b.Panel.Active != want[0] {
			t.Errorf("restored tabs %v active %v, want %v", b.Panel.Tabs, b.Panel.Active, want)
		}
	})
	// The file is one for the preview and every profile. The preview's rail
	// comes back from it, merged with the features of now; a profile's rail
	// is its own preferences', which the file neither replaces nor loses.
	t.Run("rail", func(t *testing.T) {
		a := New(preview.Load())
		a.features = MoveFeature(a.features, "page:git", "page:activity", false)
		a.unpinned["page:files"] = true
		saved := a.Prefs()
		if err := SavePrefs(dir, saved); err != nil {
			t.Fatal(err)
		}
		b := New(preview.Load())
		b.SetPrefs(LoadPrefs(dir))
		if !slices.Equal(b.features, a.features) || !b.unpinned["page:files"] {
			t.Errorf("preview rail %v unpinned %v, want %v", b.features, b.unpinned, a.features)
		}
		stale := saved
		stale.Features = []string{"page:git", "page:gone", "page:conversation"}
		b = New(preview.Load())
		b.SetPrefs(stale)
		if want := []string{"page:conversation", "page:git", "page:gone", "page:activity", "page:files", "page:settings_plugins"}; !slices.Equal(b.features, want) {
			t.Errorf("stale order merged to %v, want %v", b.features, want)
		}

		live := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
		live.SetFeaturePreferences([]string{"page:conversation", "page:files", "page:activity", "page:git", "page:settings_plugins"}, map[string]bool{"page:git": false})
		order, pins := live.FeaturePreferences()
		live.SetPrefs(LoadPrefs(dir))
		if after, afterPins := live.FeaturePreferences(); !slices.Equal(after, order) || !maps.Equal(afterPins, pins) {
			t.Errorf("the UI state file changed a profile's rail: %v %v, want %v %v", after, afterPins, order, pins)
		}
		if p := live.Prefs(); !slices.Equal(p.Features, saved.Features) || !slices.Equal(p.Unpinned, saved.Unpinned) {
			t.Errorf("a live window rewrote the preview's rail: %v %v, want %v %v", p.Features, p.Unpinned, saved.Features, saved.Unpinned)
		}
	})
}

// TestSearch groups the cases of one behavior; each keeps its own fixture.
func TestSearch(t *testing.T) {
	t.Run("SearchOpensConversation", testSearchOpensConversation)
	t.Run("SearchFolderKeys", testSearchFolderKeys)
	t.Run("SearchRules", testSearchRules)
}

func testSearchOpensConversation(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeyK)
	tt.Frame()
	tt.Frame()
	tt.Key(0, ui.KeyEnter) // Chats, the first folder
	tt.Frame()
	tt.Type("weeknight")
	tt.Frame()
	snapshot(t, tt, "search")
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if a.Sidebar.Selected != "chat/recipe" || a.search.open {
		t.Errorf("search chose %q, open %v", a.Sidebar.Selected, a.search.open)
	}
	// The search opens anew: not in the folder and query it closed with.
	tt.Key(ui.Cmd, ui.KeyK)
	tt.Frame()
	if !a.search.open || a.search.folder != "" || a.search.query != "" {
		t.Errorf("reopened search: %+v", a.search)
	}
}

// A folder of the search has no back row: Backspace on an empty query
// goes back to the root, and Escape clears the query, then goes back,
// then closes.
func testSearchFolderKeys(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeyK)
	tt.Frame()
	tt.Frame()
	tt.Key(0, ui.KeyEnter) // Chats, the first folder
	tt.Frame()
	if a.search.folder == "" {
		t.Fatal("Enter opened no folder")
	}
	if tt.HasText(i18n.In("en", "reference_back")) {
		t.Error("the folder shows a back row")
	}
	snapshot(t, tt, "search-folder")
	// Backspace edits a query and goes back only once it is empty.
	tt.Type("w")
	tt.Frame()
	tt.Key(0, ui.KeyBackspace)
	tt.Frame()
	if a.search.query != "" || a.search.folder == "" {
		t.Fatalf("Backspace on a query left %q in %q", a.search.query, a.search.folder)
	}
	tt.Key(0, ui.KeyBackspace)
	tt.Frame()
	if a.search.folder != "" || !a.search.open {
		t.Fatalf("Backspace on an empty query left folder %q, open %v", a.search.folder, a.search.open)
	}
	// Escape: the query, then the folder, then the search.
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	tt.Type("w")
	tt.Frame()
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if a.search.query != "" || a.search.folder == "" || !a.search.open {
		t.Fatalf("first Escape: query %q, folder %q, open %v", a.search.query, a.search.folder, a.search.open)
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if a.search.folder != "" || !a.search.open {
		t.Fatalf("second Escape: folder %q, open %v", a.search.folder, a.search.open)
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if a.search.open {
		t.Error("third Escape left the search open")
	}
}

func TestActivityCentreReadsAndOpens(t *testing.T) {
	a, tt := newTester(t)
	clickButton(t, tt, i18n.In("en", "notifications"))
	tt.Frame()
	snapshot(t, tt, "activity")
	click(t, tt, "Flaky integration test · "+i18n.In("en", "remote_host_name")+" · "+i18n.In("en", "activity_input")+" · Which database should the test use?")
	tt.Frame()
	if a.Sidebar.Selected != "api/tests" {
		t.Errorf("the notice opened %q", a.Sidebar.Selected)
	}
	// A knowledge notice opens the source viewer, which pages.
	clickButton(t, tt, i18n.In("en", "notifications"))
	tt.Frame()
	click(t, tt, "Frame motion · "+i18n.In("en", "local_host_name")+" · "+i18n.In("en", "notifications")+" · docs/shell/frame.md")
	tt.Frame()
	if !tt.HasText(i18n.In("en", "knowledge_lines", "start", 12, "end", 30)) {
		t.Fatalf("no knowledge viewer: %q", tt.Texts())
	}
	snapshot(t, tt, "knowledge")
	click(t, tt, i18n.In("en", "knowledge_next"))
	tt.Frame()
	if a.reading.page != 1 {
		t.Error("Next did not page")
	}
	t.Run("live knowledge viewer", testLiveKnowledge)
}

// The shell's pure rules: a knowledge link's parts, and a feature's move in
// the rail's order.
func TestShellRules(t *testing.T) {
	t.Run("ParseSource", func(t *testing.T) {
		if s, ok := ParseSource("sailry-knowledge://docs/frame?revision=3"); !ok || s.Collection != "docs" || s.Chunk != "frame" || s.Revision != "3" {
			t.Errorf("valid link: %+v %v", s, ok)
		}
		for _, bad := range []string{
			"sailry-knowledge://docs/frame?revision=3&path=x",
			"sailry-knowledge://docs/frame?revision=3#L9",
			"sailry-knowledge://docs/frame/more?revision=3",
			"sailry-knowledge://docs/frame?revision=3&revision=4",
			"https://docs/frame?revision=3",
			"sailry-knowledge://user@docs/frame?revision=3",
			"sailry-knowledge://docs:80/frame?revision=3",
		} {
			if _, ok := ParseSource(bad); ok {
				t.Errorf("%q passed", bad)
			}
		}
	})
	t.Run("MoveFeature", func(t *testing.T) {
		order := []string{"page:conversation", "page:activity", "page:files", "page:git", "page:settings_plugins"}
		got := MoveFeature(order, "page:git", "page:activity", false)
		if got[1] != "page:git" {
			t.Errorf("move: %v", got)
		}
		if got := MoveFeature(order, "page:activity", "page:settings_plugins", true); got[len(got)-1] != "page:settings_plugins" {
			t.Errorf("something went after Market: %v", got)
		}
		if got := MoveFeature(order, "page:conversation", "page:git", true); got[0] != "page:conversation" {
			t.Errorf("Conversation moved: %v", got)
		}
	})
}

func testSearchRules(t *testing.T) {
	groups := []Group{{Choices: []Choice{{ID: "a", Label: "Alpha"}, {ID: "b", Label: "Beta", Keywords: []string{"billing-api"}}}}, {Heading: "x", Choices: []Choice{{ID: "c", Label: "Gamma"}}}}
	if got := Filter(groups, "BILL"); len(got) != 1 || len(got[0].Choices) != 1 || got[0].Choices[0].ID != "b" {
		t.Errorf("keyword match: %+v", got)
	}
	if got := Filter(groups, ""); len(got) != 2 {
		t.Errorf("empty query: %+v", got)
	}
	for _, c := range []struct {
		n, sel int
		rev    bool
		want   int
	}{{3, 2, false, 0}, {3, 0, true, 2}, {3, -1, false, 0}, {3, -1, true, 2}, {0, 0, false, -1}} {
		if got := Next(c.n, c.sel, c.rev); got != c.want {
			t.Errorf("Next(%d, %d, %v) = %d", c.n, c.sel, c.rev, got)
		}
	}
}

// TestMoreDragShowsInsertionLine drags Activity over the lower half of
// Files in More: the line shows in the gap between Files and Git, not at
// the list's ends, and the drop puts Activity there.
func TestMoreDragShowsInsertionLine(t *testing.T) {
	a, tt := newTester(t)
	// Off the rail, so their names are the popover's rows alone.
	for _, id := range []string{"page:activity", "page:files", "page:git", "page:settings_plugins"} {
		a.unpinned[id] = true
	}
	a.more = true
	tt.Frame()
	tt.Advance(time.Second) // the popover's entrance
	tt.Frame()
	row := func(key string) ui.Rect {
		r, ok := tt.Find(i18n.In("en", key))
		if !ok {
			t.Fatalf("no %s row: %q", key, tt.Texts())
		}
		return r
	}
	activity, files, git, market := row("activity"), row("files"), row("git"), row("market_title")
	center := func(r ui.Rect) float32 { return r.Y + r.H/2 }
	gap := func(upper, lower ui.Rect) float32 { return (upper.Y + upper.H + lower.Y) / 2 }
	x := files.X + 10 // over the row's start, left of the dragged copy
	dark := func(y float32) bool {
		img := tt.Image()
		s := float32(img.Bounds().Dx()) / 1280
		c := img.RGBAAt(int(x*s), int(y*s))
		return int(c.R)+int(c.G)+int(c.B) < 3*200
	}
	if dark(gap(files, git)) {
		t.Fatal("a line shows before any drag")
	}
	px, py := activity.X+12, center(activity)
	tt.Press(px, py)
	tt.Frame()
	// Down to the lower half of Files, then right, so the copy following
	// the pointer leaves the row's start uncovered.
	for _, step := range [][2]float32{{px, py + 8}, {px, center(files) + 10}, {px + 160, center(files) + 10}} {
		tt.Move(step[0], step[1])
		tt.Frame()
	}
	tt.Frame()
	snapshot(t, tt, "more-drag")
	if !dark(gap(files, git)) {
		t.Error("no line between Files and Git")
	}
	if dark(gap(activity, files)) || dark(gap(git, market)) {
		t.Error("a line shows away from the pointer")
	}
	tt.Release(px+160, center(files)+10)
	tt.Frame()
	tt.Frame()
	want := []string{"page:conversation", "page:files", "page:activity", "page:git", "page:settings_plugins"}
	if !slices.Equal(a.features, want) {
		t.Errorf("order %v, want %v", a.features, want)
	}
}

// Collapsed, the page's header starts past the traffic lights, and goes
// back to the edge in full screen, where MyGo reports none.
func TestHeaderClearsTrafficLights(t *testing.T) {
	a, tt := newTester(t)
	tt.SetTitleBar(ui.TitleBar{Height: 28, Left: 70})
	a.NavOpen = false
	for range 3 {
		tt.Advance(time.Second)
		tt.Frame()
	}
	b, ok := tt.Access(i18n.In("en", "sidebar_toggle"))
	if !ok || b.Bounds.X < trafficLights {
		t.Fatalf("the sidebar toggle starts at %v under the lights", b.Bounds)
	}
	tt.SetTitleBar(ui.TitleBar{})
	tt.Frame()
	if b, _ := tt.Access(i18n.In("en", "sidebar_toggle")); b.Bounds.X >= trafficLights {
		t.Errorf("in full screen the toggle still starts at %v", b.Bounds.X)
	}
}
