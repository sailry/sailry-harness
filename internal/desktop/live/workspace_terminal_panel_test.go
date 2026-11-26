package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A main terminal pane's details panel shows the project and worktree its
// shell runs in, not those of the conversation selected behind it, and the
// conversation takes its own back when its pane is active again.
func TestTerminalPanePanelUsesTerminalWorkspace(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		first, second := protocol.DefaultProject(), protocol.DefaultProject()
		first.ID, first.Name, first.Path = protocol.ProjectID{1}, "First", "/first"
		second.ID, second.Name, second.Path = protocol.ProjectID{2}, "Second", "/second"
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Projects = protocol.NodeID{1}, []protocol.Project{first, second}
		snapshot.Worktrees = []protocol.Worktree{
			{ID: protocol.WorktreeID{1}, Project: &first.ID, Path: first.Path, Main: true},
			{ID: protocol.WorktreeID{2}, Project: &second.ID, Path: second.Path, Main: true},
		}
		session := protocol.SessionID{9}
		snapshot.Sessions = []protocol.Session{{ID: session, Worktree: protocol.WorktreeID{1}, Project: &first.ID}}
		info := terminalTabsTestInfo(3, protocol.WorktreeID{2}, "running")
		info.Directory = new("/second/src")
		snapshot.Terminals = []protocol.TerminalInfo{info}
		root, _ := snapshotFixture(t, snapshot, testassembly.RegisterTerminalFixture)

		root.App.Sidebar.Selected = session.String()
		root.workspaceFrame(nil)
		if root.workspace.selected != (protocol.WorktreeID{1}) || !root.App.Panel.Session {
			t.Fatal("the selected conversation did not show its own worktree")
		}

		root.App.OpenWorkspaceTerminal(info.ID.String(), "")
		if owner := root.selectedRuntime(); owner != root {
			t.Fatal("the terminal pane's panel did not resolve its terminal's Node")
		}
		root.workspaceFrame(nil)
		if root.workspace.selected != (protocol.WorktreeID{2}) || root.App.Panel.Session || !root.App.Panel.HasProject || root.App.Panel.Cwd != "/second/src" {
			t.Fatalf("terminal pane panel: worktree %v, session %v, project %v, cwd %q",
				root.workspace.selected, root.App.Panel.Session, root.App.Panel.HasProject, root.App.Panel.Cwd)
		}
		if root.workspaceProject() != root.workspace.projects[protocol.WorktreeID{2}].panel {
			t.Fatal("the terminal pane's panel bound another worktree's project")
		}

		root.App.DropWorkspaceTerminal(info.ID.String())
		root.workspaceFrame(nil)
		if root.workspace.selected != (protocol.WorktreeID{1}) || !root.App.Panel.Session {
			t.Fatal("the conversation did not take its worktree back from the terminal pane")
		}
	})
}

// Moving a conversation's only details terminal to the main workspace closes
// that conversation's panel instead of leaving it open and empty.
func TestTerminalMovedToWorkspaceClosesEmptyPanel(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		info := terminalTabsTestInfo(1, protocol.WorktreeID{1}, "running")
		root, _ := terminalTabsFixture(t, info)
		id := info.ID.String()
		root.App.Sidebar.Selected = "draft/first"
		root.App.MountPanel()
		root.showLiveTerminal(id)
		if !root.App.PanelOpen || !root.App.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: id}) {
			t.Fatal("the terminal did not open in the conversation's panel")
		}

		root.showWorkspaceTerminal(id)
		root.App.MountPanel()
		if !root.App.HasWorkspaceTerminal(id) || root.App.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: id}) || root.App.PanelOpen {
			t.Fatalf("moving the terminal: workspace %v, panel open %v", root.App.WorkspaceTerminalIDs(), root.App.PanelOpen)
		}
		root.App.DropWorkspaceTerminal(id)
		root.App.MountPanel()
		if root.App.PanelOpen || len(root.App.Panel.Tabs) != 0 {
			t.Fatalf("the conversation's panel came back open and empty: open %v, tabs %v", root.App.PanelOpen, root.App.Panel.Tabs)
		}
	})
}
