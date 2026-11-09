package shell

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
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
	if len(a.Panel.Tabs) != 2 || a.Panel.Active.Kind != panel.Changes {
		t.Fatalf("tabs %v, active %q", a.Panel.Tabs, a.Panel.Active)
	}
	click(t, tt, "Tracked · 3") // inside the panel
	tt.Key(ui.Cmd, ui.KeyW)
	tt.Frame()
	if len(a.Panel.Tabs) != 1 || a.Panel.Active.Kind != panel.Files || !a.PanelOpen {
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
	}{{"files", "A native desktop Agent workspace.", FilesPage}, {"git", "Tracked · 3", GitPage}} {
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
	click(t, tt, "README.md")
	tt.Frame()
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

// The native menu has the reference's menus, items and keys, localized,
// and its commands run scoped as their keys are.
func TestMenu(t *testing.T) {
	a := New(preview.Load())
	var sent []string
	menu := a.Menu("en", func(id string) { sent = append(sent, id) })
	type entry struct{ label, key string }
	cmd := func(mods ui.Modifiers, key ui.Key) string { return Accelerator(mods, key) }
	want := map[string][]entry{
		"Sailry": {{"About Sailry", ""}, {"Settings", cmd(ui.Cmd, ui.KeyComma)}, {"Services", ""},
			{"Hide Sailry", cmd(ui.Super, ui.KeyH)}, {"Hide Others", cmd(ui.Super|ui.Alt, ui.KeyH)}, {"Show All", ""},
			{"Quit Sailry", cmd(ui.Cmd, ui.KeyQ)}},
		"File": {{"New Conversation", cmd(ui.Cmd, ui.KeyN)}, {"New Project", cmd(ui.Cmd|ui.Alt, ui.KeyN)},
			{"Open Folder", cmd(ui.Cmd, ui.KeyO)}, {"Save", cmd(ui.Cmd, ui.KeyS)},
			{"Close Tab", cmd(ui.Cmd, ui.KeyW)}, {"Close Window", cmd(ui.Super|ui.Shift, ui.KeyW)}},
		"Edit": {{"Undo", cmd(ui.Cmd, ui.KeyZ)}, {"Redo", cmd(ui.Cmd|ui.Shift, ui.KeyZ)}, {"Cut", cmd(ui.Cmd, ui.KeyX)},
			{"Copy", cmd(ui.Cmd, ui.KeyC)}, {"Paste", cmd(ui.Cmd, ui.KeyV)}, {"Select All", cmd(ui.Cmd, ui.KeyA)}},
		"View": {{"Search", cmd(ui.Cmd, ui.KeyK)}, {"Find in Conversation", cmd(ui.Cmd, ui.KeyF)},
			{"Toggle Sidebar", cmd(ui.Cmd, ui.KeyB)}, {"Toggle Details", cmd(ui.Cmd|ui.Alt, ui.KeyB)},
			{"Toggle Full Screen", cmd(ui.Ctrl|ui.Super, ui.KeyF)},
			{"Chats", ""}, {"Activity board", ""}, {"Files", ""}, {"Git", ""}, {"Market", ""}},
		"Window": {{"Minimize", cmd(ui.Super, ui.KeyM)}, {"Zoom", ""}, {"Bring All to Front", ""}},
		"Help":   {{"Website", ""}, {"Feedback", ""}, {"Logs", ""}, {"Welcome to Sailry", ""}, {"Selected text", ""}},
	}
	var names []string
	for _, m := range menu {
		names = append(names, m.Label)
		var got []entry
		for _, it := range m.Submenu {
			if it.Type != mygo.MenuItemSeparator {
				got = append(got, entry{it.Label, it.Accelerator})
			}
		}
		if !slices.Equal(got, want[m.Label]) {
			t.Errorf("%s: %q, want %q", m.Label, got, want[m.Label])
		}
	}
	if !slices.Equal(names, []string{"Sailry", "File", "Edit", "View", "Window", "Help"}) {
		t.Errorf("menus %q", names)
	}
	// Every entry but the features' names (Git) is translated in Chinese.
	zh := a.Menu("zh-CN", nil)
	for i, m := range menu {
		for j, it := range m.Submenu {
			if it.Label != "" && i > 0 && !strings.HasPrefix(it.ID, "page:") && zh[i].Submenu[j].Label == it.Label {
				t.Errorf("%q is not translated", it.Label)
			}
		}
	}
	// The app's items send their command; Logs is disabled.
	for _, it := range menu[1].Submenu {
		if it.Click != nil {
			it.Click(it, nil)
		}
	}
	if !slices.Equal(sent, []string{"conversation.new", "project.new", "folder.open", "close"}) {
		t.Errorf("File sent %q", sent)
	}
	if logs := menu[5].Submenu[2]; !logs.Disabled {
		t.Error("Logs is enabled without a log folder")
	}
	// Find in Conversation sends its command, as ⌘F.
	if find := menu[3].Submenu[1]; find.Disabled || find.ID != "conversation.find" {
		t.Errorf("Find in Conversation %+v", find)
	}
}

// Close Tab of the menu closes what ⌘W closes where the focus is, and
// the window's commands wait while a dialog shows.
func TestMenuCommandsScoped(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeyP)
	settle(tt)
	a.Do(CloseFocused.ID) // the focus is nowhere
	tt.Frame()
	if len(a.Panel.Tabs) != 1 {
		t.Fatalf("Close Tab without focus closed tabs: %v", a.Panel.Tabs)
	}
	click(t, tt, "README.md")
	tt.Frame()
	click(t, tt, "A native desktop Agent workspace.")
	a.Do(CloseFocused.ID)
	settle(tt)
	a.Do(CloseFocused.ID)
	settle(tt)
	if len(a.Panel.Tabs) != 0 || a.PanelOpen {
		t.Fatalf("Close Tab in the panel left %v, open %v", a.Panel.Tabs, a.PanelOpen)
	}
	a.Do("page:files")
	tt.Frame()
	tt.Frame()
	click(t, tt, "README.md")
	tt.Frame()
	click(t, tt, "A native desktop Agent workspace.")
	a.Do(CloseFocused.ID)
	settle(tt)
	a.Do(CloseFocused.ID)
	tt.Frame()
	if a.Page != ConversationPage {
		t.Fatalf("Close Tab in the Files page left page %d", a.Page)
	}
	tt.Key(ui.Cmd, ui.KeyK)
	a.Do("sidebar.toggle")
	tt.Frame()
	if !a.NavOpen {
		t.Error("Toggle Sidebar ran under the search")
	}
}
