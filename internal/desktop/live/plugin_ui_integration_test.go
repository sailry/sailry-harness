//go:build integration

package live

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
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
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/testplugin"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginUIIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 90*time.Second)
	defer cancel()
	var requests atomic.Int32
	var fixturePrompt atomic.Bool
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		var input struct {
			Messages []struct {
				Role    string
				Content json.RawMessage
			}
		}
		if json.NewDecoder(request.Body).Decode(&input) == nil {
			for _, message := range input.Messages {
				if message.Role == "user" && strings.Contains(string(message.Content), "Return the fixture reply.") {
					fixturePrompt.Store(true)
				}
			}
		}
		requests.Add(1)
		writer.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(writer, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Fixture model reply\"},\"finish_reason\":null}]}\n\n")
		fmt.Fprint(writer, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, profile, node.Options{Assembly: panelProjectionHost()})
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
	installCompetingTerminal(t, ctx, controller)
	infos := map[string]protocol.PluginInfo{}
	for _, fixture := range []testplugin.Package{testplugin.Desktop(), testplugin.Status(), testplugin.Model()} {
		info, err := testplugin.Install(ctx, controller, fixture)
		if err != nil {
			t.Fatal(err)
		}
		infos[fixture.Name] = info
	}
	settingsOutput := do(protocol.Command{Kind: "read_plugin_settings", Data: protocol.CommandReadPluginSettingsPayload{Package: packageReference(infos[testplugin.ModelName].Summary)}})
	modelSettings, valid := detail[protocol.PluginSettingsState](settingsOutput.Data)
	if !valid {
		t.Fatal("fixture model settings were not returned")
	}
	modelSettings.Values["model"], _ = json.Marshal(provider.ID.String() + "/" + model.ID)
	do(protocol.Command{Kind: "save_plugin_settings", Data: protocol.CommandSavePluginSettingsPayload{
		Package: modelSettings.Package, Values: modelSettings.Values, Secrets: map[string]protocol.PluginSettingsSecretUpdate{},
	}})
	value, _ := json.Marshal(map[string]any{"text": "Exercise the real plugin status lifecycle", "phase": "active", "detail": "Just started"})
	seed := controller.Prepare(protocol.Command{Kind: "write_plugin_conversation_value", Data: protocol.CommandWritePluginConversationValuePayload{
		Key: "state", Value: value,
	}})
	seed.Plugin = &protocol.PluginContext{
		Surface: protocol.PluginDesktopSurface{Kind: "composer"}, Package: packageReference(infos[testplugin.StatusName].Summary),
		Worktree: &session.Worktree, Session: &session.ID,
	}
	if _, err := controller.Execute(ctx, seed); err != nil {
		t.Fatal(err)
	}
	assembly := testassembly.Full()
	assembly.ExecutionSettings, assembly.ProjectWorkflows = nil, nil
	runtime, err := NewWithAssembly(ctx, controller, preferences.Open(filepath.Join(profile, "desktop", "preferences.json")), assembly)
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
		return runtime.pluginViews != nil && len(runtime.pluginViews.navigation) == 2 &&
			runtime.packages.infos[testplugin.DesktopName].Extension != nil && runtime.packages.infos[testplugin.TerminalName].Extension != nil
	})
	auditContextRing(t, runtime, tester, session.ID.String(), wait)
	click("Desktop fixture")
	wait("the fixture settings and live activity catalog", func() bool {
		return tester.HasText("Fixture ready") && tester.HasText("Activity: Plugin acceptance · idle")
	})
	click("Increment")
	wait("retained fixture callback", func() bool { return tester.HasText("Fixture clicks: 1") })
	click("Model fixture")
	wait("fixture model configured", func() bool {
		button, found := tester.AccessByRole(ui.RoleButton, "Generate fixture")
		return found && !button.Disabled
	})
	click("Generate fixture")
	wait("Node-owned plugin model response", func() bool {
		return requests.Load() == 1 && fixturePrompt.Load() && tester.HasText("Fixture model reply")
	})
	runtime.App.Navigate(shell.ConversationPage)
	runtime.OpenSession(session.ID.String())
	badge := func(phase, detail string) bool {
		for _, feed := range runtime.pluginStatus {
			if feed.scope.Session == nil || *feed.scope.Session != session.ID || !feed.view.Connected {
				continue
			}
			for _, entry := range feed.view.Entries {
				if entry.Package.Name != testplugin.StatusName || !entry.State.Visible {
					continue
				}
				segments := entry.State.Segments
				if len(segments) != 2 || segments[0].Label.Label != phase || segments[1].Label.Label != detail ||
					entry.State.Label == nil || entry.State.Label.Label != "Fixture status" {
					return false
				}
				button, named := tester.AccessByRole(ui.RoleButton, "Fixture status")
				return named && !button.Disabled && tester.HasText(phase) && tester.HasText(detail)
			}
		}
		return false
	}
	wait("the connected two-segment fixture badge", func() bool { return badge("Running", "Just started") })
	click("Running")
	wait("fixture popover", func() bool {
		return tester.HasText("Exercise the real plugin status lifecycle") && tester.HasText("Pause") && tester.HasText("End")
	})
	for _, label := range []string{"Edit", "Clear", "Reopen", "Save"} {
		if _, found := tester.AccessByRole(ui.RoleButton, label); found {
			t.Fatalf("the fixture popover offers %q: %q", label, tester.Texts())
		}
	}
	click("Pause")
	wait("paused fixture popover", func() bool {
		return tester.HasText("Paused · Paused by you") && tester.HasText("Resume") && tester.HasText("End")
	})
	wait("the Node updates both badge segments", func() bool {
		return badge("Paused", "Paused by you") && !tester.HasText("Running")
	})
	readStatus := func() protocol.PluginStorageConversationEntry {
		t.Helper()
		read := controller.Prepare(protocol.Command{Kind: "read_plugin_conversation_value", Data: protocol.CommandReadPluginConversationValuePayload{Key: "state"}})
		read.Plugin = seed.Plugin
		output, err := controller.Execute(ctx, read)
		if err != nil {
			t.Fatal(err)
		}
		entry, valid := detail[protocol.PluginStorageConversationEntry](output.Data)
		if output.Kind != "plugin_conversation_value" || !valid {
			t.Fatalf("the Node returned no conversation value: %+v", output)
		}
		return entry
	}
	paused := readStatus()
	var state struct{ Text, Phase, Detail string }
	if !paused.Present || json.Unmarshal(paused.Value, &state) != nil || state.Phase != "paused" || state.Detail != "Paused by you" ||
		state.Text != "Exercise the real plugin status lifecycle" {
		t.Fatalf("the Node did not persist paused fixture status: %+v", paused)
	}
	click("End")
	wait("hidden fixture status", func() bool {
		for _, feed := range runtime.pluginStatus {
			if feed.scope.Session == nil || *feed.scope.Session != session.ID {
				continue
			}
			for _, entry := range feed.view.Entries {
				if entry.Package.Name == testplugin.StatusName {
					return feed.view.Connected && !entry.State.Visible && !feed.open[pluginStatusIdentity(entry)] && !tester.HasText("End")
				}
			}
		}
		return false
	})
	if ended := readStatus(); ended.Present || ended.Revision <= paused.Revision {
		t.Fatalf("the Node did not remove the ended fixture status: %+v", ended)
	}
	runtime.enqueue(func(view *ui.Context) {
		conversation := runtime.App.Data.Find(session.ID.String())
		input := runtime.App.Composer(conversation.ID)
		_, actions := runtime.conversation(view, conversation, input)
		actions.Send(composer.Message{
			Text: "/fixture", Model: input.Model, Effort: input.EffortName(view, input.Chosen(runtime.models())),
			Mode: input.Mode, Permission: input.Permission, Features: input.Features,
		})
	})
	wait("command reveals hidden status popover", func() bool { return tester.HasText("No fixture status") })
	if requests.Load() != 1 {
		t.Fatal("plugin slash command started a model request")
	}
	t.Log("the scoped slash command revealed the hidden status popover without a model call")
	click("Fixture status")
	// The header offers the overview once the conversation has a turn: one
	// plain message, answered by the fixture, with no tool and no output.
	runtime.enqueue(func(view *ui.Context) {
		conversation := runtime.App.Data.Find(session.ID.String())
		input := runtime.App.Composer(conversation.ID)
		_, actions := runtime.conversation(view, conversation, input)
		actions.Send(composer.Message{
			Text: "Say nothing", Model: input.Model, Effort: input.EffortName(view, input.Chosen(runtime.models())),
			Mode: input.Mode, Permission: input.Permission, Features: input.Features,
		})
	})
	wait("the session's first turn", func() bool {
		conversation := runtime.App.Data.Find(session.ID.String())
		return requests.Load() == 2 && conversation != nil && len(conversation.Turns) == 1 && !runtime.App.Composer(conversation.ID).Running
	})
	auditSessionOverview(t, runtime, tester, session.ID, wait)
	// A competing local package renderer must not replace the core terminal.
	tester.Key(ui.Ctrl, ui.KeyBackquote)
	var terminalID protocol.TerminalID
	wait("the terminal shortcut creates a Node terminal", func() bool {
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
	wait("native terminal input reaches the Node PTY", func() bool {
		output := do(protocol.Command{Kind: "inspect_terminal", Data: protocol.CommandInspectTerminalPayload{Terminal: terminalID}})
		screen, valid := detail[protocol.TerminalSnapshot](output.Data)
		return valid && screen.Screen.VT != nil && strings.Contains(string(screen.Screen.VT.Bytes), "sailry-plugin-io")
	})
	t.Log("the native terminal rendered and sent input while the package renderer stayed unmounted")
	assertTerminalRendererUnmounted(t, runtime)
	for _, mounted := range runtime.pluginViews.mounts {
		if mounted.key.role != "settings" && runtime.pluginViewsRetired(mounted.scope.Package) {
			t.Fatal("a native resource was also mounted through the plugin runtime")
		}
	}
	t.Run("local plugin pages", func(t *testing.T) {
		auditPluginPages(t, runtime, func(ctx context.Context, command protocol.Command) protocol.Output {
			t.Helper()
			output, err := controller.Do(ctx, command)
			if err != nil {
				t.Fatalf("%s: %v", command.Kind, err)
			}
			return output
		})
	})
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
