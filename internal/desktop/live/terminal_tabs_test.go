package live

import (
	"context"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// terminalTabsTestNode answers terminal commands with captured responses and
// records them. Terminal streams are not served: these checks stop before a
// tab attaches.
type terminalTabsTestNode struct {
	pluginViewTestNode
	mu       sync.Mutex
	commands []protocol.Command
	respond  func(protocol.Request) link.Response
}

func (source *terminalTabsTestNode) Dispatch(_ context.Context, _ protocol.NodeID, request protocol.Request) (link.Admission, error) {
	source.mu.Lock()
	source.commands = append(source.commands, request.Command)
	source.mu.Unlock()
	completion := make(chan link.Response, 1)
	completion <- source.respond(request)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

func (source *terminalTabsTestNode) sent(kind string) []protocol.Command {
	source.mu.Lock()
	defer source.mu.Unlock()
	var commands []protocol.Command
	for _, command := range source.commands {
		if command.Kind == kind {
			commands = append(commands, command)
		}
	}
	return commands
}

func terminalTabsTestInfo(id byte, worktree protocol.WorktreeID, status string) protocol.TerminalInfo {
	info := protocol.DefaultTerminalInfo()
	info.ID, info.Worktree, info.Directory = protocol.TerminalID{id}, &worktree, new("/fixture")
	info.Status = protocol.TerminalStatus{Kind: status}
	return info
}

func terminalTabsFixture(t *testing.T, terminals ...protocol.TerminalInfo) (*Runtime, *terminalTabsTestNode) {
	t.Helper()
	project := protocol.DefaultProject()
	project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Terminal project", "/fixture"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Projects = protocol.NodeID{1}, []protocol.Project{project}
	snapshot.Worktrees = []protocol.Worktree{{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}}
	snapshot.Terminals = terminals
	source := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot}}
	source.respond = func(request protocol.Request) link.Response {
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
	}
	ctx, cancel := context.WithCancel(t.Context())
	controller := client.New(link.NewLocal(snapshot.Node, snapshot.Node, source))
	if err := controller.Start(ctx); err != nil {
		cancel()
		t.Fatal(err)
	}
	root := &Runtime{
		ctx: ctx, cancel: cancel, controller: controller, packages: newPackageState(),
		busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
		App:         shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}),
		Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json")),
	}
	root.packages.terminals = map[string]*liveTerminal{}
	t.Cleanup(func() {
		cancel()
		root.workers.Wait()
		root.observers.Wait()
		_ = controller.Close()
	})
	synctest.Wait()
	if !controller.Connected() {
		t.Fatal("fixture Node did not deliver its snapshot")
	}
	return root, source
}

func terminalTabsTestTabs(root *Runtime) []string {
	var ids []string
	for _, tab := range root.App.Panel.Tabs {
		if tab.Kind == panel.Terminal {
			ids = append(ids, tab.ID)
		}
	}
	return ids
}

// testTerminalTabsPrune covers PANEL-056 and PANE-016: only a terminal its
// Node reports and has not closed keeps its tab.
func testTerminalTabsPrune(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		worktree := protocol.WorktreeID{1}
		running, closed := terminalTabsTestInfo(1, worktree, "running"), terminalTabsTestInfo(2, worktree, "closed")
		stopped := terminalTabsTestInfo(3, worktree, "stopped")
		root, _ := terminalTabsFixture(t, running, closed, stopped)
		missing, pending := protocol.TerminalID{4}.String(), protocol.TerminalID{5}.String()
		for _, id := range []string{running.ID.String(), closed.ID.String(), stopped.ID.String(), missing, pending, "1"} {
			root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
		}
		root.App.Panel.Show(panel.Tab{Kind: panel.Browser, ID: "1"})
		root.terminalInventory().pending[pending] = true
		root.terminalsFrame(nil)
		want := []string{running.ID.String(), stopped.ID.String(), pending}
		if got := terminalTabsTestTabs(root); !slices.Equal(got, want) {
			t.Fatalf("terminal tabs after prune = %v, want %v", got, want)
		}
		if !slices.Contains(root.App.Panel.Tabs, panel.Tab{Kind: panel.Browser, ID: "1"}) {
			t.Fatal("pruning terminal tabs removed another tab")
		}
		if node, known := root.terminalInventory().nodes[running.ID.String()]; !known || node != root.controller.Target() {
			t.Fatal("a reported terminal did not record its Node")
		}
	})
}

