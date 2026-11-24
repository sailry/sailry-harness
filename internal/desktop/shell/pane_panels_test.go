package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The right panel belongs to the header tab's pane, conversation or main
// terminal alike: switching tabs shows each pane's own column, and coming
// back restores it with its terminal still attached.
func TestPanelBelongsToPane(t *testing.T) {
	a, pr := panelLifecycleApp()
	a.Data.Projects[0].Conversations = append(a.Data.Projects[0].Conversations, &viewmodel.Conversation{ID: "panel/third", Title: "Third"})
	pr.SetFile("notes.txt", "source")
	stopped := 0
	a.Panel.TerminalActions = &panel.TerminalActions{
		View:  func(c *ui.Context, id string) { ui.Text(c, "details screen "+id) },
		Pane:  func(c *ui.Context, id string) { ui.Text(c, "main screen "+id) },
		Close: func(string) { stopped++ },
	}
	tt := panelLifecycleTester(a, nil)
	frames := func() {
		tt.Frame()
		settle(tt)
	}
	// tab clicks a workspace tab: the leftmost header button of its title, as
	// the details column's own tabs follow the workspace tabs.
	tab := func(title string) {
		t.Helper()
		var b ui.AccessInfo
		for _, info := range tt.AccessAll() {
			r := info.Bounds
			if info.Role == ui.RoleButton && info.Name == title && r.W > 0 && r.Y+r.H <= HeaderHeight && (b.Bounds.W == 0 || r.X < b.Bounds.X) {
				b = info
			}
		}
		if b.Bounds.W == 0 {
			t.Fatalf("no header tab %q: %q", title, tt.Texts())
		}
		tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		frames()
		frames()
	}
	for _, id := range []string{"panel/third", firstPanelChat} {
		a.OpenConversation(id)
		frames()
	}
	details := panel.Tab{Kind: panel.Terminal, ID: "details-terminal"}
	a.RevealConversationPanel()
	a.Panel.Show(details)
	frames()
	if !a.PanelOpen || a.Panel.Active != details || !tt.HasText("details screen "+details.ID) {
		t.Fatalf("the conversation's details terminal: open %v, active %v, texts %q", a.PanelOpen, a.Panel.Active, tt.Texts())
	}

	a.OpenWorkspaceTerminal("main-terminal", "Project shell")
	main := panes.Target{Kind: panes.Terminal, ID: "main-terminal"}
	a.panes.Keep(main)
	frames()
	if a.panes.Active != main || a.PanelOpen || tt.HasText("details screen "+details.ID) || !slices.Equal(a.ConversationTabs(firstPanelChat), []panel.Tab{details}) {
		t.Fatalf("the terminal tab borrowed the conversation's panel: open %v, shown %v, first's %v", a.PanelOpen, a.Panel.Tabs, a.ConversationTabs(firstPanelChat))
	}
	file := panel.Tab{Kind: panel.Files, ID: "notes.txt"}
	a.RevealConversationPanel()
	a.Panel.Show(file)
	frames()
	if !a.PanelOpen || a.Panel.Active != file || !slices.Equal(a.ConversationTabs(firstPanelChat), []panel.Tab{details}) {
		t.Fatalf("the terminal tab's own panel: open %v, shown %v, first's %v", a.PanelOpen, a.Panel.Tabs, a.ConversationTabs(firstPanelChat))
	}

	tab("Third")
	if a.Sidebar.Selected != "panel/third" || a.PanelOpen || len(a.Panel.Tabs) != 0 {
		t.Fatalf("a conversation that never opened its panel inherited one: selected %q, open %v, tabs %v", a.Sidebar.Selected, a.PanelOpen, a.Panel.Tabs)
	}

	tab("First")
	if a.Sidebar.Selected != firstPanelChat || !a.PanelOpen || a.Panel.Active != details || !slices.Equal(a.Panel.Tabs, []panel.Tab{details}) || !tt.HasText("details screen "+details.ID) {
		t.Fatalf("the conversation's panel did not come back: open %v, tabs %v, active %v, texts %q", a.PanelOpen, a.Panel.Tabs, a.Panel.Active, tt.Texts())
	}

	tab("Project shell")
	if a.panes.Active != main || !a.PanelOpen || !slices.Equal(a.Panel.Tabs, []panel.Tab{file}) || !tt.HasText("main screen main-terminal") {
		t.Fatalf("the terminal tab's panel did not come back: active %v, open %v, tabs %v", a.panes.Active, a.PanelOpen, a.Panel.Tabs)
	}
	if !a.HasPanelTab(details) || stopped != 0 {
		t.Fatalf("switching tabs closed a terminal: tabs %v, stopped %d", a.AllPanelTabs(), stopped)
	}

	// Closing the main terminal's surface takes its panel, not its process.
	a.DropWorkspaceTerminal(main.ID)
	frames()
	if a.HasPanelTab(file) || !a.HasPanelTab(details) || stopped != 0 || !a.PanelOpen || a.Panel.Active != details {
		t.Fatalf("closing the terminal tab: tabs %v, stopped %d, open %v, active %v", a.AllPanelTabs(), stopped, a.PanelOpen, a.Panel.Active)
	}
}
