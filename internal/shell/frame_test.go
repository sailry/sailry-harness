package shell

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/panel"
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
	if a.Panel.Active != panel.Files || !tt.HasText("A native desktop Agent workspace.") {
		t.Fatalf("⌘P did not show the files: %q", tt.Texts())
	}
	snapshot(t, tt, "panel-files")
	tt.Key(ui.Ctrl|ui.Shift, ui.KeyG)
	tt.Frame()
	if !tt.HasText("workspace: three flat panels") {
		t.Fatalf("no diff: %q", tt.Texts())
	}
	click(t, tt, i18n.In("en", "hunks_accept"))
	tt.Frame()
	if !tt.HasText(i18n.In("en", "hunks_state_accepted")) {
		t.Errorf("the hunk was not accepted: %q", tt.Texts())
	}
	snapshot(t, tt, "panel-changes")
	for _, k := range []panel.Kind{panel.History, panel.Documents, panel.Browser, panel.Terminal} {
		a.Panel.Open(k)
	}
	tt.Frame()
	if !tt.HasText(i18n.In("en", "resource_terminal_preview")) {
		t.Errorf("no terminal placeholder: %q", tt.Texts())
	}
	if _, ok := tt.Access(i18n.In("en", "navigation_tabs")); !ok {
		t.Error("six tabs show no overflow menu")
	}
	snapshot(t, tt, "panel-terminal")
	// ⌘W closes the active tab, then the others, then the panel.
	for range 6 {
		tt.Key(ui.Cmd, ui.KeyW)
		tt.Frame()
	}
	if len(a.Panel.Tabs) != 0 || a.PanelOpen {
		t.Errorf("⌘W left %v, open %v", a.Panel.Tabs, a.PanelOpen)
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
	if b.NavOpen || b.NavWidth != 300 || !b.PanelOpen || b.Panel.Active != panel.Changes {
		t.Errorf("preferences did not come back: %+v", b.Prefs())
	}
	if got := b.Sidebar.Collapsed(); len(got) != 1 || got[0] != "notes" {
		t.Errorf("collapsed projects %v", got)
	}
}

func TestSearchOpensConversation(t *testing.T) {
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
}

func TestParseSource(t *testing.T) {
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
}

func TestSearchRules(t *testing.T) {
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

func TestMoveFeature(t *testing.T) {
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
}

// TestMoreDragShowsInsertionLine drags Activity over the lower half of
// Files in More: the line shows in the gap between Files and Git, not at
// the list's ends, and the drop puts Activity there.
func TestMoreDragShowsInsertionLine(t *testing.T) {
	a, tt := newTester(t)
	// Off the rail, so their names are the popover's rows alone.
	for _, id := range []string{"page:activity", "page:files", "page:git"} {
		a.unpinned[id] = true
	}
	a.more = true
	tt.Frame()
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
