package live

import (
	"context"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
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
	root, source := snapshotFixture(t, snapshot)
	root.App = shell.NewLiveWithAssembly(root.App.Data, testassembly.Full())
	root.App.Sidebar.Actions = &sidebar.Actions{}
	root.initTerminalActions()
	return root, source
}

// snapshotFixture is a started Runtime on a Node that delivers snapshot and
// fails every command until the test sets its responses.
func snapshotFixture(t *testing.T, snapshot protocol.Snapshot) (*Runtime, *terminalTabsTestNode) {
	t.Helper()
	source := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot, updates: make(chan protocol.Update)}}
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

func TestTerminalTabSurvivesBindingReplacement(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		root, _ := terminalTabsFixture(t)
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = protocol.NodeID{2}
		info := terminalTabsTestInfo(4, protocol.WorktreeID{2}, "running")
		snapshot.Terminals = []protocol.TerminalInfo{info}
		node := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot}}
		makeBinding := func() *Runtime {
			owner := client.New(link.NewLocal(snapshot.Node, snapshot.Node, node))
			t.Cleanup(func() { _ = owner.Close() })
			if err := owner.Start(root.ctx); err != nil {
				t.Fatal(err)
			}
			return &Runtime{ctx: root.ctx, controller: owner}
		}
		previous := makeBinding()
		id := info.ID.String()
		ctx, cancel := context.WithCancel(root.ctx)
		root.packages.terminals[id] = &liveTerminal{owner: previous, cancel: cancel}
		root.terminalInventory().pending[id] = true
		root.App.OpenWorkspaceTerminal(id, "")

		root.detachBindingTerminals(previous)
		root.terminalsFrame(nil)
		if ctx.Err() == nil || root.packages.terminals[id] != nil || !slices.Contains(root.App.WorkspaceTerminalIDs(), id) {
			t.Fatal("retiring a binding should detach the stream and preserve its tab")
		}
		current := makeBinding()
		root.bindings = map[string]*Runtime{snapshot.Node.String(): current}
		root.terminalsFrame(nil)
		owner, captured, exists := root.terminalOwner(info.ID)
		if !exists || owner != current || captured.ID != info.ID || !slices.Contains(root.App.WorkspaceTerminalIDs(), id) {
			t.Fatal("the existing tab did not resolve to its replacement binding")
		}
	})
}

