package shell

import (
	"fmt"
	"slices"
	"strconv"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// TestConversationPaneKeys groups the focused conversation's keys; each case
// keeps its own fixture.
func TestConversationPaneKeys(t *testing.T) {
	t.Run("NewConversationSplitsRight", testNewConversationSplitsRight)
	t.Run("SplitDirections", testPaneSplitDirections)
	t.Run("InputKeepsSelectionKeys", testPaneKeysLeaveInputKeys)
	t.Run("NewTab", testPaneNewTab)
	t.Run("OutsideConversation", testPaneKeysOutsideConversation)
	t.Run("Enlarge", testPaneEnlarge)
	t.Run("SingleConversationDoesNotEnlarge", testSingleConversationDoesNotEnlarge)
	t.Run("EnlargeButtons", testPaneEnlargeButtons)
	t.Run("MinimizeOutsideConversation", testMinimizeOutsideConversation)
	t.Run("FreshFromDraft", testPaneKeysFreshFromDraft)
	t.Run("AtCapacity", testPaneKeysAtCapacity)
	t.Run("ControllerCreates", testPaneKeysControllerCreates)
}

// conversationCount counts the window's conversations.
func conversationCount(a *App) int {
	n := len(a.Data.Unassigned)
	for _, pr := range a.Data.Projects {
		n += len(pr.Conversations)
	}
	return n
}

// From an unsent conversation the keys still add a fresh one: Cmd+N splits
// beside the new draft, and Cmd+T opens another tab.
func testPaneKeysFreshFromDraft(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(ui.Cmd, ui.KeyN)
	tt.Frame()
	tt.Frame()
	first := a.panes.Active
	tt.Key(ui.Cmd, ui.KeyN)
	tt.Frame()
	tt.Frame()
	second := a.panes.Active
	g := a.panes.Displayed()
	if second == first || g == nil || !slices.Equal(g.Targets(), []panes.Target{session(preview.Big), first, second}) {
		t.Fatalf("Cmd+N from the new draft: active %v, panes %v", second, g)
	}
	tt.Key(ui.Cmd, ui.KeyT)
	tt.Frame()
	third := a.panes.Active
	if third == second || a.panes.Group(third) >= 0 || !slices.Contains(a.panes.Kept, third) || a.Data.Find(third.ID) == nil {
		t.Fatalf("Cmd+T from a draft: active %v, kept %v", third, a.panes.Kept)
	}
}

// A full split takes no new pane: the key says so, and nothing is created,
// selected or moved.
func testPaneKeysAtCapacity(t *testing.T) {
	a, tt := newTester(t)
	dest := session(preview.Big)
	for i := range panes.Limit - 1 {
		id := fmt.Sprintf("full-%d", i)
		a.OpenWorkspaceTerminal(id, "")
		target := panes.Target{Kind: panes.Terminal, ID: id}
		if !a.panes.Drop(dest, target, panes.Right) {
			t.Fatalf("drop %d", i)
		}
		dest = target
	}
	a.focusPane(session(preview.Big))
	tt.Frame()
	tt.Frame()
	group := a.panes.Displayed()
	if group == nil || len(group.Targets()) != panes.Limit {
		t.Fatalf("the fixture split is %v", group)
	}
	count := conversationCount(a)
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(ui.Cmd, ui.KeyN)
	tt.Frame()
	if a.panes.Displayed() != group || a.panes.Active != session(preview.Big) || a.Sidebar.Selected != preview.Big || conversationCount(a) != count {
		t.Fatalf("Cmd+N in a full split: active %v, selected %q, %d conversations (had %d)", a.panes.Active, a.Sidebar.Selected, conversationCount(a), count)
	}
	if notice := i18n.In("en", "pane_limit", "count", strconv.Itoa(panes.Limit)); !tt.HasText(notice) {
		t.Errorf("no notice %q", notice)
	}
}

// Live, the controller creates and selects the conversation in the focused
// one's location; one it does not create changes nothing.
func testPaneKeysControllerCreates(t *testing.T) {
	a, tt := newTester(t)
	project := a.Data.ProjectOf(preview.Big)
	var asked []*preview.Project
	a.Hooks.NewConversationDraft = func(p *preview.Project) {
		asked = append(asked, p)
		cv := &preview.Conversation{ID: "draft/fresh"}
		if p != nil {
			p.Conversations = append(p.Conversations, cv)
		} else {
			a.Data.Unassigned = append(a.Data.Unassigned, cv)
		}
		a.Sidebar.Selected = cv.ID
	}
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(ui.Cmd, ui.KeyT)
	tt.Frame()
	tt.Frame()
	if len(asked) != 1 || asked[0] != project || a.panes.Active != session("draft/fresh") || a.Sidebar.Selected != "draft/fresh" {
		t.Fatalf("Cmd+T asked %v (want %v), active %v", asked, project, a.panes.Active)
	}
	a.Hooks.NewConversationDraft = func(*preview.Project) {}
	kept := slices.Clone(a.panes.Kept)
	tt.Key(ui.Cmd, ui.KeyN)
	tt.Frame()
	if a.panes.Displayed() != nil || a.Sidebar.Selected != "draft/fresh" || !slices.Equal(a.panes.Kept, kept) {
		t.Errorf("a conversation not created changed the window: groups %v, selected %q, kept %v", a.panes.Groups, a.Sidebar.Selected, a.panes.Kept)
	}
}

// focusPaneHeader clicks the title of the first pane's header, which gives
// the pane the focus without a text input taking it. r is splitTwo's.
func focusPaneHeader(tt *ui.Tester, r ui.Rect) {
	tt.ClickAt(r.X+40, r.Y-14)
	tt.Frame()
}

// Cmd+N from the focused conversation splits a new one off to its right; the
// conversation keeps its place.
func testNewConversationSplitsRight(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(ui.Cmd, ui.KeyN)
	tt.Frame()
	g := a.panes.Displayed()
	if g == nil {
		t.Fatal("Cmd+N in the conversation did not split")
	}
	got := g.Targets()
	created := a.panes.Active
	if len(got) != 2 || got[0] != session(preview.Big) || got[1] != created || created == session(preview.Big) {
		t.Fatalf("panes %v, active %v", got, created)
	}
	if cv := a.Data.Find(created.ID); cv == nil || len(cv.Turns) != 0 {
		t.Fatalf("the new pane is not a new conversation: %+v", cv)
	}
}

// Cmd+Shift and an arrow put a new conversation on that side of the focused pane,
// leaving the other panes' shares as they were.
func testPaneSplitDirections(t *testing.T) {
	a, tt := newTester(t)
	r := splitTwo(t, a, tt)
	before := slices.Clone(a.panes.Displayed().Sizes)
	focusPaneHeader(tt, r)
	if a.panes.Active != session(preview.Big) {
		t.Fatalf("the header's click focused %v", a.panes.Active)
	}
	tt.Key(ui.Cmd|ui.Shift, ui.KeyDown)
	tt.Frame()
	g := a.panes.Displayed()
	created := a.panes.Active
	if g == nil || !slices.Equal(g.Targets(), []panes.Target{session(preview.Big), created, session(themeChat)}) {
		t.Fatalf("Cmd+Shift+Down: panes %v", g)
	}
	if !slices.Equal(g.Sizes, before) || g.Children[0].Axis != panes.Column {
		t.Fatalf("Cmd+Shift+Down changed the shares %v to %v or split along %v", before, g.Sizes, g.Children[0].Axis)
	}
}

// A text input keeps its own keys: Cmd+Shift+Left selects in the composer and splits
// nothing.
func testPaneKeysLeaveInputKeys(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, i18n.In("en", "composer"))
	tt.Type("hello")
	tt.Key(ui.Cmd|ui.Shift, ui.KeyLeft)
	tt.Frame()
	if a.panes.Displayed() != nil || a.Sidebar.Selected != preview.Big {
		t.Fatalf("Cmd+Shift+Left in the composer split: %v, selected %q", a.panes.Groups, a.Sidebar.Selected)
	}
	tt.Type("x")
	if comp := a.Composer(preview.Big); comp.Draft != "x" {
		t.Errorf("Cmd+Shift+Left did not select the draft: %q", comp.Draft)
	}
}

