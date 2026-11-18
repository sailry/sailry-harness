package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

const (
	firstPanelChat  = "panel/first"
	secondPanelChat = "panel/second"
)

func panelLifecycleApp() (*App, *panel.Project) {
	d := &viewmodel.Data{
		Projects: []*viewmodel.Project{{ID: "shared", Name: "Shared", Conversations: []*viewmodel.Conversation{
			{ID: firstPanelChat, Title: "First"}, {ID: secondPanelChat, Title: "Second"},
		}}},
		Workspace: &viewmodel.Workspace{},
	}
	a := NewLive(d)
	a.Sidebar.Selected = firstPanelChat
	pr := panel.NewLiveProject("shared", d.Workspace, &panel.Actions{})
	a.Hooks.WorkspaceProject = func() *panel.Project { return pr }
	return a, pr
}

func panelLifecycleTester(a *App, confirmations kit.ConfirmationPresenter) *ui.Tester {
	tt := ui.NewTesterWith(func(c *ui.Context) {
		if confirmations != nil {
			kit.BindConfirmations(c, confirmations)
		}
		a.View(c)
	}, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	return tt
}

func panelToggleButton(t *testing.T, tt *ui.Tester) ui.AccessInfo {
	t.Helper()
	var toggle ui.AccessInfo
	for _, info := range tt.AccessAll() {
		b := info.Bounds
		if info.Role == ui.RoleButton && info.Name == i18n.In("en", "details") && b.Y+b.H <= HeaderHeight && b.X > toggle.Bounds.X {
			toggle = info
		}
	}
	if toggle.Bounds.W == 0 {
		t.Fatal("no panel toggle in the header")
	}
	return toggle
}

func clickPanelToggle(t *testing.T, tt *ui.Tester) {
	t.Helper()
	b := panelToggleButton(t, tt).Bounds
	tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
	tt.Frame()
}

func clickPanelTabClose(t *testing.T, tt *ui.Tester, label string) {
	t.Helper()
	info, ok := headerButton(tt, label)
	if !ok {
		t.Fatalf("no panel tab named %q", label)
	}
	b := info.Bounds
	x, y := b.X+b.W-14, b.Y+b.H/2
	tt.Move(x, y)
	tt.Frame()
	tt.ClickAt(x, y)
	tt.Frame()
	tt.Frame()
}

func assertPanelFolded(t *testing.T, a *App, tt *ui.Tester) {
	t.Helper()
	if a.PanelOpen || len(a.Panel.Tabs) != 0 || a.Panel.Active != (panel.Tab{}) || a.Page != ConversationPage || tt.HasText(i18n.In("en", "resource_browser")) {
		t.Fatalf("close left panel open %v, tabs %v, active %v, page %v, texts %q", a.PanelOpen, a.Panel.Tabs, a.Panel.Active, a.Page, tt.Texts())
	}
}

func TestConversationPanelLifecycle(t *testing.T) {
	t.Run("ToggleClearsOnlyCurrentTabsAndRetainsDraft", func(t *testing.T) {
		a, pr := panelLifecycleApp()
		tt := panelLifecycleTester(a, nil)
		history := panel.Tab{Kind: panel.History}
		a.Sidebar.Selected = secondPanelChat
		a.RevealConversationPanel()
		a.Panel.Show(history)
		a.Sidebar.Selected = firstPanelChat
		a.RevealConversationPanel()
		file, terminal := panel.Tab{Kind: panel.Files, ID: "notes.txt"}, panel.Tab{Kind: panel.Terminal, ID: "running"}
		pr.SetFile(file.ID, "source")
		pr.FileChanged(file.ID, "unsaved")
		a.Panel.TerminalActions = &panel.TerminalActions{Close: func(id string) { t.Errorf("toggle stopped terminal %q", id) }}
		a.Panel.Show(file)
		a.Panel.Show(terminal)
		tt.Frame()
		settle(tt)
		clickPanelToggle(t, tt)
		assertPanelFolded(t, a, tt)
		if a.HasPanelTab(file) || a.HasPanelTab(terminal) || !slices.Equal(a.ConversationTabs(secondPanelChat), []panel.Tab{history}) {
			t.Fatalf("wrong tabs cleared: %v", a.AllPanelTabs())
		}
		clickPanelToggle(t, tt)
		settle(tt)
		if !a.PanelOpen || len(a.Panel.Tabs) != 0 || !tt.HasText(i18n.In("en", "resource_browser")) {
			t.Fatalf("reopening restored closed tabs %v instead of the launcher", a.Panel.Tabs)
		}
		if draft, dirty := pr.FileDraft(file.ID); draft != "unsaved" || !dirty {
			t.Fatalf("toggle lost the workspace draft: %q dirty=%v", draft, dirty)
		}
	})

	t.Run("LastTabButtonFoldsWithoutLauncher", func(t *testing.T) {
		a, _ := panelLifecycleApp()
		tt := panelLifecycleTester(a, nil)
		a.RevealConversationPanel()
		a.Panel.Show(panel.Tab{Kind: panel.History})
		tt.Frame()
		settle(tt)
		clickPanelTabClose(t, tt, i18n.In("en", "git_view_history"))
		assertPanelFolded(t, a, tt)
	})

	t.Run("LastDirtyTabFoldsOnlyAfterSave", func(t *testing.T) {
		a, pr := panelLifecycleApp()
		var request kit.Confirmation
		var answer func(int)
		tt := panelLifecycleTester(a, func(value kit.Confirmation, done func(int)) { request, answer = value, done })
		file := panel.Tab{Kind: panel.Files, ID: "notes.txt"}
		a.RevealConversationPanel()
		a.Panel.Show(file)
		pr.SetFile(file.ID, "source")
		pr.SetFileWritable(file.ID, true)
		pr.FileChanged(file.ID, "unsaved")
		var savingText string
		pr.Actions.SaveFile = func(_ *ui.Context, path, text string) {
			savingText = text
			pr.SetFileStatus(path, true, false)
		}
		tt.Frame()
		settle(tt)
		label := file.ID + " •"
		clickPanelTabClose(t, tt, label)
		if answer == nil || request.Title != i18n.In("en", "files_discard_title") || !a.PanelOpen || !a.HasPanelTab(file) {
			t.Fatalf("the dirty editor did not wait for its confirmation: %+v, tabs %v", request, a.Panel.Tabs)
		}
		answer(0)
		tt.Frame()
		if draft, dirty := pr.FileDraft(file.ID); draft != "unsaved" || !dirty || !a.PanelOpen || !a.HasPanelTab(file) {
			t.Fatalf("cancel lost the draft or its tab: %q dirty=%v tabs=%v", draft, dirty, a.Panel.Tabs)
		}
		answer = nil
		clickPanelTabClose(t, tt, label)
		if answer == nil {
			t.Fatal("no second file close confirmation")
		}
		answer(2)
		tt.Frame()
		if savingText != "unsaved" || !a.PanelOpen || !a.HasPanelTab(file) {
			t.Fatalf("save did not retain the tab while in flight: %q, tabs %v", savingText, a.Panel.Tabs)
		}
		pr.SavedFile(file.ID, savingText)
		pr.SetFileStatus(file.ID, false, false)
		tt.Frame()
		assertPanelFolded(t, a, tt)
	})

	t.Run("TerminalAcknowledgmentFoldsOnlyItsPanel", func(t *testing.T) {
		a, _ := panelLifecycleApp()
		var answer func(int)
		tt := panelLifecycleTester(a, func(_ kit.Confirmation, done func(int)) { answer = done })
		var closed []string
		a.Panel.TerminalActions = &panel.TerminalActions{Close: func(id string) { closed = append(closed, id) }}
		terminal := panel.Tab{Kind: panel.Terminal, ID: "running"}
		a.RevealConversationPanel()
		a.Panel.Show(terminal)
		tt.Frame()
		settle(tt)
		a.Panel.Close(terminal)
		tt.Frame()
		if answer == nil || len(closed) != 0 || !a.PanelOpen || !a.HasPanelTab(terminal) {
			t.Fatalf("terminal close was not guarded: prompt %v, closed %v, panel open %v, tabs %v", answer != nil, closed, a.PanelOpen, a.Panel.Tabs)
		}
		answer(1)
		tt.Frame()
		if !slices.Equal(closed, []string{terminal.ID}) || !a.PanelOpen || !a.HasPanelTab(terminal) {
			t.Fatalf("terminal did not wait for the Node: closed %v, tabs %v", closed, a.Panel.Tabs)
		}
		a.Sidebar.Selected = secondPanelChat
		a.RevealConversationPanel()
		a.DropPanelTab(terminal)
		if !a.PanelOpen {
			t.Fatal("the Node acknowledgment folded another conversation's panel")
		}
		a.Sidebar.Selected = firstPanelChat
		tt.Frame()
		assertPanelFolded(t, a, tt)
		a.RevealConversationPanel()
		a.Panel.Show(terminal)
		a.DropPanelTab(terminal)
		if !a.PanelOpen || a.HasPanelTab(terminal) {
			t.Fatal("a passive terminal exit counted as an explicit panel close")
		}
	})

	t.Run("SplitAndOrdinarySwitchKeepEachPanel", func(t *testing.T) {
		a, pr := panelLifecycleApp()
		tt := panelLifecycleTester(a, nil)
		file := panel.Tab{Kind: panel.Files, ID: "notes.txt"}
		a.RevealConversationPanel()
		a.Panel.Show(file)
		pr.SetFile(file.ID, "source")
		pr.FileChanged(file.ID, "unsaved")
		first, second := session(firstPanelChat), session(secondPanelChat)
		if !a.panes.Drop(first, second, panes.Right) {
			t.Fatal("could not split the fixture")
		}
		a.focusPane(second)
		tt.Frame()
		if a.PanelOpen || len(a.Panel.Tabs) != 0 {
			t.Fatalf("split focus imported another panel: %v, open %v", a.Panel.Tabs, a.PanelOpen)
		}
		a.RevealConversationPanel()
		if len(a.Panel.Tabs) != 0 {
			t.Fatalf("opening a split panel imported another session's file %v", a.Panel.Tabs)
		}
		a.Panel.Show(file)
		a.Panel.Close(file)
		if a.PanelOpen || !slices.Equal(a.ConversationTabs(firstPanelChat), []panel.Tab{file}) {
			t.Fatalf("closing a shared tab changed its parked panel: %v", a.AllPanelTabs())
		}
		a.Sidebar.Selected = firstPanelChat
		tt.Frame()
		if !a.PanelOpen || a.Panel.Active != file || !slices.Equal(a.Panel.Tabs, []panel.Tab{file}) {
			t.Fatalf("returning to the first session: open %v, active %v, tabs %v", a.PanelOpen, a.Panel.Active, a.Panel.Tabs)
		}
		if draft, dirty := pr.FileDraft(file.ID); draft != "unsaved" || !dirty {
			t.Fatalf("closing the other panel's copy lost %q dirty=%v", draft, dirty)
		}
	})

	t.Run("LayoutAndPrefsDoNotCloseOrResurrectTabs", func(t *testing.T) {
		a, _ := panelLifecycleApp()
		tt := panelLifecycleTester(a, nil)
		file := panel.Tab{Kind: panel.Files, ID: "notes.txt"}
		a.RevealConversationPanel()
		a.Panel.Show(file)
		tt.Frame()
		tt.SetSize(RailWidth+NavWidth+MainMin+PanelMin-20, 820)
		settle(tt)
		if !panelToggleButton(t, tt).Disabled || !a.PanelOpen || !a.HasPanelTab(file) {
			t.Fatal("a narrow window closed the panel instead of hiding it")
		}
		tt.SetSize(1280, 820)
		settle(tt)
		p := a.Prefs()
		tt.Key(ui.Cmd|ui.Alt, ui.KeyB)
		tt.Frame()
		if a.PanelOpen || !a.HasPanelTab(file) {
			t.Fatal("the layout shortcut destroyed the panel tab instead of folding it")
		}
		hidden := p
		hidden.PanelOpen, hidden.PanelTabs, hidden.PanelTab = false, nil, panel.Tab{}
		a.SetPrefs(hidden)
		if a.PanelOpen || !slices.Equal(a.Panel.Tabs, []panel.Tab{file}) {
			t.Fatalf("prefs hide cleared the live tabs: %v", a.Panel.Tabs)
		}
		a.SetPrefs(p)
		tt.Frame()
		settle(tt)
		clickPanelToggle(t, tt)
		dir := t.TempDir()
		if err := SavePrefs(dir, a.Prefs()); err != nil {
			t.Fatal(err)
		}
		b, _ := panelLifecycleApp()
		b.SetPrefs(LoadPrefs(dir))
		panelLifecycleTester(b, nil)
		if b.PanelOpen || len(b.Panel.Tabs) != 0 {
			t.Fatalf("the explicitly closed panel returned from prefs: %+v", b.Prefs())
		}
		a.SetPrefs(hidden)
		p.PanelOpen = false
		a.SetPrefs(p)
		if len(a.Panel.Tabs) != 0 {
			t.Fatalf("a prefs reload resurrected the old tabs: %v", a.Panel.Tabs)
		}
	})

	t.Run("RestoreFiltersTabsAndKeepsParkedLauncher", func(t *testing.T) {
		a, _ := panelLifecycleApp()
		p := DefaultPrefs()
		// Older preferences stored just the active tab.
		p.PanelOf, p.PanelTab = firstPanelChat, panel.Tab{Kind: panel.History}
		p.Panels = map[string]ConversationPanel{secondPanelChat: {
			Tabs:   []panel.Tab{{Kind: panel.Artifact}, {Kind: panel.Subagent}, {Kind: "obsolete"}},
			Active: panel.Tab{Kind: panel.Subagent}, Open: true, Width: 460,
		}}
		a.SetPrefs(p)
		if a.PanelOpen || !slices.Equal(a.Panel.Tabs, []panel.Tab{p.PanelTab}) || len(a.ConversationTabs(secondPanelChat)) != 0 {
			t.Fatalf("saved panel contents were not scoped or filtered: %+v", a.Prefs())
		}
		launcher := a.Prefs().Panels[secondPanelChat]
		if !launcher.Open || launcher.Width != 460 || len(launcher.Tabs) != 0 || launcher.Active != (panel.Tab{}) {
			t.Fatalf("the parked launcher did not survive preference filtering: %+v", launcher)
		}
		a.Sidebar.Selected = secondPanelChat
		a.MountPanel()
		if !a.PanelOpen || a.PanelWidth != 460 || len(a.Panel.Tabs) != 0 {
			t.Fatalf("other panel did not restore its launcher: %+v", a.Prefs())
		}
	})
}