// testTerminalTabsPrune covers PANEL-056 and PANE-016: only a terminal its
// Node reports as active keeps its tab.
func testTerminalTabsPrune(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		worktree := protocol.WorktreeID{1}
		running, closed := terminalTabsTestInfo(1, worktree, "running"), terminalTabsTestInfo(2, worktree, "closed")
		stopped, exited := terminalTabsTestInfo(3, worktree, "stopped"), terminalTabsTestInfo(4, worktree, "exited")
		failed := terminalTabsTestInfo(5, worktree, "failed")
		root, _ := terminalTabsFixture(t, running, closed, stopped, exited, failed)
		missing, pending := protocol.TerminalID{6}.String(), protocol.TerminalID{7}.String()
		for _, id := range []string{running.ID.String(), closed.ID.String(), stopped.ID.String(), exited.ID.String(), failed.ID.String(), missing, pending, "1"} {
			root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
		}
		root.App.Panel.Show(panel.Tab{Kind: panel.Browser, ID: "1"})
		root.terminalInventory().pending[pending] = true
		root.terminalsFrame(nil)
		want := []string{running.ID.String(), pending}
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
	t.Run("an ended terminal drops an attached tab when it leaves the inventory", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			worktree := protocol.WorktreeID{1}
			running, ended := terminalTabsTestInfo(1, worktree, "running"), terminalTabsTestInfo(2, worktree, "running")
			root, source := terminalTabsFixture(t, running, ended)
			id := ended.ID.String()
			root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: running.ID.String()})
			root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
			root.terminalsFrame(nil)
			connection := terminalConnFixture(t, root, ended)
			stream := &pluginViewTestSubscription{closed: make(chan struct{})}
			connection.stream = stream
			root.packages.terminals[id] = &liveTerminal{owner: root, conn: connection}
			ended.Status = protocol.TerminalStatus{Kind: "exited"}
			source.snapshot.Terminals = []protocol.TerminalInfo{running, ended}
			if err := root.controller.Refresh(root.ctx); err != nil {
				t.Fatal(err)
			}
			synctest.Wait()
			root.terminalsFrame(nil)
			if got := terminalTabsTestTabs(root); !slices.Equal(got, []string{running.ID.String()}) {
				t.Fatalf("terminal tabs after exit = %v", got)
			}
			if root.packages.terminals[id] != nil || connection.ctx.Err() == nil {
				t.Fatal("the ended terminal stayed attached")
			}
			select {
			case <-stream.closed:
			default:
				t.Fatal("the ended terminal kept its subscription")
			}
			if _, exists := root.terminalInventory().nodes[id]; exists {
				t.Fatal("the ended terminal kept its inventory owner")
			}
			if len(source.sent("close_terminal"))+len(source.sent("open_terminal")) != 0 {
				t.Fatal("removing an ended terminal sent another terminal command")
			}
		})
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
		id, created := root.createLiveTerminal(root, worktree, protocol.ProcessAccessUnrestricted, terminalPanel)
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
		if _, created := root.createLiveTerminal(root, protocol.WorktreeID{}, protocol.ProcessAccessUnrestricted, terminalPanel); created {
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
			id := info.ID.String()
			root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
			root.App.Panel.Find()
			root.closeLiveTerminal(id)
			if !root.App.Panel.HasTerminalChrome(id) {
				t.Fatal("a pending close lost the terminal's chrome")
			}
		}
		synctest.Wait()
		pluginViewRunQueue(root)
		if got := terminalTabsTestTabs(root); len(got) != 0 {
			t.Fatalf("closed terminals kept their tabs: %v", got)
		}
		for _, info := range []protocol.TerminalInfo{first, second} {
			if root.App.Panel.HasTerminalChrome(info.ID.String()) {
				t.Fatalf("permanently closed terminal %s retained its chrome", info.ID)
			}
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

// testTerminalTabsClosePending covers a tab closed before create_terminal
// answered: the answer closes the terminal and the tab leaves.
func testTerminalTabsClosePending(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		worktree := protocol.WorktreeID{1}
		root, source := terminalTabsFixture(t)
		source.respond = func(request protocol.Request) link.Response {
			info := terminalTabsTestInfo(0, worktree, "running")
			if request.Command.Kind == "close_terminal" {
				payload, _ := detail[protocol.CommandCloseTerminalPayload](request.Command.Data)
				info.ID, info.Status = payload.Terminal, protocol.TerminalStatus{Kind: "closed"}
			} else {
				info.ID = protocol.TerminalID(request.ID)
			}
			return link.Response{Output: protocol.Output{Kind: "terminal", Data: info}}
		}
		id, created := root.createLiveTerminal(root, worktree, protocol.ProcessAccessUnrestricted, terminalPanel)
		if !created {
			t.Fatal("New terminal did not start")
		}
		root.closeLiveTerminal(id)
		if !slices.Contains(terminalTabsTestTabs(root), id) {
			t.Fatal("a pending terminal's tab left before its creation answered")
		}
		synctest.Wait()
		pluginViewRunQueue(root)
		synctest.Wait()
		pluginViewRunQueue(root)
		commands := source.sent("close_terminal")
		if len(commands) != 1 {
			t.Fatalf("close_terminal commands = %d, want 1", len(commands))
		}
		payload, valid := detail[protocol.CommandCloseTerminalPayload](commands[0].Data)
		if !valid || payload.Worktree != worktree || payload.Terminal.String() != id {
			t.Fatalf("unexpected close payload: %+v", payload)
		}
		if got := terminalTabsTestTabs(root); len(got) != 0 || root.packages.terminals[id] != nil || root.terminalInventory().pending[id] {
			t.Fatalf("a terminal closed while pending stayed: %v", got)
		}
	})
}

