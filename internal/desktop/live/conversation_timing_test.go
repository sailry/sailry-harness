package live

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAssistantElapsedClock(t *testing.T) {
	node, project, worktree, provider := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.ProviderID{4}
	session, turn := protocol.SessionID{5}, protocol.TurnID{6}
	reference := protocol.PluginReference{Name: "assistant-clock", Digest: strings.Repeat("a", 64), SettingsRevision: 1}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider, "fixture"
	config.Assistant = &protocol.PluginConversationBinding{Package: reference, ID: "review"}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Fixture", Path: "/fixture"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture", Main: true}}
	snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Revision: 1, Enabled: true, Models: []protocol.ConversationModel{{ID: "fixture"}}}}
	snapshot.Sessions = []protocol.Session{{ID: session, Revision: 1, Project: &project, Worktree: worktree, Config: config, Roles: protocol.DefaultRoleSnapshot()}}
	transport := &conversationTestTransport{snapshot: snapshot, conversation: protocol.DefaultConversationSnapshot()}
	transport.conversation.Node, transport.conversation.Page.Session = node, session
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
	// The fixed conversation copy never receives a model, tool or network event.
	runtime.conversationWatches[session] = func() {}
	runtime.App.Page = shell.PluginPage
	info := protocol.PluginInfo{Extension: &protocol.PluginExtension{
		Actions: []protocol.PluginAction{protocol.PluginActionReadConversation, protocol.PluginActionControlConversation},
		Desktop: &protocol.PluginDesktopManifest{Conversations: []protocol.PluginConversationDeclaration{{ID: "review", Resource: protocol.PluginConversationResourceWorkspace}}},
	}}
	info.Summary.Name, info.Summary.Digest, info.Summary.SettingsRevision = reference.Name, reference.Digest, reference.SettingsRevision
	info.Summary.Enabled, info.Summary.Compatibility.Kind = true, "compatible"
	runtime.packages.infos[reference.Name] = info
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Worktree, scope.Surface = reference, &worktree, protocol.PluginDesktopSurface{Kind: "workspace"}
	native := plugins.NewNativeBinding(t.Context())
	mount := &pluginNativeState{runtime: runtime, scope: scope, native: &native, conversations: map[string]*pluginNativeConversation{}}
	defer mount.releaseConversations()
	run := protocol.ConversationRun{Session: session, Turn: turn, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning}
	visible, connected, initialized := true, true, false
	frames := 0
	var failure error
	tester := ui.NewTesterWith(func(view *ui.Context) {
		frames++
		theme.Apply(view, "", theme.Light)
		if !initialized {
			started := view.Now().Add(-94 * time.Second).UnixMilli()
			run.StartedMs, initialized = &started, true
		}
		page := protocol.DefaultConversationPage()
		page.Session, page.Runs = session, []protocol.ConversationRun{run}
		runtime.conversationCopies = map[protocol.SessionID]conversationCopy{session: {state: client.ConversationState{Connected: connected, Snapshot: protocol.ConversationSnapshot{Page: page}}, ok: true}}
		if visible {
			_, failure = mount.renderConversation(view, plugins.NativeElement{ID: 1, Props: json.RawMessage(`["clock",{"assistant":"review"}]`)}, nil, nil)
		}
	}, 520, 640, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	tester.Frame()
	if failure != nil || !tester.HasText("1m34s") {
		t.Fatalf("initial assistant clock: %q, %v", tester.Texts(), failure)
	}
	before := frames
	tester.Advance(2 * time.Second)
	if failure != nil || frames <= before || !tester.HasText("1m36s") {
		t.Fatalf("assistant clock did not advance without an event: frames %d -> %d, %q, %v", before, frames, tester.Texts(), failure)
	}
	for _, state := range []string{"completed", "disconnected", "hidden", "not started"} {
		t.Run(state, func(t *testing.T) {
			visible, connected = true, true
			run.Status = protocol.ConversationStatusRunning
			switch state {
			case "completed":
				run.Status = protocol.ConversationStatusCompleted
			case "disconnected":
				connected = false
			case "hidden":
				visible = false
			case "not started":
				run.Status, run.StartedMs = protocol.ConversationStatusQueued, nil
			}
			tester.Frame()
			before := frames
			tester.Advance(2 * time.Second)
			if failure != nil || frames != before {
				t.Fatalf("inactive assistant kept its clock running: frames %d -> %d, %v", before, frames, failure)
			}
		})
	}
}
