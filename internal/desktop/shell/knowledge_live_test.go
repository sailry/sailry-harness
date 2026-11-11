package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

// testLiveKnowledge covers the viewer over pages a controller reads: the
// notice is marked read and hands over its citation, an invalid link never
// reaches the hook, and Previous and Next are disabled at the ends and while
// a page loads.
func testLiveKnowledge(t *testing.T) {
	a, tt := newTester(t)
	var opened []Source
	var turns []bool
	var read []string
	a.Hooks.ReadNotice = func(id string) { read = append(read, id) }
	page := &Reading{Title: "Frame notes", Host: "Studio", Path: "docs/frame.md", Position: i18n.In("en", "knowledge_page", "page", 2), Text: "first page", First: true}
	page.Turn = func(forward bool) { turns = append(turns, forward) }
	link := ""
	a.Hooks.BeforeFrame = func(c *ui.Context) {
		if link != "" {
			a.openKnowledge(c, "", link)
			link = ""
		}
	}
	a.Hooks.OpenKnowledge = func(_ *ui.Context, _ string, source Source) {
		opened = append(opened, source)
		a.ShowKnowledge(page)
	}
	link = "sailry-knowledge://docs/frame?revision=3#L9"
	tt.Frame()
	tt.Frame()
	if len(opened) != 0 || a.Reading() != nil || !tt.HasText(i18n.In("en", "knowledge_citation_invalid")) {
		t.Fatalf("invalid link: opened %v, texts %q", opened, tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "notifications"))
	tt.Frame()
	click(t, tt, "Frame motion · "+i18n.In("en", "local_host_name")+" · "+i18n.In("en", "notifications")+" · docs/shell/frame.md")
	tt.Frame()
	if len(opened) != 1 || opened[0] != (Source{Collection: "docs", Chunk: "frame-motion", Revision: "3"}) || a.Reading() != page {
		t.Fatalf("notice opened %v", opened)
	}
	// ACT-008: the click marks the notice read and closes the popover.
	if len(read) != 1 || read[0] != "n5" || a.activity {
		t.Fatalf("the notice click read %v, popover open %v", read, a.activity)
	}
	for _, text := range []string{"Frame notes", "Studio · docs/frame.md · Page 2", "first page"} {
		if !tt.HasText(text) {
			t.Fatalf("viewer lacks %q: %q", text, tt.Texts())
		}
	}
	state := func() (previous, next bool) {
		p, _ := tt.AccessByRole(ui.RoleButton, i18n.In("en", "knowledge_previous"))
		n, _ := tt.AccessByRole(ui.RoleButton, i18n.In("en", "knowledge_next"))
		return !p.Disabled, !n.Disabled
	}
	if previous, next := state(); previous || !next {
		t.Fatalf("first page: previous %v, next %v", previous, next)
	}
	click(t, tt, i18n.In("en", "knowledge_next"))
	tt.Frame()
	page.Pending = true
	tt.Frame()
	if previous, next := state(); previous || next {
		t.Fatalf("loading: previous %v, next %v", previous, next)
	}
	page.Pending, page.First, page.Last, page.Text = false, false, true, "last page"
	tt.Frame()
	if previous, next := state(); !previous || next || !tt.HasText("last page") {
		t.Fatalf("last page: previous %v, next %v", previous, next)
	}
	click(t, tt, i18n.In("en", "knowledge_previous"))
	tt.Frame()
	if len(turns) != 2 || !turns[0] || turns[1] {
		t.Fatalf("turns %v", turns)
	}
	click(t, tt, i18n.In("en", "knowledge_close"))
	tt.Frame()
	if a.Reading() != nil {
		t.Fatal("Close left the live page")
	}
}