func TestRejectedTerminalClosePreservesPanelOnLaterExit(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		info := terminalTabsTestInfo(1, protocol.WorktreeID{1}, "running")
		root, source := terminalTabsFixture(t, info)
		source.respond = func(protocol.Request) link.Response {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "close denied"}}
		}
		id := info.ID.String()
		tab := panel.Tab{Kind: panel.Terminal, ID: id}
		root.App.RevealConversationPanel()
		root.App.Panel.Show(tab)
		root.App.Panel.OnCloseConfirmed(tab)
		root.closeLiveTerminal(id)
		synctest.Wait()
		root.mu.Lock()
		callbacks := root.queue
		root.queue = nil
		root.mu.Unlock()
		ui.NewTester(func(c *ui.Context) {
			for _, callback := range callbacks {
				callback(c)
			}
			callbacks = nil
		}, 400, 200).Frame()
		if len(source.sent("close_terminal")) != 1 || !root.App.HasPanelTab(tab) {
			t.Fatal("a rejected close must leave the terminal attached")
		}
		root.dropTerminalTab(id)
		if !root.App.PanelOpen || root.App.HasPanelTab(tab) {
			t.Fatal("a later passive exit folded the panel after a rejected close")
		}
	})
}

func TestProjectTerminalPlacement(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		worktree := protocol.WorktreeID{1}
		running := terminalTabsTestInfo(1, worktree, "running")
		running.Access = new(protocol.ProcessAccessWorkspace)
		root, source := terminalTabsFixture(t, running)
		source.respond = func(request protocol.Request) link.Response {
			launch, _ := detail[protocol.TerminalLaunch](request.Command.Data)
			info := terminalTabsTestInfo(0, launch.Worktree, "running")
			info.ID = protocol.TerminalID(request.ID)
			return link.Response{Output: protocol.Output{Kind: "terminal", Data: info}}
		}
		root.App.Panel.Show(panel.Tab{Kind: panel.Files})
		root.App.Sidebar.Actions = &sidebar.Actions{}
		root.initTerminalActions()
		project := &viewmodel.Project{ID: protocol.ProjectID{1}.String(), Host: root.controller.Target().String()}
		root.App.Sidebar.Actions.NewTerminal(project)
		ids := root.App.WorkspaceTerminalIDs()
		if len(ids) != 1 || len(root.App.AllPanelTabs()) != 1 || root.App.Panel.Active.Kind != panel.Files || root.App.PanelOpen {
			t.Fatalf("project creation changed the detail panel: workspace %v, tabs %v, open %v", ids, root.App.AllPanelTabs(), root.App.PanelOpen)
		}
		created := ids[0]
		root.terminalsFrame(nil)
		if !root.App.HasWorkspaceTerminal(created) || root.packages.terminals[created] == nil || !root.terminalInventory().pending[created] {
			t.Fatal("the workspace terminal detached before its creation snapshot arrived")
		}
		synctest.Wait()
		split, ok := root.splitLiveTerminal(running.ID.String())
		if !ok || !root.App.HasWorkspaceTerminal(split) || root.App.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: split}) {
			t.Fatal("splitting a workspace terminal opened a detail tab")
		}
		synctest.Wait()
		commands := source.sent("create_terminal")
		if len(commands) != 2 {
			t.Fatalf("create_terminal commands = %d, want 2", len(commands))
		}
		for i, want := range []protocol.ProcessAccess{protocol.ProcessAccessUnrestricted, protocol.ProcessAccessWorkspace} {
			launch, valid := detail[protocol.TerminalLaunch](commands[i].Data)
			if !valid || launch.Worktree != worktree || launch.Access != want {
				t.Fatalf("terminal launch %d = %+v, want worktree %s and access %s", i, launch, worktree, want)
			}
		}
	})
}