// Cmd+T in the focused conversation opens a new one in a tab of its own, and
// the one it came from stays a tab.
func testPaneNewTab(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(ui.Cmd, ui.KeyT)
	tt.Frame()
	created := a.panes.Active
	if created == session(preview.Big) || a.panes.Displayed() != nil || a.Data.Find(created.ID) == nil {
		t.Fatalf("Cmd+T: active %v, groups %v", created, a.panes.Groups)
	}
	if !slices.Contains(a.panes.Kept, session(preview.Big)) || !slices.Contains(a.panes.Kept, created) {
		t.Fatalf("Cmd+T kept %v", a.panes.Kept)
	}
	if a.PanelOpen && a.Panel.Active.Kind == panel.Browser {
		t.Error("Cmd+T in the conversation also opened the browser")
	}
}

// With the focus in the sidebar the conversation's keys go on to the window:
// Cmd+T opens the panel's browser as before, and nothing opens a conversation
// beyond the tab the row's click opened.
func testPaneKeysOutsideConversation(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "Port the neutral theme tokens")
	settle(tt)
	tt.Key(ui.Cmd, ui.KeyT)
	settle(tt)
	if a.Sidebar.Selected != themeChat || !slices.Equal(a.panes.Kept, []panes.Target{session(themeChat)}) || a.panes.Displayed() != nil {
		t.Fatalf("Cmd+T from the sidebar: selected %q, kept %v", a.Sidebar.Selected, a.panes.Kept)
	}
	if a.Panel.Offers(panel.Browser) && (!a.PanelOpen || a.Panel.Active.Kind != panel.Browser) {
		t.Errorf("Cmd+T from the sidebar did not open the browser: open %v, active %v", a.PanelOpen, a.Panel.Active)
	}
	tt.Key(ui.Cmd|ui.Shift, ui.KeyRight)
	tt.Key(ui.Cmd, ui.KeyM)
	tt.Frame()
	if a.panes.Displayed() != nil || a.panes.enlarged != (panes.Target{}) {
		t.Errorf("pane keys from the sidebar: groups %v, enlarged %v", a.panes.Groups, a.panes.enlarged)
	}
}

