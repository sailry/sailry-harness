package live

import (
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
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
	t.Run("hooks use the settings host independently of the conversation", testHostHooksSettings)
}

func testHostHooksSettings(t *testing.T) {
	app := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, desktop.Harness())
	fixture := func(index byte, administrator bool) (*Runtime, *settingsTestTransport) {
		t.Helper()
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = protocol.NodeID{index}
		snapshot.User = &protocol.User{ID: protocol.UserID{index}, Administrator: administrator}
		stored := protocol.DefaultHooksSettings()
		stored.Revision = uint64(index + 6)
		transport := &settingsTestTransport{conversationTestTransport: &conversationTestTransport{snapshot: snapshot}}
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			switch request.Command.Kind {
			case "read_hooks":
				return link.Response{Output: protocol.Output{Kind: "hook_settings", Data: protocol.HooksState{Settings: stored, Sandboxed: true}}}, true
			case "save_hooks":
				input, _ := detail[protocol.CommandSaveHooksPayload](request.Command.Data)
				if input.ExpectedRevision != stored.Revision {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}, true
				}
				stored = input.Settings
				stored.Revision++
				return link.Response{Output: protocol.Output{Kind: "hook_settings", Data: protocol.HooksState{Settings: stored, Sandboxed: true}}}, true
			}
			return link.Response{}, false
		}
		controller := client.New(transport)
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = controller.Close() })
		return &Runtime{
			ctx: t.Context(), controller: controller, App: app,
			busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
		}, transport
	}
	local, localSource := fixture(1, true)
	remote, remoteSource := fixture(2, true)
	viewer, viewerSource := fixture(3, false)
	localNode, remoteNode, viewerNode := local.controller.Target().String(), remote.controller.Target().String(), viewer.controller.Target().String()
	remote.parent, viewer.parent = local, local
	local.bindings = map[string]*Runtime{remoteNode: remote, viewerNode: viewer}
	const draftID = "draft/unassigned"
	app.Data.Unassigned = []*viewmodel.Conversation{{ID: draftID, Node: localNode}}
	app.Sidebar.Selected = draftID
	app.Page = shell.SettingsPage
	m := app.Settings
	m.Open(settings.Hooks)
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
		local.workers.Wait()
		remote.workers.Wait()
		viewer.workers.Wait()
		tester.Frame()
	}
	show := func(node string) {
		m.ManagedNode = node
		local.hooksSettingsFrame()
		settle()
	}
	assertHost := func(node string, revision uint64) {
		t.Helper()
		target, ok := m.HooksContext()
		if !ok || target.Node != node || m.Hooks.Revision != revision || m.Hooks.Pending || m.HooksStatus != "" || m.SaveHooks == nil || !m.HooksSandboxed() {
			t.Fatalf("incorrect hook scope for %s: target %+v, model %+v, status %q", node, target, m.Hooks, m.HooksStatus)
		}
	}
	show("")
	assertHost(localNode, 7)
	show(remoteNode)
	assertHost(remoteNode, 8)
	stale := m.SaveHooks
	settingsDraft := settings.HookSettings{Enabled: true, Scripts: []viewmodel.Hook{{
		ID: "fixture", Name: "Fixture", Command: "printf fixture", Phase: viewmodel.BeforeTool, TimeoutMS: 500, Enabled: true,
	}}}
	if !m.SaveHooks(settingsDraft) {
		t.Fatal("host did not admit the hook save")
	}
	show("")
	assertHost(localNode, 7)
	if m.Hooks.Draft.Enabled || len(m.Hooks.Draft.Scripts) != 0 || app.Sidebar.Selected != draftID {
		t.Fatal("remote save changed the local hook settings or the current conversation")
	}
	if stale(settings.HookSettings{}) {
		t.Fatal("a stale hook action was accepted")
	}
	show(remoteNode)
	assertHost(remoteNode, 9)
	if !m.Hooks.Draft.Enabled || len(m.Hooks.Draft.Scripts) != 1 || m.Hooks.Draft.Scripts[0].Name != "Fixture" {
		t.Fatalf("the host did not retain its hook settings: %+v", m.Hooks.Draft)
	}

	delete(local.bindings, remoteNode)
	show(remoteNode)
	if target, available := m.HooksContext(); available || target.Node != remoteNode || m.SaveHooks != nil {
		t.Fatalf("disconnected host fell back to another target: %+v", target)
	}
	local.bindings[remoteNode] = remote
	local.hooksSettingsFrame()
	if !m.Hooks.Pending {
		t.Fatal("reconnected host did not start a fresh read")
	}
	settle()
	assertHost(remoteNode, 9)

	// The script editor retains its original save action while the same
	// authenticated host goes offline. It can save after a fresh read at
	// the draft's revision, but not once that revision changes.
	draftSave := m.SaveHooks
	refresh := func(code protocol.ErrorCode) error {
		remoteSource.mu.Lock()
		remoteSource.nodeFault = code
		remoteSource.mu.Unlock()
		return remote.controller.Refresh(t.Context())
	}
	if err := refresh(protocol.ErrorCodeUnavailable); faultCode(err) != protocol.ErrorCodeUnavailable {
		t.Fatalf("could not simulate a hook host outage: %v", err)
	}
	local.hooksSettingsFrame()
	if m.HooksStatus != "connections_offline" || draftSave(settingsDraft) {
		t.Fatal("the hook editor was writable while its host was offline")
	}
	if err := refresh(""); err != nil {
		t.Fatal(err)
	}
	local.hooksSettingsFrame()
	if !m.Hooks.Pending || draftSave(settingsDraft) {
		t.Fatal("the editor did not wait for its host's current hook revision")
	}
	settle()
	assertHost(remoteNode, 9)
	retained := settings.HookSettings{Scripts: []viewmodel.Hook{{
		ID: "fixture", Name: "Edited while offline", Command: "printf edit", Phase: viewmodel.BeforeTool, TimeoutMS: 500, Enabled: true,
	}}}
	if !draftSave(retained) {
		t.Fatal("the unsaved script draft could not save after its host recovered")
	}
	settle()
	assertHost(remoteNode, 10)
	if len(m.Hooks.Saved.Scripts) != 1 || m.Hooks.Saved.Scripts[0].Name != retained.Scripts[0].Name || draftSave(settingsDraft) {
		t.Fatal("the returning host lost the edit or accepted a stale revision")
	}

	for _, blocked := range []struct{ node, status string }{{viewerNode, "permission_denied"}, {"offline-host", "connections_offline"}} {
		show(blocked.node)
		target, allowed := m.HooksContext()
		if target.Node != blocked.node || allowed || m.SaveHooks != nil || m.HooksStatus != blocked.status || m.Hooks.Revision != 0 {
			t.Fatalf("unavailable host exposed editable hook settings: target %+v, status %q", target, m.HooksStatus)
		}
	}

	saves := 0
	for _, request := range remoteSource.requests {
		switch request.Command.Kind {
		case "read_hooks":
			if request.Command.Data != nil {
				t.Fatal("host read sent a resource payload")
			}
		case "save_hooks":
			saves++
			input, valid := detail[protocol.CommandSaveHooksPayload](request.Command.Data)
			if !valid || request.Target != remote.controller.Target() || input.ExpectedRevision != uint64(7+saves) {
				t.Fatalf("hook save used the wrong host or revision: %+v", request)
			}
		default:
			t.Fatalf("the hooks page sent %q", request.Command.Kind)
		}
	}
	if saves != 2 || len(viewerSource.requests) != 0 {
		t.Fatalf("unexpected hook writes or unauthorized reads: %d %d", saves, len(viewerSource.requests))
	}
	for _, request := range localSource.requests {
		if request.Command.Kind != "read_hooks" {
			t.Fatalf("remote hook action reached the local Node: %s", request.Command.Kind)
		}
	}
}