func TestWorkspaceTerminalLifecycle(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		info := terminalTabsTestInfo(1, protocol.WorktreeID{1}, "running")
		root, source := terminalTabsFixture(t, info)
		id := info.ID.String()
		connection := terminalConnFixture(t, root, info)
		stream := &pluginViewTestSubscription{closed: make(chan struct{})}
		connection.stream = stream
		state := &liveTerminal{owner: root, conn: connection}
		root.packages.terminals[id] = state
		root.showLiveTerminal(id)
		root.showWorkspaceTerminal(id)
		root.terminalsFrame(nil)
		if !root.App.HasWorkspaceTerminal(id) || root.App.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: id}) || root.packages.terminals[id] != state || connection.ctx.Err() != nil {
			t.Fatal("moving a terminal to the workspace lost its sole live attachment")
		}
		// A project surface has no conversation ownership. Removing the
		// previous chat's panel must not terminate or detach that terminal.
		root.App.Sidebar.Selected = "draft/removed"
		root.App.MountPanel()
		root.App.Sidebar.Selected = "draft/current"
		root.conversationPanelsFrame()
		root.terminalsFrame(nil)
		if !root.App.HasWorkspaceTerminal(id) || root.packages.terminals[id] != state || len(source.sent("close_terminal")) != 0 {
			t.Fatal("a removed conversation affected an independent project terminal")
		}
		root.showLiveTerminal(id)
		root.terminalsFrame(nil)
		if root.App.HasWorkspaceTerminal(id) || !root.App.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: id}) || root.packages.terminals[id] != state {
			t.Fatal("opening in details retained a second workspace surface")
		}
		root.showWorkspaceTerminal(id)
		root.App.DropWorkspaceTerminal(id)
		root.terminalsFrame(nil)
		if root.packages.terminals[id] != nil || connection.ctx.Err() == nil {
			t.Fatal("closing the final surface retained the terminal subscription")
		}
		select {
		case <-stream.closed:
		default:
			t.Fatal("closing the workspace surface left its stream open")
		}
		if len(source.sent("close_terminal")) != 0 {
			t.Fatal("closing a workspace surface stopped the Node terminal")
		}
		root.showWorkspaceTerminal(id)
		root.terminalsFrame(nil)
		if !root.App.HasWorkspaceTerminal(id) {
			t.Fatal("the running project terminal could not be reopened")
		}
		info.Status.Kind = "closed"
		source.snapshot.Terminals = []protocol.TerminalInfo{info}
		if err := root.controller.Refresh(root.ctx); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()
		root.terminalsFrame(nil)
		if root.App.HasWorkspaceTerminal(id) || root.terminalInventory().pending[id] {
			t.Fatal("the Node-closed terminal retained its workspace surface")
		}
	})
}

func TestDetachedTerminalChromeLifetime(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		info := terminalTabsTestInfo(1, protocol.WorktreeID{1}, "running")
		root, source := terminalTabsFixture(t, info)
		id := info.ID.String()
		app := root.App
		app.Panel.TerminalActions = &panel.TerminalActions{Close: root.closeLiveTerminal}
		app.Sidebar.Selected = "draft/first"
		root.showLiveTerminal(id)
		if !app.Panel.Find() {
			t.Fatal("the terminal's search bar did not open")
		}
		app.Sidebar.Selected = "draft/second"
		app.MountPanel()
		root.terminalsFrame(nil)
		if len(app.Panel.Tabs) != 0 || !app.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: id}) || !app.Panel.HasTerminalChrome(id) {
			t.Fatal("parking the terminal lost its tab or its chrome")
		}

		root.showWorkspaceTerminal(id)
		app.DropWorkspaceTerminal(id)
		root.terminalsFrame(nil)
		root.showLiveTerminal(id)
		if !app.Panel.HasTerminalChrome(id) {
			t.Fatal("reopening the detached live terminal lost its chrome")
		}
		app.Panel.Clear()
		root.terminalsFrame(nil)
		if !app.Panel.HasTerminalChrome(id) || len(source.sent("close_terminal")) != 0 {
			t.Fatal("detaching a live terminal lost its chrome or stopped its shell")
		}

		source.snapshot.Terminals = nil
		if err := root.controller.Refresh(root.ctx); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()
		root.terminalsFrame(nil)
		if app.Panel.HasTerminalChrome(id) || len(source.sent("close_terminal")) != 0 {
			t.Fatal("the detached terminal kept its chrome or received a close command after it exited")
		}
	})
}

