package live

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testNativeComponentAuthority(t *testing.T) {
	reference := protocol.PluginReference{Name: "native-fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 3}
	actions := []protocol.PluginAction{protocol.PluginActionReadTerminals, protocol.PluginActionControlTerminals}
	for _, current := range []struct {
		name    string
		surface string
		actions []protocol.PluginAction
		enabled bool
		kind    string
		allowed bool
	}{
		{"workspace terminal", "workspace", actions, true, "compatible", true},
		{"composer", "composer", actions, true, "compatible", false},
		{"project", "project", actions, true, "compatible", false},
		{"settings", "settings", actions, true, "compatible", false},
		{"read only", "workspace", actions[:1], true, "compatible", false},
		{"control only", "workspace", actions[1:], true, "compatible", false},
		{"disabled", "workspace", actions, false, "compatible", false},
		{"incompatible", "workspace", actions, true, "incompatible", false},
	} {
		t.Run(current.name, func(t *testing.T) {
			native := plugins.NewNativeBinding(t.Context())
			scope := protocol.DefaultPluginContext()
			scope.Package, scope.Surface = reference, protocol.PluginDesktopSurface{Kind: current.surface}
			info := protocol.PluginInfo{Extension: &protocol.PluginExtension{Actions: current.actions}}
			info.Summary.Name, info.Summary.Digest = reference.Name, reference.Digest
			info.Summary.SettingsRevision, info.Summary.Enabled = reference.SettingsRevision, current.enabled
			info.Summary.Compatibility.Kind = current.kind
			state := pluginNativeState{
				scope: scope, native: &native,
				runtime: &Runtime{packages: &packageState{infos: map[string]protocol.PluginInfo{reference.Name: info}}},
			}
			if err := state.workspaceComponent(actions...); (err == nil) != current.allowed {
				t.Fatalf("native action gate changed: %v", err)
			}
		})
	}
	t.Run("native component IDs remain distinct without explicit keys", func(t *testing.T) {
		first, _, err := pluginNativeProps(plugins.NativeElement{ID: 1, Props: json.RawMessage(`{"terminal":null}`)})
		if err != nil {
			t.Fatal(err)
		}
		second, _, err := pluginNativeProps(plugins.NativeElement{ID: 2, Props: json.RawMessage(`{"terminal":null}`)})
		if err != nil || first == second {
			t.Fatalf("native elements reused an owner: %q %q %v", first, second, err)
		}
	})
	t.Run("final terminal release is idempotent before async creation", func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		owner := pluginNativeTerminal{cancel: cancel}
		owner.release()
		owner.release()
		if ctx.Err() == nil || !owner.closed {
			t.Fatal("terminal release did not close its pending owner")
		}
	})
	t.Run("assistant draft retains source through first admission and retry", testNativeAssistantDraft)
}

func testNativeAssistantDraft(t *testing.T) {
	node, project, worktree, provider := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.ProviderID{4}
	reference := protocol.PluginReference{Name: "assistant-fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 3}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Captured project", Path: "/fixture/project"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/project", Main: true}}
	snapshot.Providers = []protocol.ConversationProvider{{
		ID: provider, Revision: 7, Name: "Captured provider", Enabled: true, Api: "responses", Authentication: "api_key",
		Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}},
	}}
	transport := &conversationTestTransport{snapshot: snapshot, failure: protocol.ErrorCodeOutcomeUnknown}
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node = node
	transport.conversation.Page.Session = protocol.SessionID{9}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	runtime, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Close()
	info := protocol.PluginInfo{Extension: &protocol.PluginExtension{
		Actions: []protocol.PluginAction{protocol.PluginActionReadConversation, protocol.PluginActionControlConversation},
		Desktop: &protocol.PluginDesktopManifest{Conversations: []protocol.PluginConversationDeclaration{
			{ID: "review", Resource: protocol.PluginConversationResourceWorkspace},
		}},
	}}
	info.Summary.Name, info.Summary.Digest = reference.Name, reference.Digest
	info.Summary.SettingsRevision, info.Summary.Enabled, info.Summary.Compatibility.Kind = reference.SettingsRevision, true, "compatible"
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Worktree, scope.Surface = reference, &worktree, protocol.PluginDesktopSurface{Kind: "workspace"}
	native := plugins.NewNativeBinding(t.Context())
	state := &pluginNativeState{runtime: runtime, scope: scope, native: &native, conversations: map[string]*pluginNativeConversation{}}
	runtime.App.Page = shell.SettingsPage
	var failure error
	tester := ui.NewTester(func(view *ui.Context) {
		runtime.drain(view)
		theme.Apply(view, "", theme.Light)
		runtime.packages.infos[reference.Name] = info
		_, failure = state.renderConversation(view, plugins.NativeElement{
			ID: 19, Props: json.RawMessage(`["assistant-chat",{"assistant":"review"}]`),
		}, nil, nil)
	}, 1000, 800)
	tester.SetLocale("en")
	tester.Frame()
	if failure != nil {
		t.Fatal(failure)
	}
	mounted := state.conversations["assistant-chat"]
	if mounted == nil || mounted.draft == nil || mounted.session != (protocol.SessionID{}) ||
		runtime.App.Data.Find(mounted.view.ID) != nil {
		t.Fatal("assistant did not retain an isolated ordinary draft")
	}
	draftID := mounted.view.ID
	if runtime.pluginComposerView(draftID) != mounted.view {
		t.Fatal("assistant contributions did not retain their exact presentation owner")
	}
	if mounted.composer.Host != node.String() || mounted.composer.Project != project.String() ||
		mounted.composer.Worktree != "/fixture/project" || !mounted.context.LocationLocked {
		t.Fatal("assistant draft lost its captured location")
	}
	message := composer.Message{Text: "Inspect the captured project", Model: mounted.composer.Model, Mode: composer.Code, Permission: composer.AskFirst}
	mounted.actions.Send(message)
	for range 4 {
		runtime.workers.Wait()
		tester.Frame()
	}
	if failure != nil {
		t.Fatal(failure)
	}
	if mounted.session == (protocol.SessionID{}) || mounted.draft != nil || runtime.App.Page != shell.SettingsPage ||
		mounted.composer.Draft != message.Text {
		t.Fatal("first admission changed the embedding page or lost its uncertain draft")
	}
	if runtime.pluginComposerView(draftID) != nil {
		t.Fatal("admitted assistant retained its obsolete draft presentation")
	}
	mounted.actions.Send(message)
	for range 4 {
		runtime.workers.Wait()
		tester.Frame()
	}
	transport.mu.Lock()
	defer transport.mu.Unlock()
	var created []protocol.Request
	var submitted []protocol.Request
	for _, request := range transport.requests {
		switch request.Command.Kind {
		case "create_session_at":
			created = append(created, request)
		case "submit_turn":
			submitted = append(submitted, request)
		}
	}
	if len(created) != 1 || len(submitted) != 2 || submitted[0].ID != submitted[1].ID {
		t.Fatalf("assistant retry replaced admission identity: creates=%d submits=%d", len(created), len(submitted))
	}
	input, valid := detail[protocol.CommandCreateSessionAtPayload](created[0].Command.Data)
	if !valid || input.Target != node || input.Worktree == nil || *input.Worktree != worktree || input.Project == nil || *input.Project != project ||
		input.Config == nil || input.Config.Assistant == nil || input.Config.Assistant.Package != reference || input.Config.Assistant.ID != "review" {
		t.Fatal("assistant first admission lost its captured configuration")
	}
}