// Cmd+M floats the focused pane over the layout, which keeps its tree and
// shares; Cmd+M again, the menu's Minimize or choosing another conversation
// puts it back.
func testPaneEnlarge(t *testing.T) {
	a, tt := newTester(t)
	r := splitTwo(t, a, tt)
	group := a.panes.Displayed()
	sizes := slices.Clone(group.Sizes)
	focusPaneHeader(tt, r)
	tt.Key(ui.Cmd, ui.KeyM)
	tt.Frame()
	tt.Frame()
	restore := i18n.In("en", "pane_restore")
	if a.panes.enlarged != session(preview.Big) || buttons(tt, restore) == 0 {
		t.Fatalf("Cmd+M: enlarged %v, restore buttons %d", a.panes.enlarged, buttons(tt, restore))
	}
	if a.panes.Displayed() != group || !slices.Equal(group.Sizes, sizes) {
		t.Fatal("enlarging changed the layout under it")
	}
	if n := len(paneRows(tt)); n != 1 {
		t.Fatalf("%d pane headers under the enlarged pane, want the other one", n)
	}
	// The composer kept the focus in the floating pane: Cmd+M puts it back.
	tt.Key(ui.Cmd, ui.KeyM)
	tt.Frame()
	if a.panes.enlarged != (panes.Target{}) {
		t.Fatalf("Cmd+M again left %v enlarged", a.panes.enlarged)
	}
	// The menu's Minimize carries the same key from the focused pane.
	tt.Frame()
	minimized := 0
	a.minimizeWindow = func() { minimized++ }
	a.Do(WindowMinimize)
	tt.Frame()
	if a.panes.enlarged != session(preview.Big) || minimized != 0 {
		t.Fatalf("Minimize in the pane: enlarged %v, minimized %d", a.panes.enlarged, minimized)
	}
	a.focusPane(session(themeChat))
	tt.Frame()
	if a.panes.enlarged != (panes.Target{}) {
		t.Errorf("another active pane left %v enlarged", a.panes.enlarged)
	}
}

func testSingleConversationDoesNotEnlarge(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(ui.Cmd, ui.KeyM)
	tt.Frame()
	if a.panes.enlarged != (panes.Target{}) {
		t.Fatal("a single conversation captured the pane enlarge shortcut")
	}
	minimized := 0
	a.minimizeWindow = func() { minimized++ }
	a.Do(WindowMinimize)
	tt.Frame()
	if minimized != 1 || a.panes.enlarged != (panes.Target{}) {
		t.Fatalf("Minimize in single pane: minimized %d, enlarged %v", minimized, a.panes.enlarged)
	}
}

func testPaneEnlargeButtons(t *testing.T) {
	a, tt := newTester(t)
	splitTwo(t, a, tt)
	a.focusPane(session(themeChat))
	tt.Frame()
	group := a.panes.Displayed()
	sizes := slices.Clone(group.Sizes)
	rows := paneRows(tt)
	if len(rows) != 2 {
		t.Fatalf("expected two pane headers, got %d", len(rows))
	}
	press := func(bounds ui.Rect) {
		t.Helper()
		if bounds.W <= 0 || bounds.H <= 0 {
			t.Fatal("missing pane size button")
		}
		tt.ClickAt(bounds.X+bounds.W/2, bounds.Y+bounds.H/2)
		tt.Frame()
		tt.Frame()
	}
	press(rows[0][i18n.In("en", "shortcut_pane_enlarge")])
	if a.panes.enlarged != session(preview.Big) || a.panes.Active != session(preview.Big) || a.Sidebar.Selected != preview.Big {
		t.Fatalf("enlarging the inactive pane: enlarged %v, active %v, selected %q", a.panes.enlarged, a.panes.Active, a.Sidebar.Selected)
	}
	restore, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "pane_restore"))
	if !ok {
		t.Fatal("no restore button in enlarged pane")
	}
	press(restore.Bounds)
	if a.panes.enlarged != (panes.Target{}) || a.panes.Active != session(preview.Big) || a.panes.Displayed() != group || !slices.Equal(group.Sizes, sizes) {
		t.Fatal("restoring did not keep the focused pane and split layout")
	}
}

