package shell

import (
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The strip groups sessions by lane, folds a full group into +N and opens
// a hidden session from its popover.
func TestHeaderStrip(t *testing.T) {
	a, tt := newTester(t)
	// Four finished unread: two show, then +2.
	a.Data.Find("sailry/release").Status = preview.Unread
	tt.Frame()
	for _, name := range []string{
		"Tester coverage for the composer · Needs attention",
		"Invoice PDF rendering · Running",
		"Port the neutral theme tokens · Completed",
		"Windows release lint fixes · Completed",
	} {
		if _, ok := tt.AccessByRole(ui.RoleButton, name); !ok {
			t.Errorf("no strip button %q", name)
		}
	}
	if _, ok := tt.AccessByRole(ui.RoleButton, "Stabilize MCP warm-up tests · Completed"); ok {
		t.Error("a third finished session shows instead of +N")
	}
	snapshot(t, tt, "strip")
	clickButton(t, tt, "Completed · +2")
	tt.Frame()
	snapshot(t, tt, "strip-more")
	clickButton(t, tt, "Translate release notes · Completed")
	tt.Frame()
	if a.Sidebar.Selected != "chat/translate" || a.Page != ConversationPage {
		t.Fatalf("selected %q on %v", a.Sidebar.Selected, a.Page)
	}
	if _, ok := tt.AccessByRole(ui.RoleButton, "Stabilize MCP warm-up tests · Completed"); ok {
		t.Error("the popover stays open after a row opens its session")
	}
	t.Run("ends the main region before Find", testStripPlace)
	t.Run("folds before it covers the title", testStripFolds)
	t.Run("asks for no frame at rest", testStripAtRest)
}

// stripButtons are the band's activity strip: its sessions' buttons and its
// groups' +N, with the box from the first to the last of them.
func stripButtons(tt *ui.Tester) (sessions, more int, box ui.Rect) {
	for _, info := range tt.AccessAll() {
		b := info.Bounds
		if info.Role != ui.RoleButton || b.W == 0 || b.Y+b.H > HeaderHeight {
			continue
		}
		for _, lane := range []string{"activity_waiting", "activity_running", "activity_completed"} {
			label := i18n.In("en", lane)
			switch {
			case strings.HasSuffix(info.Name, " · "+label):
				sessions++
			case strings.HasPrefix(info.Name, label+" · +"):
				more++
			default:
				continue
			}
			if box.W == 0 {
				box = b
			}
			end := max(box.X+box.W, b.X+b.W)
			box.X = min(box.X, b.X)
			box.W = end - box.X
		}
	}
	return sessions, more, box
}

// The strip is the main region's, directly left of Find and the overview,
// with the panel closed and open; the far-right cluster is the fold toggle
// alone. A page without those actions has the strip at the region's end.
func testStripPlace(t *testing.T) {
	a, tt := newTester(t)
	find, overview, details := i18n.In("en", "chat_search"), i18n.In("en", "chat_assets_title"), i18n.In("en", "details")
	end := func(r ui.Rect) float32 { return r.X + r.W }
	for _, open := range []bool{false, true} {
		a.PanelOpen = open
		still(tt)
		_, _, box := stripButtons(tt)
		f, found := headerButton(tt, find)
		o, shown := headerButton(tt, overview)
		fold, ok := headerButton(tt, details)
		if box.W == 0 || !found || !shown || !ok {
			t.Fatalf("open %v: strip %v, Find %v, overview %v, fold toggle %v", open, box, found, shown, ok)
		}
		if end(box) != f.Bounds.X-ButtonGap || o.Bounds.X != end(f.Bounds)+ButtonGap {
			t.Errorf("open %v: the band is not strip %v, Find %v, overview %v", open, box, f.Bounds, o.Bounds)
		}
		if open && end(o.Bounds) > 1280-PanelWidth {
			t.Errorf("the overview %v is over the panel, which starts at %v", o.Bounds, 1280-PanelWidth)
		}
		// The strip's box is all of it: none is left beside the toggle,
		// which stays where it was.
		if fold.Bounds.X != 1280-HeaderInsets(ui.TitleBar{})-HeaderButton {
			t.Errorf("open %v: the fold toggle is at %v", open, fold.Bounds)
		}
	}
	// A conversation without a turn has no Find or overview: the strip ends
	// the main region, before the page's tag and the fold toggle.
	a.PanelOpen = false
	a.Sidebar.NewChat()
	still(tt)
	if _, ok := headerButton(tt, find); ok {
		t.Fatal("a conversation without a turn has Find")
	}
	_, _, box := stripButtons(tt)
	fold, _ := headerButton(tt, details)
	tag, ok := tt.Find(i18n.In("en", "preview"))
	if box.W == 0 || !ok || end(box) != tag.X-ButtonGap || end(tag) != fold.Bounds.X-ButtonGap {
		t.Errorf("without actions the band is not strip %v, tag %v, fold toggle %v", box, tag, fold.Bounds)
	}
}

// Beside an open panel the main region is short. The title shortens first;
// then the groups fold, down to a +N each, rather than reach over the title:
// the strip still ends one gap before Find and starts after the title.
func testStripFolds(t *testing.T) {
	a, tt := newTester(t)
	a.Data.Find("sailry/release").Status = preview.Unread
	still(tt)
	wide, _, _ := stripButtons(tt)
	a.PanelOpen = true
	still(tt)
	sessions, more, box := stripButtons(tt)
	if sessions >= wide || more == 0 {
		t.Fatalf("beside the panel the strip shows %d sessions (%d before) and %d +N", sessions, wide, more)
	}
	// Four finished unread fold into their +N alone.
	if _, ok := headerButton(tt, i18n.In("en", "activity_completed")+" · +4"); !ok {
		t.Fatalf("the finished sessions did not fold into +4: %d sessions, %d +N", sessions, more)
	}
	find, ok := headerButton(tt, i18n.In("en", "chat_search"))
	if !ok || box.X+box.W != find.Bounds.X-ButtonGap {
		t.Errorf("the folded strip %v does not end one gap before Find %v", box, find.Bounds)
	}
	toggle, ok := headerButton(tt, i18n.In("en", "sidebar_toggle"))
	if !ok {
		t.Fatal("no sidebar toggle")
	}
	// The title is the conversation's, in the band between the toggle and
	// the strip (a transcript row scrolled under the band is not it).
	shown := a.Data.Find(a.Sidebar.Selected)
	if shown == nil || shown.Title == "" {
		t.Fatalf("no titled conversation is shown: %q", a.Sidebar.Selected)
	}
	titled := false
	for _, info := range tt.AccessAll() {
		b := info.Bounds
		if info.Name != shown.Title || b.W == 0 || b.Y < 0 || b.Y+b.H > HeaderHeight || b.X < toggle.Bounds.X+toggle.Bounds.W || b.X >= box.X {
			continue
		}
		titled = true
		if b.X+b.W > box.X {
			t.Errorf("%q at %v reaches over the strip, which starts at %v", info.Name, b, box.X)
		}
	}
	if !titled {
		t.Error("the title left the band")
	}
	// A +N still opens its group.
	clickButton(t, tt, i18n.In("en", "activity_completed")+" · +4")
	tt.Frame()
	clickButton(t, tt, "Translate release notes · Completed")
	tt.Frame()
	if a.Sidebar.Selected != "chat/translate" {
		t.Errorf("a folded group's row selected %q", a.Sidebar.Selected)
	}
}

// The strip's room is measured from the last frame's layout: a header at
// rest asks for no frame, alone and over a split. The preview shell animates
// on its own, so the header is built alone, with no session running.
func testStripAtRest(t *testing.T) {
	a := New(preview.Load())
	rest := func(list []*preview.Conversation) {
		for _, cv := range list {
			if cv.Status == preview.Running {
				cv.Status = preview.Unread
			}
		}
	}
	for _, pr := range a.Data.Projects {
		rest(pr.Conversations)
	}
	rest(a.Data.Unassigned)
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		a.followPanes()
		a.sweepOverviews()
		root := ui.Column(c.Key("frame")).Fill()
		f := a.frame(c, root)
		root.Children(func() {
			a.header(c, f)
			if a.panes.Displayed() == nil {
				return
			}
			for _, pane := range a.panes.Members(a.panes.Active) {
				ui.Column(c.Key(pane)).FillWidth().Children(func() { a.paneHeader(c, pane) })
			}
		})
	}, 1280, 820)
	tt.SetLocale("en")
	check := func(state string) {
		t.Helper()
		tt.Frame()
		tt.Advance(time.Second)
		if tt.FramePending() {
			t.Fatalf("%s: the header at rest keeps redrawing", state)
		}
		if _, _, box := stripButtons(tt); box.W == 0 {
			t.Fatalf("%s: the header has no strip", state)
		}
	}
	check("alone")
	if _, ok := headerButton(tt, i18n.In("en", "chat_search")); !ok {
		t.Fatal("the header alone has no Find")
	}
	if !a.panes.Drop(session(preview.Big), session(themeChat), panes.Right) {
		t.Fatal("drop")
	}
	check("split")
	if _, ok := headerButton(tt, i18n.In("en", "chat_search")); ok {
		t.Fatal("the band keeps Find over a split")
	}
	if n := buttons(tt, i18n.In("en", "chat_search")); n != 2 {
		t.Fatalf("the split's pane headers have %d Find buttons, want 2", n)
	}
}