func TestWorkspaceTerminalKeepsHiddenConversationUnread(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		root, source := terminalTabsFixture(t)
		session := protocol.DefaultSession()
		session.ID = protocol.SessionID{1}
		session.Activity.Attention.Unread = true
		session.Activity.Attention.Revision = 7
		source.snapshot.Sessions = []protocol.Session{session}
		if err := root.controller.Refresh(root.ctx); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()
		id := session.ID.String()
		root.App.Data.Unassigned = []*viewmodel.Conversation{{ID: id, Node: root.controller.Target().String()}}
		root.App.Sidebar.Selected = id
		root.App.OpenWorkspaceTerminal(protocol.TerminalID{1}.String(), "")
		root.attentionFrame()
		synctest.Wait()
		if len(source.sent("set_session_read")) != 0 || root.attachmentConversationVisible(id) {
			t.Fatal("a standalone terminal treated its hidden conversation as visible")
		}
		root.App.DropWorkspaceTerminal(protocol.TerminalID{1}.String())
		root.attentionFrame()
		synctest.Wait()
		if len(source.sent("set_session_read")) != 1 || !root.attachmentConversationVisible(id) {
			t.Fatal("returning to the conversation did not restore visibility")
		}
	})
}

// A terminal another controller started in the project's worktree, such as
// one opened from Mobile, is listed by the project overview and opens here
// without starting another shell.
func TestProjectOverviewListsNodeTerminals(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		project := protocol.DefaultProject()
		project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Terminal project", "/fixture"
		main, scratch := protocol.WorktreeID{1}, protocol.WorktreeID{2}
		remote := terminalTabsTestInfo(1, main, "running")
		remote.Title, remote.Owner = new("zsh"), &protocol.NodeID{9}
		exited := terminalTabsTestInfo(2, main, "exited")
		chat := terminalTabsTestInfo(3, scratch, "running")
		tool := terminalTabsTestInfo(4, main, "running")
		tool.Tool = new(protocol.TerminalToolsTool)
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Projects = protocol.NodeID{1}, []protocol.Project{project}
		snapshot.Worktrees = []protocol.Worktree{
			{ID: main, Project: &project.ID, Path: project.Path, Main: true},
			{ID: scratch, Path: "/scratch"},
		}
		snapshot.Terminals = []protocol.TerminalInfo{remote, exited, chat, tool}
		root, source := snapshotFixture(t, snapshot)
		overview := &viewmodel.Project{ID: project.ID.String(), Host: root.controller.Target().String()}
		listed := root.projectTerminals(overview)
		want := []shell.ProjectTerminal{{ID: remote.ID.String(), Title: "zsh", Path: "/fixture"}}
		if !slices.Equal(listed, want) {
			t.Fatalf("project terminals = %+v, want %+v", listed, want)
		}
		root.showWorkspaceTerminal(listed[0].ID)
		if !root.App.HasWorkspaceTerminal(remote.ID.String()) {
			t.Fatal("opening a listed terminal did not show it in the workspace")
		}
		synctest.Wait()
		if commands := source.sent("create_terminal"); len(commands) != 0 {
			t.Fatalf("opening a listed terminal started %d shells", len(commands))
		}
	})
}