// Outside a conversation, under a dialog and while a key is recorded, the
// menu's Minimize does not enlarge: it minimizes the window, except while
// recording, when it is the recorded key.
func testMinimizeOutsideConversation(t *testing.T) {
	a, tt := newTester(t)
	minimized := 0
	a.minimizeWindow = func() { minimized++ }
	a.Do(WindowMinimize) // the focus is nowhere
	tt.Frame()
	if minimized != 1 || a.panes.enlarged != (panes.Target{}) {
		t.Fatalf("Minimize without focus: minimized %d, enlarged %v", minimized, a.panes.enlarged)
	}
	a.Page = SettingsPage
	a.Settings.Open(settings.Shortcuts)
	tt.Frame()
	a.Settings.Keys.Record("conversation.new")
	tt.Frame()
	a.Do(WindowMinimize)
	tt.Frame()
	if minimized != 1 || a.panes.enlarged != (panes.Target{}) {
		t.Errorf("Minimize ran while recording: minimized %d, enlarged %v", minimized, a.panes.enlarged)
	}
}

// The view zoom's keys are reserved: no shortcut records them, a key saved on
// one before stays saved but runs nothing (nor shows in the menu), and they
// zoom only where the window's keys reach: not while recording, not under a
// modal dialog.
func TestViewZoomKeysReserved(t *testing.T) {
	for _, s := range viewZoomKeys {
		if !slices.Contains(settings.Fixed, settings.Chord{Mods: s.Mods, Key: s.Key}) {
			t.Errorf("%s on %s is not reserved", s.ID, Accelerator(s.Mods, s.Key))
		}
	}
	t.Run("SavedOverride", func(t *testing.T) {
		a, tt := newTester(t)
		a.Settings.Keys.SetOverrides(map[string]string{"sidebar.toggle": "secondary-="})
		tt.Frame()
		for _, m := range a.Menu("en", func(string) {}) {
			for _, it := range m.Submenu {
				if it.ID == "sidebar.toggle" && it.Accelerator != "" {
					t.Errorf("the menu shows the reserved key %q", it.Accelerator)
				}
			}
		}
		nav := a.NavOpen
		tt.Key(ui.Cmd, ui.KeyEqual)
		tt.Frame()
		if a.Settings.ViewZoom != 1 || a.NavOpen != nav {
			t.Fatalf("Cmd+=: zoom %d, sidebar toggled %v", a.Settings.ViewZoom, a.NavOpen != nav)
		}
		if got := a.Settings.Keys.Overrides()["sidebar.toggle"]; got != "secondary-=" {
			t.Errorf("the saved key became %q", got)
		}
	})
	t.Run("Recording", func(t *testing.T) {
		a, tt := newTester(t)
		a.Page = SettingsPage
		a.Settings.Open(settings.Shortcuts)
		tt.Frame()
		a.Settings.Keys.Record("conversation.new")
		tt.Frame()
		tt.Key(ui.Cmd, ui.KeyEqual)
		tt.Frame()
		if a.Settings.ViewZoom != 0 || !a.Settings.Keys.Recording() {
			t.Fatalf("Cmd+= while recording: zoom %d, recording %v", a.Settings.ViewZoom, a.Settings.Keys.Recording())
		}
		if s, _ := a.Settings.Keys.Bound(settings.Registry[0]); s.Key != ui.KeyN {
			t.Errorf("Cmd+= was recorded: %+v", s)
		}
	})
	t.Run("ModalDialog", func(t *testing.T) {
		a, tt := newTester(t)
		if err := tt.RightClick("Port the neutral theme tokens"); err != nil {
			t.Fatal(err)
		}
		if err := tt.ChooseMenuItem(i18n.In("en", "workspace_archive")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		tt.Key(ui.Cmd, ui.KeyEqual)
		tt.Frame()
		if a.Settings.ViewZoom != 0 {
			t.Fatalf("Cmd+= zoomed behind the dialog: %d", a.Settings.ViewZoom)
		}
		tt.Key(0, ui.KeyEscape)
		settle(tt)
		tt.Key(ui.Cmd, ui.KeyEqual)
		tt.Frame()
		if a.Settings.ViewZoom != 1 {
			t.Errorf("Cmd+= after the dialog: %d", a.Settings.ViewZoom)
		}
	})
}