// testTerminalTabsCreate covers PANEL-051 and PANE-011: New terminal shows a
// tab named by the request before the Node answers, and a split starts its
// terminal in the same worktree.
func testTerminalTabsCreate(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		worktree := protocol.WorktreeID{1}
		root, source := terminalTabsFixture(t)
		source.respond = func(request protocol.Request) link.Response {
			launch, _ := detail[protocol.TerminalLaunch](request.Command.Data)
			info := terminalTabsTestInfo(0, launch.Worktree, "running")
			info.ID = protocol.TerminalID(request.ID)
			return link.Response{Output: protocol.Output{Kind: "terminal", Data: info}}
		}
		id, created := root.createLiveTerminal(root, worktree)
		if !created || root.App.Panel.Active != (panel.Tab{Kind: panel.Terminal, ID: id}) {
			t.Fatalf("New terminal did not show its tab: %q %v", id, root.App.Panel.Active)
		}
		if !root.terminalInventory().pending[id] || root.packages.terminals[id] == nil {
			t.Fatal("a created terminal was not retained until its snapshot")
		}
		root.terminalsFrame(nil)
		if !slices.Contains(terminalTabsTestTabs(root), id) {
			t.Fatal("a created terminal's tab was pruned before its Node reported it")
		}
		synctest.Wait()
		commands := source.sent("create_terminal")
		if len(commands) != 1 {
			t.Fatalf("create_terminal commands = %d, want 1", len(commands))
		}
		launch, valid := detail[protocol.TerminalLaunch](commands[0].Data)
		if !valid || launch.Worktree != worktree || launch.Access != protocol.ProcessAccessUnrestricted {
			t.Fatalf("unexpected terminal launch: %+v", launch)
		}
		if _, created := root.createLiveTerminal(root, protocol.WorktreeID{}); created {
			t.Fatal("a terminal was created without a worktree")
		}
	})
}

// testTerminalTabsClose covers PANEL-052: Close terminal sends
// close_terminal and the tab leaves when the Node closed it or no longer
// knows it; any other failure keeps the tab.
func testTerminalTabsClose(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		worktree := protocol.WorktreeID{1}
		first, second := terminalTabsTestInfo(1, worktree, "running"), terminalTabsTestInfo(2, worktree, "running")
		root, source := terminalTabsFixture(t, first, second)
		source.respond = func(request protocol.Request) link.Response {
			payload, _ := detail[protocol.CommandCloseTerminalPayload](request.Command.Data)
			if payload.Terminal == second.ID {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "terminal is unavailable"}}
			}
			info := first
			info.Status = protocol.TerminalStatus{Kind: "closed"}
			return link.Response{Output: protocol.Output{Kind: "terminal", Data: info}}
		}
		for _, info := range []protocol.TerminalInfo{first, second} {
			root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: info.ID.String()})
			root.closeLiveTerminal(info.ID.String())
		}
		synctest.Wait()
		pluginViewRunQueue(root)
		if got := terminalTabsTestTabs(root); len(got) != 0 {
			t.Fatalf("closed terminals kept their tabs: %v", got)
		}
		commands := source.sent("close_terminal")
		if len(commands) != 2 {
			t.Fatalf("close_terminal commands = %d, want 2", len(commands))
		}
		payload, valid := detail[protocol.CommandCloseTerminalPayload](commands[0].Data)
		if !valid || payload.Worktree != worktree {
			t.Fatalf("unexpected close payload: %+v", payload)
		}
	})
}

// testTerminalTabsProject covers PROJ-028: a project's Terminal records are
// its worktrees' terminals the Node has not closed.
func testTerminalTabsProject(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		worktree := protocol.WorktreeID{1}
		running, closed := terminalTabsTestInfo(1, worktree, "running"), terminalTabsTestInfo(2, worktree, "closed")
		other := terminalTabsTestInfo(3, protocol.WorktreeID{9}, "running")
		root, _ := terminalTabsFixture(t, running, closed, other)
		project := &viewmodel.Project{ID: protocol.ProjectID{1}.String(), Host: root.controller.Target().String()}
		terminals := root.projectTerminals(project)
		if len(terminals) != 1 || terminals[0].ID != running.ID.String() || terminals[0].Path != "/fixture" {
			t.Fatalf("project terminals = %+v", terminals)
		}
		root.showLiveTerminal(running.ID.String())
		if root.App.Panel.Active != (panel.Tab{Kind: panel.Terminal, ID: running.ID.String()}) || !root.App.PanelOpen {
			t.Fatal("opening a terminal record did not show its tab")
		}
	})
}
