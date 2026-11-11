package shell

import (
	"maps"
	"os"
	"path/filepath"
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
	// A tab's close button closes it, the shown one and one behind it: the
	// button is the 20 DIP box at the tab's end, showing as it is hovered.
	closeTab := func(title string) {
		t.Helper()
		for _, info := range tt.AccessAll() {
			if info.Role != ui.RoleButton || info.Name != title || info.Bounds.H != 28 || info.Bounds.Y > HeaderHeight+40 || info.Bounds.X < 1280-a.PanelWidth {
				continue
			}
			x, y := info.Bounds.X+info.Bounds.W-14, info.Bounds.Y+info.Bounds.H/2
			tt.Move(x, y)
			tt.Frame()
			tt.ClickAt(x, y)
			tt.Frame()
			tt.Frame()
			return
		}
		t.Fatalf("no tab %q in the strip: %q", title, tt.Texts())
	}
	terminal := a.Panel.Active
	closeTab(i18n.In("en", "terminal"))
	if len(a.Panel.Tabs) != 7 || slices.Contains(a.Panel.Tabs, terminal) || a.Panel.Active != a.Panel.Tabs[0] {
		t.Fatalf("the close button left the shown tab: %v, active %v", a.Panel.Tabs, a.Panel.Active)
	}
	closeTab("go.mod")
	if len(a.Panel.Tabs) != 6 || slices.Contains(a.Panel.Tabs, panel.Tab{Kind: panel.Files, ID: "sailry-go/go.mod"}) || a.Panel.Active != a.Panel.Tabs[0] {
		t.Fatalf("the close button left a tab behind the shown one: %v, active %v", a.Panel.Tabs, a.Panel.Active)
	}
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
func TestPanelLauncherCentred(t *testing.T) {
	for _, lang := range []string{"en", "zh-CN"} {
		_, tt := newTester(t)
		tt.SetLocale(lang)
		tt.Key(ui.Cmd|ui.Alt, ui.KeyB)
		tt.Advance(time.Second)
		tt.Frame()
		// The launcher has no title: its rows are centred between the header
		// and the window's bottom edge.
		// The first and last rows have labels no other control carries.
		head, ok := tt.AccessByRole(ui.RoleButton, i18n.In(lang, panel.Resources[0].Label))
		tail, found := tt.AccessByRole(ui.RoleButton, i18n.In(lang, panel.Resources[len(panel.Resources)-1].Label))
		if !ok || !found || head.Bounds.X < 640 || tail.Bounds.X != head.Bounds.X {
			t.Fatalf("%s: launcher rows %v %v: %+v %+v", lang, ok, found, head.Bounds, tail.Bounds)
		}
		first, last := head.Bounds.Y, tail.Bounds.Y+tail.Bounds.H
		above, below := first-HeaderHeight, 820-last
		if above < 80 || above-below > 2 || below-above > 2 {
			t.Fatalf("%s: the rows are not vertically centred: %v above, %v below", lang, above, below)
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

// sessionsShown reports whether the session sidebar shows: its New session
// button and the "+" of its Chats group, in the column beside the rail.
func sessionsShown(tt *ui.Tester) bool {
	var button, plus bool
	for _, info := range tt.AccessAll() {
		if info.Role != ui.RoleButton || info.Bounds.X < RailWidth || info.Bounds.X >= RailWidth+NavMax {
			continue
		}
		button = button || info.Name == i18n.In("en", "conversation")
		plus = plus || info.Name == i18n.In("en", "workspace_new_session")
	}
	return button && plus
}

// headerButton finds a button of the header's band by its name, apart from
// controls of the same name below it.
func headerButton(tt *ui.Tester, name string) (ui.AccessInfo, bool) {
	for _, info := range tt.AccessAll() {
		if b := info.Bounds; info.Role == ui.RoleButton && info.Name == name && b.W > 0 && b.Y+b.H <= HeaderHeight {
			return info, true
		}
	}
	return ui.AccessInfo{}, false
}

// Each page owns its side column: the sessions beside the conversation and
// the host and project pages opened from it, the sections beside Settings,
// none beside the other pages, whose header has no toggle and where ⌘B
// leaves the saved choice.
func testSideColumns(t *testing.T) {
	a, tt := newTester(t)
	toggle := i18n.In("en", "sidebar_toggle")
	pages := []struct {
		name             string
		open             func()
		sessions, column bool
	}{
		{"files", func() { a.Navigate(FilesPage) }, false, false},
		{"conversation", func() { a.Navigate(ConversationPage) }, true, true},
		{"git", func() { a.Navigate(GitPage) }, false, false},
		{"host", func() { a.openHost("remote", false) }, true, true},
		{"activity", func() { a.Navigate(ActivityPage) }, false, false},
		{"project", func() { a.OpenProject(a.Data.Projects[0], ProjectActions{}) }, true, true},
		{"market", func() { a.Navigate(MarketPage) }, false, false},
		{"settings", func() { a.Navigate(SettingsPage) }, false, true},
		{"notifications", func() { a.Navigate(NotificationsPage) }, false, false},
	}
	for _, p := range pages {
		p.open()
		// One frame: the destination shows settled, with no motion.
		tt.Frame()
		button, shown := headerButton(tt, toggle)
		if a.HasNavigation() != p.column || shown != p.column || a.NavShown() != p.column {
			t.Errorf("%s: side column %v, toggle %v, want %v", p.name, a.HasNavigation(), shown, p.column)
		}
		if got := sessionsShown(tt); got != p.sessions {
			t.Errorf("%s: session sidebar shown %v, want %v", p.name, got, p.sessions)
		}
		if shown && (button.Bounds.X < RailWidth+NavWidth+11 || button.Bounds.X > RailWidth+NavWidth+13) {
			t.Errorf("%s: the page starts at %v on its first frame, want past the open column", p.name, button.Bounds.X)
		}
		if !a.NavOpen {
			t.Fatalf("%s: the saved choice closed", p.name)
		}
		tt.Key(ui.Cmd, ui.KeyB)
		settle(tt)
		if a.NavOpen == p.column {
			t.Errorf("%s: ⌘B left the choice open %v with a side column %v", p.name, a.NavOpen, p.column)
		}
		if p.column {
			if sessionsShown(tt) || a.NavShown() {
				t.Errorf("%s: ⌘B kept the column", p.name)
			}
			button, _ = headerButton(tt, toggle)
			tt.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
			settle(tt)
			if !a.NavOpen || sessionsShown(tt) != p.sessions {
				t.Errorf("%s: the toggle did not bring the column back", p.name)
			}
		}
	}
	// A closed choice stays closed across a page without a column, and the
	// page's breadcrumb leads back to the conversation with the column open.
	a.Navigate(ConversationPage)
	a.NavOpen = false
	settle(tt)
	a.Navigate(FilesPage)
	tt.Frame()
	if a.NavOpen || a.Prefs().NavOpen || sessionsShown(tt) {
		t.Errorf("Files changed the closed choice: %v", a.NavOpen)
	}
	a.Navigate(ConversationPage)
	tt.Frame()
	if button, ok := headerButton(tt, toggle); !ok || a.NavOpen || sessionsShown(tt) || button.Bounds.X > RailWidth+13 {
		t.Errorf("the conversation came back with open %v, toggle %v", a.NavOpen, button.Bounds)
	}
	a.Navigate(FilesPage)
	tt.Frame()
	crumb := false
	for _, info := range tt.AccessAll() {
		if info.Name == "sailry-go" && info.Bounds.Y+info.Bounds.H <= HeaderHeight && info.Bounds.W > 0 {
			tt.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
			crumb = true
			break
		}
	}
	settle(tt)
	if !crumb || a.Page != ConversationPage || !a.NavOpen || !sessionsShown(tt) {
		t.Errorf("the project crumb (found %v) left page %d, open %v", crumb, a.Page, a.NavOpen)
	}
	// The saved state is one choice and one width, as before: a page
	// without a column saves and restores them untouched, and a file from
	// before pages owned their columns loads.
	a.NavWidth = 300
	a.Navigate(MarketPage)
	tt.Frame()
	dir := t.TempDir()
	if err := SavePrefs(dir, a.Prefs()); err != nil {
		t.Fatal(err)
	}
	b := New(preview.Load())
	b.SetPrefs(LoadPrefs(dir))
	if !b.NavOpen || b.NavWidth != 300 {
		t.Errorf("the choice did not come back from a page without a column: %+v", b.Prefs())
	}
	if err := os.WriteFile(filepath.Join(dir, PrefsFile), []byte(`{"navWidth":280,"navOpen":false,"panelWidth":420,"panelOpen":true}`), 0o644); err != nil {
		t.Fatal(err)
	}
	b.SetPrefs(LoadPrefs(dir))
	if b.NavOpen || b.NavWidth != 280 || !b.PanelOpen || b.PanelWidth != 420 {
		t.Errorf("an earlier file did not load: %+v", b.Prefs())
	}
	bt := ui.NewTesterWith(b.View, 1280, 820, opaque)
	bt.SetLocale("en")
	b.Navigate(GitPage)
	bt.Frame()
	if _, ok := headerButton(bt, toggle); ok || sessionsShown(bt) {
		t.Error("the restored app shows a column beside Git")
	}
	b.Navigate(ConversationPage)
	b.toggleNav()
	settle(bt)
	if _, ok := headerButton(bt, toggle); !ok || !sessionsShown(bt) {
		t.Error("the restored app has no sessions beside the conversation")
	}
}

func TestSidebarToggleAndPrefs(t *testing.T) {
	t.Run("side columns", testSideColumns)
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
	t.Run("icon buttons", testIconButtons)
}

// The header's icon buttons are one Kit button: 28 square, centred in the
// band, 8 from their neighbours, with one hover fill; the rail's are 36
// square, 8 apart. A details toggle that cannot act is disabled.
func testIconButtons(t *testing.T) {
	a, tt := newTester(t)
	toggle, _ := headerButton(tt, i18n.In("en", "sidebar_toggle"))
	details, _ := headerButton(tt, i18n.In("en", "details"))
	t.Logf("header buttons: toggle %v, details %v", toggle.Bounds, details.Bounds)
	for name, b := range map[string]ui.Rect{"toggle": toggle.Bounds, "details": details.Bounds} {
		if b.W != HeaderButton || b.H != HeaderButton || b.Y != (HeaderHeight-HeaderButton)/2 {
			t.Errorf("%s is %v, want %v square centred in the %v band", name, b, HeaderButton, HeaderHeight)
		}
	}
	if got := 1280 - (details.Bounds.X + details.Bounds.W); got != 12 {
		t.Errorf("details ends %v from the edge, want 12", got)
	}
	// Their neighbours are one gap away: the title after the toggle, the
	// preview tag before details.
	var trail, tag ui.Rect
	for _, info := range tt.AccessAll() {
		b := info.Bounds
		if b.Y+b.H > HeaderHeight || b.W == 0 {
			continue
		}
		if b.X >= toggle.Bounds.X+toggle.Bounds.W && (trail.W == 0 || b.X < trail.X) {
			trail = b
		}
		if end := b.X + b.W; end <= details.Bounds.X && end > tag.X+tag.W {
			tag = b
		}
	}
	if got := trail.X - (toggle.Bounds.X + toggle.Bounds.W); got != ButtonGap {
		t.Errorf("the title starts %v after the toggle, want %v", got, ButtonGap)
	}
	if got := details.Bounds.X - (tag.X + tag.W); got != ButtonGap {
		t.Errorf("details starts %v after the preview tag, want %v", got, ButtonGap)
	}
	// One hover fill: the same box, and so the same number of pixels, on
	// each, and nothing outside it.
	tt.Move(640, 400)
	tt.Frame()
	rest := tt.Image()
	fills := map[string]int{}
	for name, b := range map[string]ui.Rect{"toggle": toggle.Bounds, "details": details.Bounds} {
		tt.Move(b.X+b.W/2, b.Y+b.H/2)
		tt.Frame()
		hovered := tt.Image()
		fills[name] = changed(rest, hovered, b)
		around := ui.Rect{X: b.X - 6, Y: b.Y - 6, W: b.W + 12, H: b.H + 12}
		if outside := changed(rest, hovered, around) - fills[name]; outside != 0 {
			t.Errorf("%s: its hover changes %d pixels outside its box", name, outside)
		}
		if name == "details" {
			snapshot(t, tt, "header-hover")
		}
	}
	t.Logf("hover fills: %v", fills)
	scale := rest.Bounds().Dx() / 1280
	full := HeaderButton * HeaderButton * scale * scale
	if fills["toggle"] < full/2 || fills["details"] < full/2 {
		t.Errorf("hover fills %v do not cover the %d pixel boxes", fills, full)
	}
	if d := fills["toggle"] - fills["details"]; d > full/10 || d < -full/10 {
		t.Errorf("hover fills differ: %v", fills)
	}
	tt.Move(640, 400)
	// The host page's metrics toggle is the same button in the same place.
	a.openHost("remote", false)
	settle(tt)
	if metrics, ok := headerButton(tt, i18n.In("en", "details")); !ok || metrics.Bounds != details.Bounds {
		t.Errorf("the host page's toggle is %v, want %v", metrics.Bounds, details.Bounds)
	}
	a.Navigate(ConversationPage)
	settle(tt)
	// The rail: every button 36 square in one column, the pinned features
	// 8 apart from 8 under the band.
	var rail []ui.Rect
	for _, info := range tt.AccessAll() {
		if b := info.Bounds; info.Role == ui.RoleButton && b.X+b.W <= RailWidth && b.Y >= HeaderHeight {
			rail = append(rail, b)
		}
	}
	slices.SortFunc(rail, func(x, y ui.Rect) int { return int(x.Y - y.Y) })
	if len(rail) < 9 {
		t.Fatalf("the rail has %d buttons: %v", len(rail), rail)
	}
	for i, b := range rail {
		if b.W != RailButton || b.H != RailButton || b.X != (RailWidth-RailButton)/2 {
			t.Errorf("rail button %d is %v, want %v square in the middle of the rail", i, b, RailButton)
		}
	}
	for i := range 5 {
		if want := float32(HeaderHeight + 8 + i*(RailButton+ButtonGap)); rail[i].Y != want {
			t.Errorf("rail button %d is at %v, want %v", i, rail[i].Y, want)
		}
	}
	// Too narrow for the panel beside the conversation, the details toggle
	// cannot act: it is disabled, not a dimmed button that takes clicks.
	narrow := New(preview.Load())
	nt := ui.NewTesterWith(narrow.View, RailWidth+NavWidth+MainMin+PanelMin-40, 820, opaque)
	nt.SetLocale("en")
	nt.Frame()
	info, ok := headerButton(nt, i18n.In("en", "details"))
	snapshot(t, nt, "details-disabled")
	if !ok || !info.Disabled {
		t.Fatalf("the details toggle without room is %+v", info)
	}
	nt.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
	nt.Frame()
	if narrow.PanelOpen {
		t.Error("a disabled details toggle opened the panel")
	}
	narrow.NavOpen = false
	settle(nt)
	if info, _ := headerButton(nt, i18n.In("en", "details")); info.Disabled {
		t.Error("the details toggle stays disabled with room for the panel")
	}
	clickButton(t, nt, i18n.In("en", "details"))
	settle(nt)
	if !narrow.PanelOpen {
		t.Error("the details toggle did not open the panel")
	}
}
