package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

// An assembly with the terminal panel and no workspace terminal pane, as an
// older SDK registration supplied, opens project terminals in the right
// panel. Neither a new shell nor a listed one is left without a surface.
func TestPanelOnlyTerminalAssemblyKeepsSurfaces(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		project := protocol.DefaultProject()
		project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Panel project", "/fixture"
		worktree := protocol.WorktreeID{1}
		running := terminalTabsTestInfo(1, worktree, "running")
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Projects = protocol.NodeID{1}, []protocol.Project{project}
		snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project.ID, Path: project.Path, Main: true}}
		snapshot.Terminals = []protocol.TerminalInfo{running}
		root, source := snapshotFixture(t, snapshot, func(a *shell.Assembly) {
			a.RegisterPanelResource(panel.Resource{Kind: panel.Terminal, Label: "terminal", Icon: icons.Terminal, Render: native.RenderTerminalTab})
		})
		source.respond = func(request protocol.Request) link.Response {
			launch, _ := detail[protocol.TerminalLaunch](request.Command.Data)
			info := terminalTabsTestInfo(0, launch.Worktree, "running")
			info.ID = protocol.TerminalID(request.ID)
			return link.Response{Output: protocol.Output{Kind: "terminal", Data: info}}
		}
		if root.App.HasWorkspacePane(panes.Terminal) {
			t.Fatal("the fixture assembly unexpectedly has a workspace terminal pane")
		}
		root.App.Sidebar.Actions = &sidebar.Actions{}
		root.initTerminalActions()
		listed := &viewmodel.Project{ID: project.ID.String(), Host: root.controller.Target().String()}

		root.App.Sidebar.Actions.NewTerminal(listed)
		created := root.App.Panel.Active
		if created.Kind != panel.Terminal || !root.App.HasPanelTab(created) || len(root.App.WorkspaceTerminalIDs()) != 0 {
			t.Fatalf("a new project terminal has no panel surface: active %v, tabs %v", created, root.App.AllPanelTabs())
		}
		synctest.Wait()
		root.terminalsFrame(nil)
		if !root.App.HasPanelTab(created) || root.packages.terminals[created.ID] == nil {
			t.Fatal("the created terminal lost its panel surface")
		}

		// Choosing the panel's own terminal from the sidebar keeps its tab.
		root.App.Sidebar.Actions.OpenTerminal(created.ID)
		root.terminalsFrame(nil)
		if !root.App.HasPanelTab(created) || root.packages.terminals[created.ID] == nil {
			t.Fatal("opening a panel terminal from the sidebar removed its only surface")
		}

		existing := panel.Tab{Kind: panel.Terminal, ID: running.ID.String()}
		root.App.Sidebar.Actions.OpenTerminal(existing.ID)
		root.terminalsFrame(nil)
		if !root.App.HasPanelTab(existing) || root.App.Panel.Active != existing {
			t.Fatalf("a listed terminal did not open in the panel: tabs %v", root.App.AllPanelTabs())
		}
		synctest.Wait()
		if commands := source.sent("create_terminal"); len(commands) != 1 {
			t.Fatalf("create_terminal commands = %d, want 1", len(commands))
		}
		if len(source.sent("close_terminal")) != 0 {
			t.Fatal("a panel-only terminal action stopped a Node terminal")
		}
	})
}
