//go:build integration

package live

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginUIIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 90*time.Second)
	defer cancel()
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		requests.Add(1)
		writer.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(writer, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"{\\\"move\\\":0}\"},\"finish_reason\":null}]}\n\n")
		fmt.Fprint(writer, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.Start(ctx, profile)
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	controller := client.New(pluginUITransport{Transport: link.NewLocal(owner.ID(), owner.ID(), owner), test: t})
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Plugin fixture", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, server.URL+"/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output = "fixture", 200000, 8192
	model.Reasoning, model.DefaultEffort = true, protocol.Effort{Kind: "none"}
	model.Efforts = []protocol.Effort{model.DefaultEffort}
	provider.Models = []protocol.ConversationModel{model}
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	projectOutput := do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Plugin workspace", Path: t.TempDir(), Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}})
	project, valid := detail[protocol.Project](projectOutput.Data)
	if !valid {
		t.Fatal("registered project was not returned")
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider.ID, model.ID
	config.Effort = model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	sessionOutput := do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}})
	session, valid := detail[protocol.Session](sessionOutput.Data)
	if !valid {
		t.Fatal("created session was not returned")
	}
	do(protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: session.Revision, Title: "Plugin acceptance"}})
	infos := map[string]protocol.PluginInfo{}
	for _, name := range []string{"activity-board", "commands", "gomoku", "goals"} {
		if !slices.ContainsFunc(controller.Snapshot().Plugins, func(entry protocol.PluginSummary) bool { return entry.Name == name }) {
			do(protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: name}})
		}
		output := do(protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: name}})
		info, exists := detail[protocol.PluginInfo](output.Data)
		if !exists {
			t.Fatalf("required bundled package %q was not returned", name)
		}
		if !info.Summary.Enabled {
			do(protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{
				Name: name, ExpectedRevision: info.Summary.Revision, Enabled: true,
			}})
			output = do(protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: name}})
			info, _ = detail[protocol.PluginInfo](output.Data)
		}
		infos[name] = info
	}
	settingsOutput := do(protocol.Command{Kind: "read_plugin_settings", Data: protocol.CommandReadPluginSettingsPayload{Package: packageReference(infos["gomoku"].Summary)}})
	gameSettings, valid := detail[protocol.PluginSettingsState](settingsOutput.Data)
	if !valid {
		t.Fatal("game settings were not returned")
	}
	gameSettings.Values["ai_model"], _ = json.Marshal(provider.ID.String() + "/" + model.ID)
	do(protocol.Command{Kind: "save_plugin_settings", Data: protocol.CommandSavePluginSettingsPayload{
		Package: gameSettings.Package, Values: gameSettings.Values, Secrets: map[string]protocol.PluginSettingsSecretUpdate{},
	}})
	goal := protocol.NewRequestID()
	value, _ := json.Marshal(map[string]any{"id": goal.String(), "description": "Exercise the real plugin status lifecycle", "state": "active", "after": nil})
	seed := controller.Prepare(protocol.Command{Kind: "write_plugin_conversation_value", Data: protocol.CommandWritePluginConversationValuePayload{
		Key: "goal", Value: value,
	}})
	seed.Plugin = &protocol.PluginContext{
		Surface: protocol.PluginDesktopSurface{Kind: "composer"}, Package: packageReference(infos["goals"].Summary),
		Worktree: &session.Worktree, Session: &session.ID,
	}
	if _, err := controller.Execute(ctx, seed); err != nil {
		t.Fatal(err)
	}
	runtime, err := NewWithPreferences(ctx, controller, preferences.Open(filepath.Join(profile, "desktop", "preferences.json")))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Close()
	runtime.OpenSession(session.ID.String())
	runtime.App.NavOpen = true
	tester := ui.NewTester(runtime.App.View, 1440, 1000)
	tester.SetLocale("en")
	wait := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.NewTimer(15 * time.Second)
		defer deadline.Stop()
		ticker := time.NewTicker(10 * time.Millisecond)
		defer ticker.Stop()
		for {
			tester.Advance(16 * time.Millisecond)
			tester.Frame()
			if ready() {
				return
			}
			if runtime.pluginViews != nil {
				for _, mounted := range runtime.pluginViews.mounts {
					if mounted.failure != nil {
						t.Fatalf("%s: package %s: %v", label, mounted.scope.Package.Name, mounted.failure)
					}
				}
			}
			select {
			case <-ctx.Done():
				t.Fatalf("%s: %v", label, ctx.Err())
			case <-deadline.C:
				t.Fatalf("%s did not settle; visible text: %q", label, tester.Texts())
			case <-ticker.C:
			}
		}
	}
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	wait("plugin inventory", func() bool {
		return runtime.pluginViews != nil && len(runtime.pluginViews.navigation) > 0 && runtime.packages.infos["gomoku"].Extension != nil
	})
	order, pins := runtime.App.FeaturePreferences()
	gameID := "plugin:" + controller.Target().String() + "/gomoku"
	pins[gameID] = true
	runtime.App.SetFeaturePreferences(order, pins)
	tester.Frame()
	click("Activity Board")
	wait("activity board authoritative catalog", func() bool { return tester.HasText("Plugin acceptance") && tester.HasText("Idle") })
	t.Log("activity board: real Node catalog and captured session rendered")
	click("Gomoku")
	wait("game settings and models", func() bool { return tester.HasText("Choose a side") })
	click("Play black")
	wait("game board", func() bool { return tester.HasText("Your turn") })
	click("H8, empty")
	wait("game model turn", func() bool {
		return requests.Load() == 1 && tester.HasText("Your turn") && tester.HasText("H8, Black")
	})
	t.Log("gomoku: real retained callbacks, human move and one Node-owned model response completed")
	runtime.App.Navigate(shell.ConversationPage)
	runtime.OpenSession(session.ID.String())
	// Coming back to the conversation, the badge is drawn disabled until its
	// status watch reconnects, and its tooltip repeats the goal: the click waits
	// for the watch and the popover is recognized by its own action.
	wait("goal badge", func() bool {
		for _, feed := range runtime.pluginStatus {
			if feed.scope.Session != nil && *feed.scope.Session == session.ID {
				return tester.HasText("Goal") && feed.view.Connected
			}
		}
		return false
	})
	tester.Frame()
	click("Goal")
	wait("goal popover", func() bool {
		return tester.HasText("Exercise the real plugin status lifecycle") && tester.HasText("Stop")
	})
	click("Stop")
	wait("goal pause", func() bool { return tester.HasText("Stopped") && tester.HasText("Resume") })
	t.Log("goals: Node-owned badge opened the mounted popover and persisted pause through the SDK")
	click("Clear")
	wait("hidden goal status", func() bool {
		for _, feed := range runtime.pluginStatus {
			if feed.scope.Session == nil || *feed.scope.Session != session.ID {
				continue
			}
			for _, entry := range feed.view.Entries {
				if entry.Package.Name == "goals" {
					return !entry.State.Visible && !feed.open[pluginStatusIdentity(entry)]
				}
			}
		}
		return false
	})
	runtime.enqueue(func(view *ui.Context) {
		conversation := runtime.App.Data.Find(session.ID.String())
		input := runtime.App.Composer(conversation.ID)
		_, actions := runtime.conversation(view, conversation, input)
		actions.Send(composer.Message{
			Text: "/goal", Model: input.Model, Effort: input.EffortName(view, input.Chosen(runtime.models())),
			Mode: input.Mode, Permission: input.Permission, Features: input.Features,
		})
	})
	wait("command reveals hidden status popover", func() bool { return tester.HasText("No goal") && tester.HasText("Goal") })
	if requests.Load() != 1 {
		t.Fatal("plugin slash command started an extra model request")
	}
	t.Log("goals: original scoped slash command revealed the hidden status popover without a model call")
	click("Goal")
	runtime.App.PanelOpen = true
	runtime.App.Panel.Open(panel.Terminal)
	var terminalID protocol.TerminalID
	// Local terminals are core-owned: with the commands package enabled, its
	// declared terminal renderer and creation contribution stay unmounted and
	// the panel creates and draws a core terminal named by its Node ID.
	wait("the panel creates a Node terminal", func() bool {
		for _, terminal := range controller.Snapshot().Terminals {
			if terminal.Worktree != nil && *terminal.Worktree == session.Worktree && terminal.Status.Kind == "running" {
				terminalID = terminal.ID
				return true
			}
		}
		return false
	})
	wait("core terminal rendering", func() bool {
		state := runtime.packages.terminals[terminalID.String()]
		return state != nil && state.terminal != nil && runtime.terminalInventory().focus != terminalID.String() &&
			runtime.App.Panel.Active == (panel.Tab{Kind: panel.Terminal, ID: terminalID.String()}) && tester.HasText("Terminal")
	})
	tester.Type("printf '\\163\\141\\151\\154\\162\\171\\055\\160\\154\\165\\147\\151\\156\\055\\151\\157\\n'")
	tester.Key(0, ui.KeyEnter)
	wait("commands terminal input reaches the Node PTY", func() bool {
		output := do(protocol.Command{Kind: "inspect_terminal", Data: protocol.CommandInspectTerminalPayload{Terminal: terminalID}})
		screen, valid := detail[protocol.TerminalSnapshot](output.Data)
		return valid && screen.Screen.VT != nil && strings.Contains(string(screen.Screen.VT.Bytes), "sailry-plugin-io")
	})
	t.Log("commands: with the package enabled its terminal renderer stayed unmounted; the core terminal was created, rendered and sent input to the attached Node PTY")
	for _, mounted := range runtime.pluginViews.mounts {
		if unmountedPlugin(mounted.scope.Package.Name) {
			t.Fatal("native Files/Git were also mounted through the plugin runtime")
		}
	}
}

type pluginUITransport struct {
	link.Transport
	test *testing.T
}

func (transport pluginUITransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	admitted, err := transport.Transport.Dispatch(ctx, request)
	if err != nil {
		transport.test.Logf("%s rejected: %v", request.Command.Kind, err)
		return admitted, err
	}
	completion := make(chan link.Response, 1)
	original := admitted.Completion
	go func() {
		defer close(completion)
		result, exists := <-original
		if exists {
			if result.Fault != nil {
				transport.test.Logf("%s failed: %v", request.Command.Kind, result.Fault)
			}
			completion <- result
		}
	}()
	admitted.Completion = completion
	return admitted, nil
}
