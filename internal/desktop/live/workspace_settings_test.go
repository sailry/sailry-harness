package live

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceHookSettings(t *testing.T) {
	t.Run("terminal settings projection", checkTerminalSettingsHelpers)
	t.Run("local model projection", checkLocalModelHelpers)
	t.Run("dictation", checkDictation)
	t.Run("terminal tabs prune", testTerminalTabsPrune)
	t.Run("terminal tabs create", testTerminalTabsCreate)
	t.Run("terminal tabs close", testTerminalTabsClose)
	t.Run("terminal tabs project", testTerminalTabsProject)
	t.Run("browser address", testBrowserAddress)
	t.Run("browser capture", testBrowserCapture)
	t.Run("browser tab", testBrowserTab)
	t.Run("remote settings keep their owner through navigation and completion", func(t *testing.T) {
		owners := make([]*Runtime, 2)
		transports := make([]*conversationTestTransport, 2)
		data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}}
		app := shell.NewLive(data)
		for index := range owners {
			node, project, worktree, session := protocol.NodeID{byte(index + 1)}, protocol.NewProjectID(), protocol.NewWorktreeID(), protocol.NewSessionID()
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = node
			snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/owner"}}
			snapshot.Sessions = []protocol.Session{{ID: session, Project: &project, Worktree: worktree}}
			transports[index] = &conversationTestTransport{snapshot: snapshot}
			controller := client.New(transports[index])
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			hooks := &settings.Stored[settings.HookSettings]{Revision: uint64(index + 7)}
			owners[index] = &Runtime{
				ctx: t.Context(), controller: controller, App: app,
				busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
				workspaceSettings: workspaceSettingsState{
					worktree: worktree, sandboxKnown: true,
					hookModels:   map[protocol.WorktreeID]*settings.Stored[settings.HookSettings]{worktree: hooks},
					sandboxModel: settings.Stored[bool]{Revision: uint64(index + 3)},
				},
			}
			data.Projects = append(data.Projects, &viewmodel.Project{ID: project.String(), Host: node.String(), Conversations: []*viewmodel.Conversation{{ID: session.String(), Node: node.String()}}})
		}
		local, remote := owners[0], owners[1]
		remote.parent = local
		local.bindings = map[string]*Runtime{remote.controller.Target().String(): remote}
		local.bindWorkspaceSettings()
		localSession, remoteSession := transports[0].snapshot.Sessions[0].ID.String(), transports[1].snapshot.Sessions[0].ID.String()
		local.OpenSession(remoteSession)
		local.workspaceSettingsFrame()
		if label, ok := app.Settings.HooksContext(); !ok || !strings.Contains(label, remote.controller.Target().String()) || app.Settings.Hooks.Revision != 8 {
			t.Fatal("remote hooks read and consent did not share an execution owner")
		}
		app.Settings.Hooks.Draft.Enabled = true
		app.Settings.SaveHooks()
		app.Settings.Sandbox.Draft = true
		app.Settings.SaveSandbox()
		local.OpenSession(localSession)
		local.workspaceSettingsFrame()
		remote.workers.Wait()
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			local.mu.Lock()
			queue := local.queue
			local.queue = nil
			local.mu.Unlock()
			for _, complete := range queue {
				complete(c)
			}
		}, 640, 480)
		tester.Frame()
		if app.Settings.Hooks.Revision != 7 || app.Settings.Hooks.Draft.Enabled || app.Settings.Sandbox.Revision != 3 || app.Settings.Sandbox.Draft {
			t.Fatal("remote completion overwrote the local settings draft")
		}
		transports[1].mu.Lock()
		requests := append([]protocol.Request(nil), transports[1].requests...)
		transports[1].mu.Unlock()
		if len(requests) != 2 {
			t.Fatalf("remote settings dispatched %d requests", len(requests))
		}
		for _, request := range requests {
			if request.Target != remote.controller.Target() {
				t.Fatal("remote settings save escaped its execution Node")
			}
			if request.Command.Kind == "save_hooks" {
				input, valid := detail[protocol.CommandSaveHooksPayload](request.Command.Data)
				if !valid || input.Worktree != remote.workspaceSettings.worktree || input.ExpectedRevision != 8 || !input.Settings.Enabled {
					t.Fatal("remote hook save lost its captured worktree, revision or draft")
				}
			}
		}
		if controller, ok := local.SessionClient(remoteSession); !ok || controller != remote.controller {
			t.Fatal("export lost the remote session owner after local navigation")
		}
		local.OpenSession(remoteSession)
		local.workspaceSettingsFrame()
		if !app.Settings.Hooks.Draft.Enabled || !app.Settings.Sandbox.Draft || app.Settings.Hooks.Pending || app.Settings.Sandbox.Pending {
			t.Fatal("navigation lost the retained remote settings draft")
		}
	})
	for _, admitted := range []bool{false, true} {
		name := "snapshot worktree"
		if admitted {
			name = "newly admitted worktree"
		}
		t.Run(name, func(t *testing.T) {
			node, project, worktree := protocol.NodeID{1}, protocol.NewProjectID(), protocol.NewWorktreeID()
			tree := protocol.Worktree{ID: worktree, Project: &project, Path: "/fixture/hooks", Main: true}
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = node
			if !admitted {
				snapshot.Worktrees = []protocol.Worktree{tree}
			}
			controller := client.New(&conversationTestTransport{snapshot: snapshot})
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			const draftID = "draft/hooks"
			data := &viewmodel.Data{
				Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{{ID: node.String(), Local: true}}},
				Projects:  []*viewmodel.Project{{ID: project.String(), Host: node.String(), Conversations: []*viewmodel.Conversation{{ID: draftID, Node: node.String()}}}},
			}
			r := &Runtime{controller: controller, App: shell.NewLive(data), workspace: &workspaceState{admitted: map[protocol.WorktreeID]protocol.Worktree{}}}
			if admitted {
				r.workspace.admitted[worktree] = tree
			}
			r.App.Sidebar.Selected = draftID
			r.App.Composer(draftID).Worktree = tree.Path
			r.workspaceSettings = workspaceSettingsState{worktree: worktree, sandboxed: true, sandboxKnown: true}
			r.App.Settings.Hooks = settings.Stored[settings.HookSettings]{Revision: 7}
			r.workspaceSettingsFrame()
			if r.workspaceSettings.worktree != worktree || r.App.Settings.Hooks.Revision != 7 || !r.hooksSandboxed() {
				t.Fatal("project draft lost its captured hook owner or settings")
			}
			label, ok := r.hookContext()
			if !ok || !strings.Contains(label, node.String()) || !strings.Contains(label, tree.Path) || !strings.Contains(label, worktree.String()) {
				t.Fatalf("hook consent scope = %q, available=%v", label, ok)
			}
			delete(r.workspace.admitted, worktree)
			r.App.Data.Projects = nil
			r.App.Data.Unassigned = []*viewmodel.Conversation{{ID: "draft/unassigned", Node: node.String()}}
			r.App.Sidebar.Selected = "draft/unassigned"
			r.workspaceSettingsFrame()
			if r.workspaceSettings.worktree != (protocol.WorktreeID{}) || r.hooksSandboxed() || r.App.Settings.Hooks.Revision != 0 {
				t.Fatal("unassigned draft retained project hook authority")
			}
			if _, ok := r.hookContext(); ok {
				t.Fatal("unassigned draft exposed project hook consent")
			}
		})
	}
}
