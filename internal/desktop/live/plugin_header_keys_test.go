package live

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginHeaderKeepsTitleAndControlsDistinct(t *testing.T) {
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "header-fixture", strings.Repeat("a", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface.Kind = packageReference(info.Summary), "settings"
	section := settings.Section("plugin:header-fixture")
	key := pluginMountKey{scope: pluginScopeIdentity(snapshot.Node, scope), role: "settings", name: info.Summary.Name}
	root.pluginViews.settings[section] = pluginSettingsEntry{section: section, owner: root, info: info, scope: scope}
	ctx, cancel := context.WithCancel(root.ctx)
	host := desktopplugins.NewHost(desktopplugins.HostBinding{Context: ctx, Client: root.controller, Scope: scope})
	components, err := desktopplugins.NativeComponents(host.Native())
	if err != nil {
		t.Fatal(err)
	}
	mounted := &pluginMountedView{key: key, owner: root, info: info, scope: scope, locale: root.packageLocale(),
		generation: 44, ctx: ctx, cancel: cancel, host: host, components: components, mounted: new(desktopplugins.Mounted)}
	root.pluginViews.mounts[key] = mounted
	// This fixture renders native header components directly, without a JS view.
	t.Cleanup(func() { mounted.mounted = nil })
	root.App.Settings.Open(section)
	root.App.Navigate(shell.SettingsPage)
	content := `{"tabs":{"id":"sections","items":[{"id":"overview","label":"Overview"}],"selected":"overview"},"items":[{"label":"Requests","value":"1"}]}`
	props, err := json.Marshal([]any{"header", map[string]string{"content": content}})
	if err != nil {
		t.Fatal(err)
	}
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		for _, component := range components {
			if component.Module == "sailry" && component.Export == "Header" {
				if _, err := component.Render(view.Key("publication"), desktopplugins.NativeElement{Props: props}, nil, nil); err != nil {
					t.Fatal(err)
				}
			}
		}
		ui.Row(view.Key("shared-header")).Children(func() {
			if !root.renderPluginTitle(view) {
				t.Fatal("plugin title was not rendered")
			}
			root.renderPluginHeader(view)
		})
	}, 640, 120)
	for _, generation := range []uint64{44, 44, 45, 45} {
		mounted.generation = generation
		tester.Frame()
		if !tester.HasText("Overview") || !tester.HasText("Requests") || !tester.HasText("1") {
			t.Fatalf("plugin header lost independent content: %v", tester.Texts())
		}
	}
}

func TestPluginConversationHeaderWaitsForFirstTurn(t *testing.T) {
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "context-fixture", strings.Repeat("b", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	info.Extension = new(protocol.DefaultPluginExtension())
	if err := json.Unmarshal([]byte(`[{"id":"context","slot":"header","kind":"popover","label":{"label":"Context"},"handler":"context"}]`), &info.Extension.Ui); err != nil {
		t.Fatal(err)
	}
	session := protocol.DefaultSession()
	session.ID, session.Worktree = protocol.SessionID{1}, protocol.WorktreeID{2}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins, snapshot.Sessions = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}, []protocol.Session{session}
	snapshot.Worktrees = []protocol.Worktree{{ID: session.Worktree}}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response)})
	current := &viewmodel.Conversation{ID: session.ID.String(), Node: snapshot.Node.String()}
	root.App.Data.Unassigned = append(root.App.Data.Unassigned, current)
	draft := root.App.Data.Find("draft/first")
	mount := func(cv *viewmodel.Conversation) *desktopplugins.Registry {
		t.Helper()
		scope := composerPluginScope(root, cv, root.App.Composer(cv.ID))
		registry := root.pluginRegistry(scope, cv.ID)
		mounted := root.ensurePluginView(nil, root, info, scope, "contribution", registry)
		if mounted == nil {
			t.Fatal("the context contribution did not mount")
		}
		if err := mounted.hooks.Publish(json.RawMessage(`[{"id":"context"}]`)); err != nil {
			t.Fatal(err)
		}
		if err := registry.SetActivity(mounted.scope.Package, true, false); err != nil {
			t.Fatal(err)
		}
		return registry
	}
	mount(draft)
	registry := mount(current)
	root.App.Page, root.App.Sidebar.Selected = shell.ConversationPage, draft.ID
	tester := ui.NewTesterWith(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		ui.Row(view).FillWidth().Children(func() { root.renderPluginHeader(view) })
		root.pluginControlsAfterFrame()
	}, 640, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	tester.SetLocale("en")
	for _, id := range []string{draft.ID, current.ID} {
		root.App.Sidebar.Selected = id
		tester.Frame()
		if _, visible := tester.AccessByRole(ui.RoleButton, "Context"); visible {
			t.Fatal("a conversation without a turn shows its context control")
		}
	}
	current.Turns = []conversation.Turn{{ID: "first"}}
	tester.Frame()
	button, visible := tester.AccessByRole(ui.RoleButton, "Context")
	if !visible || button.Disabled {
		t.Fatal("the admitted conversation's first turn has no context control")
	}
	tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
	tester.Frame()
	key, exists := registry.Key(packageReference(info.Summary), "context")
	state := root.pluginControls[pluginControlIdentity(key)]
	if !exists || state == nil || !state.open {
		t.Fatal("the admitted conversation's context popover did not open")
	}
	root.App.Sidebar.Selected = draft.ID
	tester.Frame()
	if _, visible := tester.AccessByRole(ui.RoleButton, "Context"); visible || state.open {
		t.Fatal("switching to a new conversation retained its context control or popover")
	}
	root.App.Sidebar.Selected = current.ID
	tester.Frame()
	if button, visible := tester.AccessByRole(ui.RoleButton, "Context"); !visible || button.Expanded || state.open {
		t.Fatal("returning to the conversation reopened its hidden context popover")
	}
	draft.Turns = current.Turns
	root.App.Sidebar.Selected = draft.ID
	tester.Frame()
	if _, visible := tester.AccessByRole(ui.RoleButton, "Context"); visible {
		t.Fatal("an unadmitted draft shows a session context control")
	}
}
