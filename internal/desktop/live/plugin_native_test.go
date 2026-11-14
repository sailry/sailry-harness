package live

import (
	"context"
	"encoding/json"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"image/png"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
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
	t.Run("the ssh package mounts its SSH terminals", func(t *testing.T) {
		// The bundled package's own declaration: no terminal actions.
		data, err := fs.ReadFile(sailry.Plugins, "plugins/ssh/plugin.json")
		if err != nil {
			t.Fatal(err)
		}
		var manifest struct {
			Extensions map[string]struct {
				Actions []protocol.PluginAction `json:"actions"`
			} `json:"extensions"`
		}
		if err := json.Unmarshal(data, &manifest); err != nil {
			t.Fatal(err)
		}
		declared := manifest.Extensions["dev.sailry.platform"].Actions
		if len(declared) == 0 || slices.Contains(declared, protocol.PluginActionReadTerminals) {
			t.Fatalf("the ssh package declares %v", declared)
		}
		worktree, other, profile := protocol.WorktreeID{1}, protocol.WorktreeID{2}, protocol.SshID{3}
		for _, current := range []struct {
			name          string
			actions       []protocol.PluginAction
			surface       string
			local, remote bool
		}{
			{"ssh package", declared, "workspace", false, true},
			{"ssh package outside a workspace", declared, "settings", false, false},
			{"ssh read only", []protocol.PluginAction{protocol.PluginActionReadSsh}, "workspace", false, false},
			{"terminal package", actions, "workspace", true, false},
			{"both", append(slices.Clone(actions), declared...), "workspace", true, true},
			{"neither", []protocol.PluginAction{protocol.PluginActionReadProjects}, "workspace", false, false},
		} {
			native := plugins.NewNativeBinding(t.Context())
			scope := protocol.DefaultPluginContext()
			scope.Package, scope.Surface, scope.Worktree = reference, protocol.PluginDesktopSurface{Kind: current.surface}, &worktree
			info := protocol.PluginInfo{Extension: &protocol.PluginExtension{Actions: current.actions}}
			info.Summary.Name, info.Summary.Digest = reference.Name, reference.Digest
			info.Summary.SettingsRevision, info.Summary.Enabled = reference.SettingsRevision, true
			info.Summary.Compatibility.Kind = "compatible"
			state := pluginNativeState{
				scope: scope, native: &native,
				runtime: &Runtime{packages: &packageState{infos: map[string]protocol.PluginInfo{reference.Name: info}}},
			}
			local, remote, err := state.terminalAuthority()
			if local != current.local || remote != current.remote || (err == nil) != (local || remote) {
				t.Errorf("%s: local %t, remote %t, %v", current.name, local, remote, err)
			}
			// An SSH terminal has no worktree; a local one is the view's own.
			for name, row := range map[string]struct {
				info    protocol.TerminalInfo
				allowed bool
			}{
				"ssh terminal":     {protocol.TerminalInfo{Ssh: &profile}, remote},
				"own worktree":     {protocol.TerminalInfo{Worktree: &worktree}, local},
				"another worktree": {protocol.TerminalInfo{Worktree: &other}, false},
				"no owner":         {protocol.TerminalInfo{}, false},
			} {
				if pluginTerminalInScope(scope, row.info, local, remote) != row.allowed {
					t.Errorf("%s: %s allowed %t", current.name, name, !row.allowed)
				}
			}
			scope.Worktree = nil
			if pluginTerminalInScope(scope, protocol.TerminalInfo{Ssh: &profile}, local, remote) != remote ||
				pluginTerminalInScope(scope, protocol.TerminalInfo{Worktree: &worktree}, local, remote) {
				t.Errorf("%s: a Node-wide view reaches the wrong terminals", current.name)
			}
		}
	})
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
	t.Run("assistant panel switches between its sessions", testNativeAssistantSessions)
	t.Run("composer state carries the selected model and its window", testNativeComposerModel)
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
	// A view that ends before its draft became a session leaves no composer.
	ended := &pluginNativeState{runtime: runtime, scope: scope, native: &native, conversations: map[string]*pluginNativeConversation{}}
	endedView, endedComposer := ended.newAssistantDraft("ended", *mounted.draft)
	ended.conversations["ended"] = &pluginNativeConversation{view: endedView, composer: endedComposer, draftID: endedView.ID, draft: mounted.draft}
	ended.releaseConversations()
	if _, based := runtime.composerBases[endedComposer]; based || runtime.composerInitialized[endedView.ID] || runtime.App.DropComposer(endedView.ID) != nil || len(ended.conversations) != 0 {
		t.Fatal("an ended assistant view left its draft composer behind")
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
	if runtime.App.Sidebar.Data.Find(mounted.session.String()) != nil || runtime.App.Data.Find(mounted.session.String()) == nil {
		t.Fatal("first admission leaked the assistant into ordinary navigation or lost its mounted view")
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
		case "create_session":
			created = append(created, request)
		case "submit_turn":
			submitted = append(submitted, request)
		}
	}
	if len(created) != 1 || len(submitted) != 2 || submitted[0].ID != submitted[1].ID {
		t.Fatalf("assistant retry replaced admission identity: creates=%d submits=%d", len(created), len(submitted))
	}
	input, valid := detail[protocol.CommandCreateSessionPayload](created[0].Command.Data)
	if !valid || created[0].Target != node || input.Worktree == nil || *input.Worktree != worktree || input.Project == nil || *input.Project != project ||
		input.Config == nil || input.Config.Assistant == nil || input.Config.Assistant.Package != reference || input.Config.Assistant.ID != "review" {
		t.Fatal("assistant first admission lost its captured configuration")
	}
}

