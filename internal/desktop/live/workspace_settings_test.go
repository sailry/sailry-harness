package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceHookSettings(t *testing.T) {
	t.Run("terminal settings projection", checkTerminalSettingsHelpers)
	t.Run("backup settings projection", checkBackupSettingsHelpers)
	t.Run("local model projection", checkLocalModelHelpers)
	t.Run("speech model library", checkSpeechLibrary)
	t.Run("knowledge collections projection", checkKnowledgeViews)
	t.Run("dictation", checkDictation)
	t.Run("system settings projection", checkSystemHelpers)
	t.Run("feature usage", testFeatureUsage)
	t.Run("terminal tabs prune", testTerminalTabsPrune)
	t.Run("terminal tabs create", testTerminalTabsCreate)
	t.Run("terminal tabs close", testTerminalTabsClose)
	t.Run("terminal tabs close pending", testTerminalTabsClosePending)
	t.Run("SSH host install", testSSHHostInstall)
	t.Run("browser address", testBrowserAddress)
	t.Run("browser capture", testBrowserCapture)
	t.Run("browser call deadline", testBrowserDeadline)
	t.Run("project hooks stay remote while sandbox remains local", func(t *testing.T) {
		alternate := protocol.NewWorktreeID()
		owners := make([]*Runtime, 2)
		transports := make([]*conversationTestTransport, 2)
		data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}}
		app := shell.NewLive(data)
		for index := range owners {
			node, project, worktree, session := protocol.NodeID{byte(index + 1)}, protocol.NewProjectID(), protocol.NewWorktreeID(), protocol.NewSessionID()
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = node
			snapshot.SandboxSettingsRevision = uint64(index + 3)
			snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/owner"}}
			if index == 1 {
				snapshot.Worktrees = append(snapshot.Worktrees, protocol.Worktree{ID: alternate, Project: &project, Path: "/fixture/alternate"})
			}
			snapshot.Sessions = []protocol.Session{{ID: session, Project: &project, Worktree: worktree}}
			transports[index] = &conversationTestTransport{snapshot: snapshot}
			transports[index].handle = func(request protocol.Request) (link.Response, bool) {
				switch request.Command.Kind {
				case "read_hooks":
					hooks := protocol.DefaultHooksSettings()
					hooks.Revision = 9
					return link.Response{Output: protocol.Output{Kind: "hooks", Data: protocol.HooksState{Settings: hooks, Sandboxed: true}}}, true
				case "save_hooks":
					input, _ := detail[protocol.CommandSaveHooksPayload](request.Command.Data)
					input.Settings.Revision++
					return link.Response{Output: protocol.Output{Kind: "hooks", Data: protocol.HooksState{Settings: input.Settings, Sandboxed: true}}}, true
				case "save_sandbox_settings":
					input, _ := detail[protocol.SandboxSettings](request.Command.Data)
					input.Revision++
					return link.Response{Output: protocol.Output{Kind: "sandbox_settings", Data: input}}, true
				}
				return link.Response{}, false
			}
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
					worktree: worktree, sandboxKnown: true, sandboxRevision: snapshot.SandboxSettingsRevision,
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
		// Project selection owns hooks independently of the monitored device.
		localHost, remoteHost := local.controller.Target().String(), remote.controller.Target().String()
		app.Host = localHost
		local.OpenSession(remoteSession)
		local.workspaceSettingsFrame()
		if target, ok := app.Settings.HooksContext(); !ok || target.Host != remoteHost || app.Settings.Hooks.Revision != 8 || app.Settings.Sandbox.Revision != 3 {
			t.Fatal("remote project hooks or local sandbox lost their owner")
		}
		app.Host = remoteHost
		local.workspaceSettingsFrame()
		if app.Settings.Sandbox.Revision != 3 || app.Settings.Hooks.Revision != 8 {
			t.Fatal("monitor changed settings ownership")
		}

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
		app.Settings.SelectHookWorktree(alternate.String())
		remote.workers.Wait()
		tester.Frame()
		if target, ok := app.Settings.HooksContext(); !ok || target.Worktree != alternate.String() || app.Settings.Hooks.Revision != 9 {
			t.Fatal("the worktree picker did not load the chosen worktree")
		}
		app.Settings.Hooks.Draft.Enabled = true
		app.Settings.SaveHooks()
		app.Settings.Sandbox.Draft = true
		app.Settings.SaveSandbox()
		app.Host = localHost
		local.OpenSession(localSession)
		local.workspaceSettingsFrame()
		remote.workers.Wait()
		local.workers.Wait()
		tester.Frame()
		if app.Settings.Hooks.Revision != 7 || app.Settings.Hooks.Draft.Enabled || app.Settings.Sandbox.Revision != 4 || !app.Settings.Sandbox.Draft {
			t.Fatal("remote hook completion overwrote local hooks or sandbox completion")
		}
		transports[1].mu.Lock()
		requests := append([]protocol.Request(nil), transports[1].requests...)
		transports[1].mu.Unlock()
		if len(requests) != 2 {
			t.Fatalf("remote hooks dispatched %d requests", len(requests))
		}
		for _, request := range requests {
			if request.Target != remote.controller.Target() {
				t.Fatal("remote settings save escaped its execution Node")
			}
			switch request.Command.Kind {
			case "read_hooks":
				input, valid := detail[protocol.CommandReadHooksPayload](request.Command.Data)
				if !valid || input.Worktree != alternate {
					t.Fatal("the hooks page read a different worktree")
				}
			case "save_hooks":
				input, valid := detail[protocol.CommandSaveHooksPayload](request.Command.Data)
				if !valid || input.Worktree != alternate || input.ExpectedRevision != 9 || !input.Settings.Enabled {
					t.Fatal("remote hook save lost its captured worktree, revision or draft")
				}
			default:
				t.Fatalf("unexpected settings command %s on the remote Node", request.Command.Kind)
			}
		}
		transports[0].mu.Lock()
		localRequests := append([]protocol.Request(nil), transports[0].requests...)
		transports[0].mu.Unlock()
		if len(localRequests) != 1 || localRequests[0].Command.Kind != "save_sandbox_settings" || localRequests[0].Target != local.controller.Target() {
			t.Fatalf("sandbox save escaped local Node: %+v", localRequests)
		}
		if controller, ok := local.SessionClient(remoteSession); !ok || controller != remote.controller {
			t.Fatal("export lost the remote session owner after local navigation")
		}
		app.Host = remoteHost
		local.OpenSession(remoteSession)
		local.workspaceSettingsFrame()
		if !app.Settings.Hooks.Draft.Enabled || !app.Settings.Sandbox.Draft || app.Settings.Hooks.Pending || app.Settings.Sandbox.Pending {
			t.Fatal("navigation lost the retained remote hook or local sandbox draft")
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
			target, ok := r.hookContext()
			if !ok || target.Host != "composer_host_local" || target.Path != tree.Path || target.Worktree != worktree.String() {
				t.Fatalf("hook consent scope = %+v, available=%v", target, ok)
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
