package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/panel"
)

// settle lets the columns' motion end.
func settle(tt *ui.Tester) {
	tt.Advance(time.Second)
	tt.Frame()
}

// ⌘W in the focused panel closes its active tab, then the panel with its
// last tab, and the focus goes back to the conversation; the window
// stays.
func TestCloseTabInFocusedPanel(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeyP)
	tt.Key(ui.Ctrl|ui.Shift, ui.KeyG)
	settle(tt)
	if len(a.Panel.Tabs) != 2 || a.Panel.Active != panel.Changes {
		t.Fatalf("tabs %v, active %q", a.Panel.Tabs, a.Panel.Active)
	}
	click(t, tt, "File changes") // inside the panel
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	if len(a.Panel.Tabs) != 1 || a.Panel.Active != panel.Files || !a.PanelOpen {
		t.Fatalf("⌘W closed more than the tab: tabs %v, open %v", a.Panel.Tabs, a.PanelOpen)
	}
	tt.Key(ui.Cmd, ui.KeyW)
	settle(tt)
	if len(a.Panel.Tabs) != 0 || a.PanelOpen {
		t.Fatalf("the last tab left tabs %v, open %v", a.Panel.Tabs, a.PanelOpen)
	}
	// The focus is back in the conversation, where ⌘W closes nothing.
	tt.Key(ui.Cmd|ui.Alt, ui.KeyB)
	settle(tt)
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	if !a.PanelOpen {
		t.Error("⌘W closed the panel from the conversation")
	}
}

// ⌘W in a focused Files or Git page closes the page, back to the
// conversation.
func TestCloseFocusedPage(t *testing.T) {
	for _, f := range []struct {
		label, inside string
		page          Page
	}{{"files", "A native desktop Agent workspace.", FilesPage}, {"git", "File changes", GitPage}} {
		a, tt := newTester(t)
		clickButton(t, tt, i18n.In("en", f.label))
		settle(tt)
		// The rail's button has the focus: the page does not close.
		tt.Key(ui.Cmd, ui.KeyW)
		tt.Frame()
		if a.Page != f.page {
			t.Fatalf("%s: ⌘W from the rail closed the page", f.label)
		}
		click(t, tt, f.inside)
		tt.Key(ui.Cmd, ui.KeyW)
		tt.Frame()
		if a.Page != ConversationPage {
			t.Errorf("%s: ⌘W left page %d", f.label, a.Page)
		}
	}
}

// ⌘W with nothing closable focused does nothing: the composer of the
// conversation has the focus while the panel shows a tab.
func TestCloseWithNothingClosableFocused(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeyP)
	settle(tt)
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	if len(a.Panel.Tabs) != 1 || !a.PanelOpen || a.Page != ConversationPage {
		t.Errorf("⌘W in the composer changed tabs %v, open %v, page %d", a.Panel.Tabs, a.PanelOpen, a.Page)
	}
	// Settings is a page ⌘W does not close.
	clickButton(t, tt, i18n.In("en", "settings"))
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	if a.Page != SettingsPage {
		t.Errorf("⌘W closed Settings to page %d", a.Page)
	}
}

// Escape closes the popover on top only: the page under it, the panel and
// the search stay.
func TestEscapeClosesOnlyPopover(t *testing.T) {
	a, tt := newTester(t)
	clickButton(t, tt, i18n.In("en", "files"))
	settle(tt)
	click(t, tt, "A native desktop Agent workspace.")
	clickButton(t, tt, i18n.In("en", "features_all"))
	if !a.more {
		t.Fatal("More did not open")
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if a.more || a.Page != FilesPage {
		t.Fatalf("Escape left More %v, page %d", a.more, a.Page)
	}

	a, tt = newTester(t)
	tt.Key(ui.Cmd, ui.KeyP)
	settle(tt)
	click(t, tt, "A native desktop Agent workspace.")
	clickButton(t, tt, i18n.In("en", "features_all"))
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if a.more || !a.PanelOpen || len(a.Panel.Tabs) != 1 {
		t.Fatalf("Escape left More %v, panel %v, tabs %v", a.more, a.PanelOpen, a.Panel.Tabs)
	}

	// Under the search, the search's own Escape clears the query first,
	// then closes the search alone.
	tt.Key(ui.Cmd, ui.KeyK)
	tt.Type("zz")
	tt.Key(0, ui.KeyEscape)
	if !a.search.open || a.search.query != "" {
		t.Fatalf("Escape left search %v, query %q", a.search.open, a.search.query)
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if a.search.open || !a.PanelOpen {
		t.Errorf("Escape left search %v, panel %v", a.search.open, a.PanelOpen)
	}
}

// A text input keeps its editing keys: ⌘A selects its text and ⌘Z undoes,
// whatever the window binds.
func TestInputKeepsEditingKeys(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, i18n.In("en", "composer"))
	tt.Type("hello")
	tt.Key(ui.Cmd, ui.KeyA)
	tt.Type("x")
	comp := a.Composer(a.Sidebar.Selected)
	if comp.Draft != "x" {
		t.Fatalf("⌘A did not select the draft: %q", comp.Draft)
	}
	tt.Key(ui.Cmd, ui.KeyZ)
	if comp.Draft != "hello" {
		t.Errorf("⌘Z did not undo: %q", comp.Draft)
	}
}