// An assistant's panel has New and the session history beside its heading:
// the history lists the assistant's sessions of the captured workspace and
// moves the panel to the one chosen, New to a draft, which says what the
// panel is for.
func testNativeAssistantSessions(t *testing.T) {
	node, project, worktree, provider := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.ProviderID{4}
	reference := protocol.PluginReference{Name: "assistant-fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 3}
	older, newer, foreign := protocol.SessionID{10}, protocol.SessionID{11}, protocol.SessionID{12}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Captured project", Path: "/fixture/project"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/project", Main: true}}
	snapshot.Providers = []protocol.ConversationProvider{{
		ID: provider, Revision: 7, Name: "Captured provider", Enabled: true, Api: "responses", Authentication: "api_key",
		Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}},
	}}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	config.Effort = protocol.Effort{Kind: "default"}
	bound := config
	bound.Assistant = &protocol.PluginConversationBinding{Package: reference, ID: "review"}
	for index, row := range []struct {
		id     protocol.SessionID
		title  string
		config protocol.SessionConfig
	}{{older, "Older review", bound}, {newer, "Newer review", bound}, {foreign, "Ordinary session", config}} {
		session := protocol.Session{ID: row.id, CreationOrder: uint64(index + 1), Revision: 1, Project: &project, Worktree: worktree, Config: row.config, Roles: protocol.DefaultRoleSnapshot()}
		session.Activity.Title = row.title
		snapshot.Sessions = append(snapshot.Sessions, session)
	}
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node = node
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
	var failure error
	tester := ui.NewTesterWith(func(view *ui.Context) {
		runtime.drain(view)
		theme.Apply(view, "", theme.Dark)
		runtime.packages.infos[reference.Name] = info
		_, failure = state.renderConversation(view, plugins.NativeElement{
			ID: 19, Props: json.RawMessage(`["assistant-chat",{"assistant":"review","heading":"Review assistant"}]`),
		}, nil, nil)
		runtime.finishFrame()
	}, 520, 640, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("zh-CN")
	tester.SetDark(true)
	text := func(key string) string { return i18n.In("zh-CN", key) }
	settle := func() {
		t.Helper()
		for range 4 {
			runtime.workers.Wait()
			tester.Advance(100 * time.Millisecond)
			tester.Frame()
		}
		if failure != nil {
			t.Fatal(failure)
		}
	}
	settle()
	press := func(name string) {
		t.Helper()
		info, ok := tester.AccessByRole(ui.RoleButton, name)
		if !ok || info.Disabled {
			t.Fatalf("no enabled button %q: %q", name, tester.Texts())
		}
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		settle()
	}
	shot := func(name string) {
		if dir := os.Getenv("SAILRY_UI_SHOTS"); dir != "" {
			if file, err := os.Create(filepath.Join(dir, name+".png")); err == nil {
				_ = png.Encode(file, tester.Image())
				file.Close()
			}
		}
	}
	// The panel opens on the assistant's latest session.
	if mounted := state.conversations["assistant-chat"]; mounted == nil || mounted.session != newer {
		t.Fatalf("the panel did not open on the latest session: %+v", mounted)
	}
	if runtime.App.Sidebar.Data.Find(newer.String()) != nil || runtime.App.Sidebar.Data.Find(older.String()) != nil || runtime.App.Data.Find(newer.String()) == nil {
		t.Fatal("assistant history leaked into ordinary navigation or lost its embedded transcript")
	}
	if !tester.HasText("Review assistant") {
		t.Fatalf("no heading: %q", tester.Texts())
	}
	// The history lists the assistant's sessions only, the current one
	// checked, and filters by the query.
	press(text("chat_connection_history"))
	if !tester.HasText("Older review") || !tester.HasText("Newer review") || tester.HasText("Ordinary session") {
		t.Fatalf("the history lists %q", tester.Texts())
	}
	shot("parity-live-session-history")
	tester.Type("older")
	settle()
	if !tester.HasText("Older review") || tester.HasText("Newer review") {
		t.Fatalf("the filtered history lists %q", tester.Texts())
	}
	tester.Key(0, ui.KeyEnter)
	settle()
	if mounted := state.conversations["assistant-chat"]; mounted.session != older || mounted.draft != nil {
		t.Fatalf("choosing a session left the panel on %v", mounted.session)
	}
	// New starts a draft for the same assistant and workspace.
	press(text("chat_new"))
	mounted := state.conversations["assistant-chat"]
	if mounted.draft == nil || mounted.session != (protocol.SessionID{}) || mounted.draft.Config.Assistant == nil || *mounted.draft.Config.Assistant != *bound.Assistant ||
		mounted.draft.Project == nil || *mounted.draft.Project != project || mounted.composer.Permission != composer.AskFirst {
		t.Fatalf("New left the panel on %+v", mounted)
	}
	if !tester.HasText(text("chat_sidebar_empty")) {
		t.Fatalf("a new assistant draft has no empty state: %q", tester.Texts())
	}
	shot("parity-live-session-empty")
	if button, _ := tester.AccessByRole(ui.RoleButton, text("chat_new")); !button.Disabled {
		t.Error("New is enabled on a draft")
	}
	// The history goes back from the draft.
	press(text("chat_connection_history"))
	if err := tester.Click("Newer review"); err != nil {
		t.Fatal(err)
	}
	settle()
	if mounted := state.conversations["assistant-chat"]; mounted.session != newer {
		t.Fatalf("the history did not leave the draft: %+v", mounted)
	}
	if runtime.pluginComposerView(mounted.view.ID) != nil {
		t.Error("the left draft kept its presentation")
	}
}

