package live

import (
	"slices"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	harness "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativeTerminalEntriesUseBoundNode(t *testing.T) {
	synctestWithRaster(t, func(t *testing.T) {
		local := protocol.DefaultSnapshot()
		local.Node = protocol.NodeID{1}
		project := protocol.DefaultProject()
		project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Local project", "/local"
		local.Projects = []protocol.Project{project}
		local.Worktrees = []protocol.Worktree{{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}}
		local.Plugins = []protocol.PluginSummary{{Name: "commands", Enabled: true, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}}}
		localFixture, localSource := snapshotFixture(t, local)

		remote := protocol.DefaultSnapshot()
		remote.Node = protocol.NodeID{2}
		project.Name, project.Path = "Remote project", "/remote"
		main, topic := protocol.WorktreeID{2}, protocol.WorktreeID{3}
		remote.Projects = []protocol.Project{project}
		remote.Worktrees = []protocol.Worktree{
			{ID: main, Project: &project.ID, Path: project.Path, Main: true},
			{ID: topic, Project: &project.ID, Path: "/remote/topic"},
		}
		session := protocol.DefaultSession()
		session.ID, session.Project, session.Worktree = protocol.SessionID{1}, &project.ID, topic
		remote.Sessions = []protocol.Session{session}
		remoteFixture, remoteSource := snapshotFixture(t, remote)
		fallback := remoteSource.respond
		remoteSource.respond = func(request protocol.Request) link.Response {
			if request.Command.Kind == "create_terminal" {
				launch, _ := detail[protocol.TerminalLaunch](request.Command.Data)
				info := terminalTabsTestInfo(0, launch.Worktree, "running")
				info.ID = protocol.TerminalID(request.ID)
				return link.Response{Output: protocol.Output{Kind: "terminal", Data: info}}
			}
			return fallback(request)
		}

		assembly := harness.Harness()
		assembly.Terminal = true
		root, err := NewWithAssembly(t.Context(), localFixture.controller, localFixture.Preferences, assembly)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = root.Close() })
		root.bindings = map[string]*Runtime{remote.Node.String(): root.newBinding(remoteFixture.controller)}
		root.project()
		index := slices.IndexFunc(root.App.Data.Projects, func(project *viewmodel.Project) bool { return project.Host == remote.Node.String() })
		if index < 0 {
			t.Fatal("the bound project's workspace was not projected")
		}
		listed := root.App.Data.Projects[index]
		label := i18n.In("en", "workspace_new_terminal")
		check := func(worktrees ...protocol.WorktreeID) {
			t.Helper()
			synctest.Wait()
			if count := len(localSource.sent("create_terminal")); count != 0 {
				t.Fatalf("the local Node received %d remote terminal requests", count)
			}
			commands := remoteSource.sent("create_terminal")
			if len(commands) != len(worktrees) {
				t.Fatalf("remote terminal requests = %d, want %d", len(commands), len(worktrees))
			}
			for i, command := range commands {
				if launch, valid := detail[protocol.TerminalLaunch](command.Data); !valid || launch.Worktree != worktrees[i] {
					t.Fatalf("remote terminal request %d = %+v, want worktree %s", i, launch, worktrees[i])
				}
			}
		}

		menu := ui.NewTester(func(view *ui.Context) {
			ui.ButtonBase(view.Key("project")).Label(listed.Name).Size(160, 28).ContextMenu(func(menu *ui.Menu) {
				root.App.Sidebar.Actions.ProjectMenu(view, menu, listed)
			})
		}, 400, 300)
		menu.SetLocale("en")
		menu.Frame()
		if err := menu.RightClick(listed.Name); err != nil {
			t.Fatal(err)
		}
		if err := menu.ChooseMenuItem(label); err != nil {
			t.Fatal(err)
		}
		check(main)

		root.openProjectOverviewAt(listed, "/remote/topic")
		tester := ui.NewTester(root.App.View, 1240, 900)
		tester.SetLocale("en")
		tester.Frame()
		if err := tester.Click(label); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		check(main, topic)
		if ids := root.App.WorkspaceTerminalIDs(); len(ids) != 2 {
			t.Fatalf("project actions did not open two independent terminals: %v", ids)
		}

		root.OpenSession(session.ID.String())
		tester.Frame()
		if !root.App.Panel.Offers(panel.Terminal) {
			t.Fatal("the bound session has no terminal launcher")
		}
		tester.Key(ui.Ctrl, ui.KeyBackquote)
		tester.Frame()
		check(main, topic, topic)
		root.App.Panel.Open(panel.Terminal)
		check(main, topic, topic, topic)
		if ids := terminalTabsTestTabs(root); len(ids) != 2 || root.App.Panel.Active.ID != ids[1] {
			t.Fatalf("the shortcut and launcher did not open two panel terminals: %v", ids)
		}
		if count := len(remoteSource.sent("read_plugin_view")); count != 0 {
			t.Fatalf("native terminals read %d package renderers", count)
		}
	})
}
