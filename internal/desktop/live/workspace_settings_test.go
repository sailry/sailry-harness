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
	t.Run("explicit Hook targets work without a selected project", func(t *testing.T) {
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
			snapshot.Projects = []protocol.Project{{ID: project, Name: "Fixture"}}
			snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/owner"}, {ID: alternate, Project: &project, Path: "/fixture/alternate"}}
			snapshot.Sessions = []protocol.Session{{ID: session, Project: &project, Worktree: worktree}}
			mainHooks, alternateHooks := protocol.DefaultHooksSettings(), protocol.DefaultHooksSettings()
			mainHooks.Revision, alternateHooks.Revision = uint64(index+7), 9
			stored := map[protocol.WorktreeID]protocol.HooksSettings{worktree: mainHooks, alternate: alternateHooks}
			transports[index] = &conversationTestTransport{snapshot: snapshot}
			transports[index].handle = func(request protocol.Request) (link.Response, bool) {
				switch request.Command.Kind {
				case "read_hooks":
					input, _ := detail[protocol.CommandReadHooksPayload](request.Command.Data)
					return link.Response{Output: protocol.Output{Kind: "hook_settings", Data: protocol.HooksState{Worktree: input.Worktree, Settings: stored[input.Worktree], Sandboxed: true}}}, true
				case "save_hooks":
					input, _ := detail[protocol.CommandSaveHooksPayload](request.Command.Data)
					input.Settings.Revision++
					stored[input.Worktree] = input.Settings
					return link.Response{Output: protocol.Output{Kind: "hook_settings", Data: protocol.HooksState{Worktree: input.Worktree, Settings: input.Settings, Sandboxed: true}}}, true
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
			owners[index] = &Runtime{
				ctx: t.Context(), controller: controller, App: app,
				busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
				workspaceSettings: workspaceSettingsState{
					sandboxKnown: true, sandboxRevision: snapshot.SandboxSettingsRevision,
					sandboxModel: settings.Stored[bool]{Revision: uint64(index + 3)},
					hookModels:   map[protocol.WorktreeID]*settings.Stored[settings.HookSettings]{},
				},
			}
			data.Projects = append(data.Projects, &viewmodel.Project{ID: project.String(), Host: node.String(), Conversations: []*viewmodel.Conversation{{ID: session.String(), Node: node.String()}}})
		}
		local, remote := owners[0], owners[1]
		remote.parent = local
		localHost, remoteHost := local.controller.Target().String(), remote.controller.Target().String()
		local.bindings = map[string]*Runtime{remoteHost: remote}
		localSession, remoteSession := transports[0].snapshot.Sessions[0].ID.String(), transports[1].snapshot.Sessions[0].ID.String()
		m := app.Settings
		m.ManagedNode = "unrelated-settings-host"
		showHooks := func() {
			app.Page = shell.SettingsPage
			m.Open(settings.Hooks)
			local.workspaceSettingsFrame()
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
		settle := func() {
			remote.workers.Wait()
			local.workers.Wait()
			tester.Frame()
		}
		showHooks()
		if _, available := m.HooksContext(); available || m.SaveHooks != nil {
			t.Fatal("opening Hooks without a selected project exposed a save target")
		}
		choices := m.HookTargets()
		var localTarget, remoteTarget settings.HookTarget
		for _, target := range choices {
			if target.Worktree == alternate.String() {
				switch target.Node {
				case localHost:
					localTarget = target
				case remoteHost:
					remoteTarget = target
				}
			}
		}
		if len(choices) != 4 || localTarget.Node != localHost || remoteTarget.Node != remoteHost || !m.SelectHookTarget(remoteTarget) {
			t.Fatalf("the picker did not distinguish matching worktree IDs by Node: %+v", choices)
		}
		settle()
		if target, ok := m.HooksContext(); !ok || target != remoteTarget || m.Hooks.Revision != 9 || m.Sandbox.Revision != 3 {
			t.Fatal("the picker did not load its chosen worktree independently of sandbox settings")
		}
		capturedSave := m.SaveHooks
		draft := m.Hooks.Draft
		draft.Enabled = true
		capturedSave(draft)
		m.Sandbox.Draft = true
		m.SaveSandbox()
		app.Host = remoteHost
		local.OpenSession(localSession)
		showHooks()
		if target, ok := m.HooksContext(); !ok || target != remoteTarget || m.ManagedNode != "unrelated-settings-host" {
			t.Fatal("chat, the monitor, or machine settings changed an explicit Hooks target")
		}
		if !m.SelectHookTarget(settings.HookTarget{}) {
			t.Fatal("could not restore the automatic worktree target")
		}
		capturedSave(draft)
		settle()
		if m.Hooks.Revision != 7 || m.Hooks.Draft.Enabled || m.Sandbox.Revision != 4 || !m.Sandbox.Draft {
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
		localReads, localSandbox := 0, 0
		for _, request := range localRequests {
			switch request.Command.Kind {
			case "read_hooks":
				payload, _ := detail[protocol.CommandReadHooksPayload](request.Command.Data)
				if payload.Worktree == transports[0].snapshot.Worktrees[0].ID {
					localReads++
				}
			case "save_sandbox_settings":
				if request.Target == local.controller.Target() {
					localSandbox++
				}
			}
		}
		if len(localRequests) != 2 || localReads != 1 || localSandbox != 1 {
			t.Fatalf("a stale hook save reached the local Node, its automatic target changed, or sandbox was retargeted: %+v", localRequests)
		}
		if controller, ok := local.SessionClient(remoteSession); !ok || controller != remote.controller {
			t.Fatal("export lost the remote session owner after local navigation")
		}
		local.OpenSession(remoteSession)
		showHooks()
		settle()
		if target, ok := m.HooksContext(); !ok || target.Node != remoteHost || target.Worktree == alternate.String() || m.Hooks.Revision != 8 {
			t.Fatal("automatic Hooks target did not follow the current remote worktree")
		}
		m.SelectHookTarget(remoteTarget)
		settle()
		delete(local.bindings, remoteHost)
		showHooks()
		if target, ok := m.HooksContext(); ok || target != remoteTarget || m.SaveHooks != nil || len(m.HookTargets()) != 2 {
			t.Fatal("an unavailable target disappeared or fell back to another Node")
		}
		local.bindings[remoteHost] = remote
		showHooks()
		if !m.Hooks.Pending {
			t.Fatal("reconnecting an explicit Hooks target did not start a fresh read")
		}
		settle()
		if !m.Hooks.Draft.Enabled || m.Hooks.Revision != 10 || m.Hooks.Pending || !m.Sandbox.Draft {
			t.Fatal("reconnecting lost the intended worktree's state or the local sandbox draft")
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
			transport := &conversationTestTransport{snapshot: snapshot}
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind != "read_hooks" {
					return link.Response{}, false
				}
				hooks := protocol.DefaultHooksSettings()
				hooks.Revision = 7
				return link.Response{Output: protocol.Output{Kind: "hook_settings", Data: protocol.HooksState{Worktree: worktree, Settings: hooks, Sandboxed: true}}}, true
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			const draftID = "draft/hooks"
			data := &viewmodel.Data{
				Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{{ID: node.String(), Local: true}}},
				Projects:  []*viewmodel.Project{{ID: project.String(), Host: node.String(), Conversations: []*viewmodel.Conversation{{ID: draftID, Node: node.String()}}}},
			}
			r := &Runtime{ctx: t.Context(), controller: controller, App: shell.NewLive(data), workspace: &workspaceState{admitted: map[protocol.WorktreeID]protocol.Worktree{}}, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
			if admitted {
				r.workspace.admitted[worktree] = tree
			}
			r.App.Sidebar.Selected = draftID
			r.App.Composer(draftID).Worktree = tree.Path
			r.App.Page = shell.SettingsPage
			r.App.Settings.Open(settings.Hooks)
			r.workspaceSettings = workspaceSettingsState{sandboxKnown: true}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				r.mu.Lock()
				queue := r.queue
				r.queue = nil
				r.mu.Unlock()
				for _, complete := range queue {
					complete(c)
				}
			}, 640, 480)
			r.workspaceSettingsFrame()
			r.workers.Wait()
			tester.Frame()
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
			if r.workspaceSettingsOwner != nil || r.App.Settings.Hooks.Revision != 0 || r.App.Settings.SaveHooks != nil {
				t.Fatal("unassigned draft retained project hook authority")
			}
			if _, ok := r.App.Settings.HooksContext(); ok {
				t.Fatal("unassigned draft exposed project hook consent")
			}
		})
	}
}