// testNativeComposerModel covers the two members a package compares with the
// statistics to tell that the model changed since the last request: the
// composer's own selection and that model's window, whatever the last
// request used.
func testNativeComposerModel(t *testing.T) {
	node, provider, session := protocol.NodeID{1}, protocol.ProviderID{4}, protocol.SessionID{9}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Providers = []protocol.ConversationProvider{{
		ID: provider, Revision: 7, Name: "Captured provider", Enabled: true, Api: "responses", Authentication: "api_key",
		Models: []protocol.ConversationModel{
			{ID: "small", Context: 32000, DefaultEffort: protocol.Effort{Kind: "default"}},
			{ID: "large", Context: 200000, Tools: true, DefaultEffort: protocol.Effort{Kind: "default"}},
		},
	}}
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node = node
	transport.conversation.Page.Session, transport.conversation.Page.Revision = session, 1
	// The last request ran on the small model.
	statistics := protocol.DefaultConversationStatistics()
	used, window, last := uint64(12000), uint32(32000), "small"
	statistics.ContextTokens, statistics.ContextLimit, statistics.ContextModel = &used, &window, &last
	transport.conversation.Statistics = &statistics
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
	changes := make(chan struct{}, 1)
	release, err := controller.WatchConversation(t.Context(), session, func() {
		select {
		case changes <- struct{}{}:
		default:
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	for {
		history, _ := controller.Conversation(session)
		if history.Err != nil {
			t.Fatal(history.Err)
		}
		if history.Snapshot.Statistics != nil {
			break
		}
		select {
		case <-changes:
		case <-t.Context().Done():
			t.Fatal(t.Context().Err())
		}
	}
	scope := protocol.DefaultPluginContext()
	scope.Session, scope.Surface = &session, protocol.PluginDesktopSurface{Kind: "composer"}
	native := plugins.NewNativeBinding(t.Context())
	state := &pluginNativeState{runtime: runtime, scope: scope, native: &native}
	view := &viewmodel.Conversation{ID: session.String(), Node: node.String()}
	for _, row := range []struct {
		model  string
		window any
		tools  bool
	}{
		{"small", float64(32000), false},
		// A model chosen since: its own window, not the last request's.
		{"large", float64(200000), true},
		// A model the provider no longer lists keeps its name and has no window.
		{"retired", nil, false},
	} {
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider, row.model, protocol.WorkModeCode, protocol.PermissionAsk
		projected, err := state.projectComposer(view, runtime.App.Composer(view.ID), config, protocol.PluginInfo{Extension: &protocol.PluginExtension{}})
		if err != nil {
			t.Fatal(err)
		}
		body, err := json.Marshal(projected)
		if err != nil {
			t.Fatal(err)
		}
		var got struct {
			Model      any `json:"model"`
			Limit      any `json:"context_limit"`
			Tools      any `json:"model_tools"`
			Statistics struct {
				Model any `json:"context_model"`
				Limit any `json:"context_limit"`
			} `json:"statistics"`
		}
		if err = json.Unmarshal(body, &got); err != nil {
			t.Fatal(err)
		}
		if got.Model != row.model || got.Limit != row.window || got.Tools != row.tools {
			t.Fatalf("selected %q: the composer state has model %v, window %v, tools %v", row.model, got.Model, got.Limit, got.Tools)
		}
		if got.Statistics.Model != "small" || got.Statistics.Limit != "32000" {
			t.Fatalf("selected %q: the statistics carry model %v and window %v of the last request", row.model, got.Statistics.Model, got.Statistics.Limit)
		}
	}
}
