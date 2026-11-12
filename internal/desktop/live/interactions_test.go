package live

import (
	"context"
	"encoding/json"
	"errors"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/egoist/mygo/ui"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// interactionModel is a fake model. The last "scenario:<name>" a user wrote
// names the calls it makes, one a request, each once; it answers in text
// when all of them returned. It records every tool result it is given.
type interactionModel struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	results map[string]string
	plans   map[string][][3]string
}

func (m *interactionModel) result(id string) string {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.results[id]
}

func (m *interactionModel) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	m.mu.Lock()
	scenario := ""
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if text, found := fantasy.AsMessagePart[fantasy.TextPart](part); found && message.Role == fantasy.MessageRoleUser {
				if at := strings.LastIndex(text.Text, "scenario:"); at >= 0 {
					scenario = strings.Fields(text.Text[at+len("scenario:"):])[0]
				}
			}
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found {
				if text, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output); found {
					m.results[result.ToolCallID] = text.Text
				}
				if failure, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentError](result.Output); found && failure.Error != nil {
					m.results[result.ToolCallID] = failure.Error.Error()
				}
			}
		}
	}
	var next *[3]string
	for _, planned := range m.plans[scenario] {
		if _, returned := m.results[planned[0]]; !returned {
			next = &planned
			break
		}
	}
	m.mu.Unlock()
	return func(yield func(fantasy.StreamPart) bool) {
		part := fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done " + scenario}
		if next != nil {
			part = fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: next[0], ToolCallName: next[1], ToolCallInput: next[2]}
		}
		if yield(part) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

// interactionTransport lets the test fail or hold a command on its way.
type interactionTransport struct {
	link.Transport
	mu   *sync.Mutex
	hook *func(protocol.Request) error
	sent *[]protocol.Request
}

func (t interactionTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	t.mu.Lock()
	*t.sent = append(*t.sent, request)
	hook := *t.hook
	t.mu.Unlock()
	if hook != nil {
		if err := hook(request); err != nil {
			return link.Admission{}, err
		}
	}
	return t.Transport.Dispatch(ctx, request)
}

// interactionKinds answers every kind of question and decides approvals in
// the window, with pointer clicks and typed text, against a real Node, a
// fake model and an MCP server in memory. SAILRY_UI_SHOTS names a directory
// that receives its frames. It runs as a step of TestComposerSendRouting.
func interactionKinds(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 4*time.Minute)
	defer cancel()
	const pkg, authorizeURL = "mcp-interactions", "https://fixture.test/authorize?state=fixture"
	alias := func(tool string) string { return mcp.Alias(pkg, "server", tool) }
	selectTools, _ := json.Marshal(map[string]any{"query": "select:" + alias("form") + "," + alias("authorize") + "," + alias("write")})
	ask := func(prompt string, input map[string]any) string {
		body, _ := json.Marshal(map[string]any{"prompt": prompt, "input": input})
		return string(body)
	}
	script := &interactionModel{results: map[string]string{}, plans: map[string][][3]string{
		"choice": {{"choice-ask", "ask_user", ask("Which loader?", map[string]any{"kind": "choice", "options": []string{"Keep the loader", "Use the spinner"}, "multiple": false, "allow_other": true})}},
		"other":  {{"other-ask", "ask_user", ask("Which loader then?", map[string]any{"kind": "choice", "options": []string{"Keep the loader", "Use the spinner"}, "multiple": false, "allow_other": true})}},
		"multi":  {{"multi-ask", "ask_user", ask("Which checks?", map[string]any{"kind": "choice", "options": []string{"Build", "Vet", "Race"}, "multiple": true, "allow_other": true})}},
		"text":   {{"text-ask", "ask_user", ask("Name the branch", map[string]any{"kind": "text"})}},
		"form":   {{"form-search", "tool_search", string(selectTools)}, {"form-call", alias("form"), `{}`}},
		"link":   {{"link-search", "tool_search", string(selectTools)}, {"link-call", alias("authorize"), `{}`}},
		"deny":   {{"deny-search", "tool_search", string(selectTools)}, {"deny-call", alias("write"), `{"text":"denied"}`}},
		"allow":  {{"allow-search", "tool_search", string(selectTools)}, {"allow-call", alias("write"), `{"text":"allowed"}`}},
		"reason": {{"reason-search", "tool_search", string(selectTools)}, {"reason-call", alias("write"), `{"text":"reasoned"}`}},
	}}
	elicited := make(chan *sdk.ElicitResult, 4)
	written := make(chan string, 4)
	options := &mcp.Options{Open: func(ctx context.Context, _ mcp.Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		server := sdk.NewServer(&sdk.Implementation{Name: "interaction-fixture", Version: "1"}, nil)
		elicitTool := func(name string, parameters *sdk.ElicitParams) {
			sdk.AddTool(server, &sdk.Tool{Name: name, Annotations: &sdk.ToolAnnotations{ReadOnlyHint: true}}, func(_ context.Context, request *sdk.CallToolRequest, _ struct{}) (*sdk.CallToolResult, any, error) {
				// The handler runs twice: it asks for input, then reads it.
				if len(request.Params.InputResponses) == 0 {
					return &sdk.CallToolResult{InputRequests: sdk.InputRequestMap{"input": parameters}}, nil, nil
				}
				result, valid := request.Params.InputResponses["input"].(*sdk.ElicitResult)
				if !valid {
					return nil, nil, errors.New("the elicitation was not answered")
				}
				elicited <- result
				return &sdk.CallToolResult{Content: []sdk.Content{&sdk.TextContent{Text: name + ":" + result.Action}}}, nil, nil
			})
		}
		elicitTool("form", &sdk.ElicitParams{Mode: "form", Message: "Describe the release", RequestedSchema: json.RawMessage(`{"type":"object",
			"propertyOrder":["name","count","ratio","agree","color","tags","note"],
			"required":["name","count","agree","color","tags"],
			"properties":{
				"name":{"type":"string","title":"Name","description":"Two to eight letters","minLength":2,"maxLength":8},
				"count":{"type":"integer","title":"Count","minimum":1,"maximum":5},
				"ratio":{"type":"number","title":"Ratio","minimum":0,"maximum":1},
				"agree":{"type":"boolean","title":"Agree"},
				"color":{"type":"string","title":"Color","enum":["red","blue"],"enumNames":["Red","Blue"]},
				"tags":{"type":"array","title":"Tags","minItems":1,"maxItems":2,"items":{"type":"string","enum":["a","b","c"],"enumNames":["Tag A","Tag B","Tag C"]}},
				"note":{"type":"string","title":"Note","format":"email"}
			}}`)})
		elicitTool("authorize", &sdk.ElicitParams{Mode: "url", Message: "Authorize the fixture", URL: authorizeURL, ElicitationID: "authorization"})
		sdk.AddTool(server, &sdk.Tool{Name: "write"}, func(_ context.Context, _ *sdk.CallToolRequest, input struct {
			Text string `json:"text"`
		}) (*sdk.CallToolResult, any, error) {
			written <- input.Text
			return &sdk.CallToolResult{Content: []sdk.Content{&sdk.TextContent{Text: "wrote:" + input.Text}}}, nil, nil
		})
		clientWire, serverWire := sdk.NewInMemoryTransports()
		serverSession, err := server.Connect(ctx, serverWire, nil)
		if err != nil {
			return nil, err
		}
		t.Cleanup(func() { _ = serverSession.Close() })
		return sdk.NewClient(&sdk.Implementation{Name: "interaction-client", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit, Capabilities: &sdk.ClientCapabilities{
			Elicitation: &sdk.ElicitationCapabilities{Form: &sdk.FormElicitationCapabilities{}, URL: &sdk.URLElicitationCapabilities{}},
		}}).Connect(ctx, clientWire, nil)
	}}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, profile, node.Options{
		MCP: options, Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("interactions must not launch a process")
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	var mu sync.Mutex
	var hook func(protocol.Request) error
	var sent []protocol.Request
	controller := client.New(interactionTransport{Transport: link.NewLocal(owner.ID(), owner.ID(), owner), mu: &mu, hook: &hook, sent: &sent})
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	setHook := func(f func(protocol.Request) error) {
		mu.Lock()
		hook = f
		mu.Unlock()
	}
	dispatched := func(kind string) []protocol.Request {
		mu.Lock()
		defer mu.Unlock()
		var list []protocol.Request
		for _, request := range sent {
			if request.Command.Kind == kind {
				list = append(list, request)
			}
		}
		return list
	}
	r, err := NewWithPreferences(ctx, controller, preferences.ForStartup(profile))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	r.App.Mode = theme.Dark
	tester := ui.NewTesterWith(r.App.View, 1280, 820, ui.TesterOptions{Dark: true, Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("zh-CN")
	text := func(key string, args ...any) string { return i18n.In("zh-CN", key, args...) }
	frames := func() {
		for range 3 {
			tester.Advance(200 * time.Millisecond)
			tester.Frame()
		}
	}
	settle := func() {
		for range 4 {
			r.workers.Wait()
			tester.Advance(200 * time.Millisecond)
			tester.Frame()
		}
	}
	wait := func(what string, done func() bool) {
		t.Helper()
		for end := time.Now().Add(30 * time.Second); time.Now().Before(end); time.Sleep(10 * time.Millisecond) {
			settle()
			if done() {
				return
			}
		}
		script.mu.Lock()
		defer script.mu.Unlock()
		t.Fatalf("timed out waiting for %s; model results: %q; visible text: %q", what, script.results, tester.Texts())
	}
	shot := func(name string) {
		t.Helper()
		dir := os.Getenv("SAILRY_UI_SHOTS")
		if dir == "" {
			return
		}
		frames() // past a popover's arrival
		file, err := os.Create(filepath.Join(dir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tester.Image()); err != nil {
			t.Fatal(err)
		}
	}
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatalf("%v; visible text: %q", err, tester.Texts())
		}
		settle()
	}
	// role clicks the element of a role named name, the last of them: a
	// popover's and a menu's come after the page's. One a popover's scroll
	// clips is scrolled into view first.
	role := func(name string, roles ...ui.Role) {
		t.Helper()
		for range 12 {
			var found, box *ui.AccessInfo
			for _, info := range tester.AccessAll() {
				if info.Name == name && slices.Contains(roles, info.Role) {
					found = &info
				}
				if info.Role == ui.RoleScroll && info.Bounds.X > 600 && info.Bounds.W < 400 {
					box = &info
				}
			}
			inside := found != nil && found.Bounds.H >= 14
			if inside && box != nil && found.Bounds.X >= box.Bounds.X && found.Bounds.X < box.Bounds.X+box.Bounds.W {
				middle := found.Bounds.Y + found.Bounds.H/2
				inside = middle > box.Bounds.Y+4 && middle < box.Bounds.Y+box.Bounds.H-4
			}
			if inside {
				tester.ClickAt(found.Bounds.X+found.Bounds.W/2, found.Bounds.Y+found.Bounds.H/2)
				settle()
				return
			}
			if box == nil {
				break
			}
			delta := float32(80) // further down
			if found != nil && found.Bounds.Y < box.Bounds.Y+box.Bounds.H/2 {
				delta = -80
			}
			tester.Scroll(box.Bounds.X+box.Bounds.W/2, box.Bounds.Y+box.Bounds.H/2, 0, delta)
			frames()
		}
		t.Fatalf("no %v named %q in view; visible text: %q", roles, name, tester.Texts())
	}
	typed := func(field, value string) {
		t.Helper()
		role(field, ui.RoleTextField)
		tester.Type(value)
		settle()
	}
	button := func(name string) ui.AccessInfo {
		t.Helper()
		var found *ui.AccessInfo
		for _, info := range tester.AccessAll() {
			if info.Name == name && info.Role == ui.RoleButton {
				found = &info
			}
		}
		if found == nil {
			t.Fatalf("no button named %q; visible text: %q", name, tester.Texts())
		}
		return *found
	}
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	settle()
	if tester.HasText(text("close")) {
		click(text("close"))
	}

	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project, _ := detail[protocol.Project](do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Interactions", Path: root, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}}).Data)
	do(protocol.Command{Kind: "install_mcp", Data: protocol.CommandInstallMcpPayload{Name: pkg,
		Definition: protocol.PluginMcpDefinition{Kind: "streamable-http", Data: &protocol.PluginMcpDefinitionStreamableHttpPayload{Url: "https://fixture.test/mcp", Headers: []string{}}}}})
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Interaction fixture", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "http://127.0.0.1:9/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	create := func(mode protocol.WorkMode) protocol.Session {
		t.Helper()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", mode, protocol.PermissionAsk
		session, valid := detail[protocol.Session](do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data)
		if !valid {
			t.Fatal("the created session was not returned")
		}
		wait("the session on screen", func() bool {
			r.OpenSession(session.ID.String())
			return r.App.Sidebar.Selected == session.ID.String()
		})
		return session
	}
	current := func(id protocol.SessionID) protocol.Session {
		for _, session := range controller.Snapshot().Sessions {
			if session.ID == id {
				return session
			}
		}
		t.Fatal("the session left the snapshot")
		return protocol.Session{}
	}
	submit := func(id protocol.SessionID, message string) {
		t.Helper()
		input := protocol.DefaultConversationInput()
		input.Text = message
		do(protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: id, ExpectedRevision: current(id).Revision, Message: input}})
	}
	runs := func(id protocol.SessionID) []protocol.ConversationRun {
		state, _ := controller.Conversation(id)
		return state.Snapshot.Page.Runs
	}
	completed := func(id protocol.SessionID, count int) {
		t.Helper()
		wait("the turn to complete", func() bool {
			list := runs(id)
			return len(list) == count && list[len(list)-1].Status == "completed"
		})
	}
	reply, submitLabel, cancelLabel := text("question_reply"), text("interaction_submit"), text("interaction_cancel")
	// asked waits for the question's capsule and opens its popover; the
	// popover stays open from one question to the next.
	asked := func(prompt string) {
		t.Helper()
		wait("the question "+prompt, func() bool { return tester.HasText(reply) })
		if !tester.HasText(cancelLabel) && !tester.HasText(text("interaction_continue")) {
			role(reply, ui.RoleButton, ui.RolePopUpButton, ui.RoleMenuButton)
		}
		if !tester.HasText(prompt) {
			t.Fatalf("the question does not show %q: %q", prompt, tester.Texts())
		}
	}
	answered := func(id protocol.SessionID, count int, call string, parts ...string) {
		t.Helper()
		completed(id, count)
		for _, part := range parts {
			if !strings.Contains(script.result(call), part) {
				t.Fatalf("%s returned %q to the model, want %q in it", call, script.result(call), part)
			}
		}
		if tester.HasText(reply) {
			t.Fatalf("the answered question still waits: %q", tester.Texts())
		}
	}

	session := create(protocol.WorkModeCode).ID

	// A single choice: Submit waits for a choice, then the Node takes it.
	submit(session, "scenario:choice")
	asked("Which loader?")
	shot("question-choice")
	if !button(submitLabel).Disabled {
		t.Fatal("Submit is enabled without a choice")
	}
	role("Use the spinner", ui.RoleRadio)
	click(submitLabel)
	answered(session, 1, "choice-ask", "Use the spinner")

	// The session has a turn now. The statistics package publishes its
	// figures for the context bar, the turns among them with their row, and
	// its context panel is a header button the window opens and closes
	// while it stays enabled.
	shown := r.App.Data.Find(session.String())
	registry := r.pluginRegistry(composerPluginScope(r, shown, r.App.Composer(shown.ID)), shown.ID)
	var turns string
	wait("the statistics package's turns figure", func() bool {
		for _, entry := range registry.Entries(protocol.PluginUiSlotStatistics) {
			if entry.Key.Package.Name == pluginStatisticsPackage && entry.Key.ID == "composer_turns" && len(entry.State.Details) == 1 {
				return json.Unmarshal(entry.State.Value, &turns) == nil && turns != ""
			}
		}
		return false
	})
	// The finished turn's statistics carry what the package's context panel
	// draws beyond the figures: the compaction budget of the selected model,
	// the request with its turn, the latency and the turn's phases, where the
	// answer the turn waited for is not active time.
	wait("the finished turn's statistics", func() bool {
		history, _ := r.controller.Conversation(session)
		statistics := history.Snapshot.Statistics
		if statistics == nil || statistics.Observations.Phases.ActiveUs == nil {
			return false
		}
		observed, phases := statistics.Observations, statistics.Observations.Phases
		if statistics.Budget == nil || statistics.Budget.TriggerTokens == 0 || statistics.Budget.OutputReserve == 0 ||
			len(observed.Recent) != 1 || observed.Recent[0].TurnIndex == nil || *observed.Recent[0].TurnIndex != 1 ||
			observed.Latency.FirstTokenUs == nil || observed.Latency.ElapsedUs == nil || observed.Tools["ask_user"] != 1 ||
			phases.WaitingUs == nil || phases.OutputUs == nil || phases.ToolsUs == nil || phases.OtherUs == nil ||
			*phases.WaitingUs+*phases.OutputUs+*phases.ToolsUs+*phases.OtherUs != *phases.ActiveUs {
			body, _ := json.Marshal(statistics)
			t.Fatalf("the finished turn's statistics are incomplete: %s", body)
		}
		return true
	})
	if _, compact := tester.AccessByRole(ui.RoleButton, text("composer_statistics")); !compact && !slices.Contains(tester.Texts(), turns) {
		t.Fatalf("the context bar shows neither the turns figure %q nor the statistics button: %q", turns, tester.Texts())
	}
	contextPanel := func() ui.AccessInfo {
		t.Helper()
		for _, info := range tester.AccessAll() {
			// The statistics package's header control, as its manifest labels it.
			if info.Role == ui.RoleButton && info.Name == "上下文" && info.Bounds.Y+info.Bounds.H <= shell.HeaderHeight {
				return info
			}
		}
		t.Fatalf("the header has no context panel button; visible text: %q", tester.Texts())
		return ui.AccessInfo{}
	}
	for _, open := range []bool{true, false} {
		at := contextPanel()
		if at.Disabled || at.Expanded == open {
			t.Fatalf("the context panel button before a press: disabled %v, expanded %v", at.Disabled, at.Expanded)
		}
		tester.ClickAt(at.Bounds.X+at.Bounds.W/2, at.Bounds.Y+at.Bounds.H/2)
		settle()
		if at = contextPanel(); at.Disabled || at.Expanded != open {
			t.Fatalf("a press left the context panel button disabled %v, expanded %v, want expanded %v", at.Disabled, at.Expanded, open)
		}
		if open {
			shot("context-panel")
		}
	}

	// Other: the free answer of a single choice.
	submit(session, "scenario:other")
	asked("Which loader then?")
	role(text("question_other"), ui.RoleRadio)
	if !button(submitLabel).Disabled {
		t.Fatal("Submit is enabled with an empty other answer")
	}
	typed(text("interaction_input"), "A progress bar")
	shot("question-other")
	click(submitLabel)
	answered(session, 2, "other-ask", "A progress bar")

	// Several choices and another answer.
	submit(session, "scenario:multi")
	asked("Which checks?")
	role("Build", ui.RoleCheckBox)
	role("Race", ui.RoleCheckBox)
	role(text("question_other"), ui.RoleCheckBox)
	typed(text("interaction_input"), "Lint")
	shot("question-multiple")
	click(submitLabel)
	answered(session, 3, "multi-ask", "Build", "Race", "Lint")
	if strings.Contains(script.result("multi-ask"), "Vet") {
		t.Fatalf("the unchecked option was answered: %q", script.result("multi-ask"))
	}

	// A free answer.
	submit(session, "scenario:text")
	asked("Name the branch")
	if !button(submitLabel).Disabled {
		t.Fatal("Submit is enabled without text")
	}
	typed(text("interaction_input"), "wt/questions")
	shot("question-text")
	click(submitLabel)
	answered(session, 4, "text-ask", "wt/questions")

	// An MCP form with every field type: Submit waits until each included
	// field fits its schema, then the server receives the structured values.
	submit(session, "scenario:form")
	asked("Describe the release")
	for _, shown := range []string{"Name *", "Two to eight letters", "Count *", text("question_optional", "field", "Ratio"), "Agree *", "Color *", "Tags *", text("question_optional", "field", "Note"), text("question_decline")} {
		if !tester.HasText(shown) {
			t.Fatalf("the form does not show %q: %q", shown, tester.Texts())
		}
	}
	shot("question-form-empty")
	typed("Name", "v")
	typed("Count", "9")
	role(text("question_yes"), ui.RoleRadio)
	role("Blue", ui.RoleRadio)
	role("Tag A", ui.RoleCheckBox)
	role("Tag C", ui.RoleCheckBox)
	if !button(submitLabel).Disabled {
		t.Fatal("Submit is enabled with a short name and a count over its maximum")
	}
	typed("Name", "2")
	if !button(submitLabel).Disabled {
		t.Fatal("Submit is enabled with a count over its maximum")
	}
	role("Count", ui.RoleTextField)
	tester.Key(0, ui.KeyBackspace)
	tester.Type("3")
	settle()
	if button(submitLabel).Disabled {
		t.Fatal("Submit is disabled with every required field valid")
	}
	role("Tag B", ui.RoleCheckBox)
	if !button(submitLabel).Disabled {
		t.Fatal("Submit is enabled with more tags than the maximum")
	}
	role("Tag B", ui.RoleCheckBox)
	role(text("question_optional", "field", "Ratio"), ui.RoleCheckBox)
	typed("Ratio", "1.5")
	if !button(submitLabel).Disabled {
		t.Fatal("Submit is enabled with an included ratio over its maximum")
	}
	role("Ratio", ui.RoleTextField)
	tester.Key(0, ui.KeyBackspace)
	tester.Key(0, ui.KeyBackspace)
	tester.Key(0, ui.KeyBackspace)
	tester.Type("0.5")
	settle()
	shot("question-form")
	click(submitLabel)
	answered(session, 5, "form-call", "form:accept")
	form := <-elicited
	if got, _ := json.Marshal(form.Content); form.Action != "accept" || string(got) != `{"agree":true,"color":"blue","count":3,"name":"v2","ratio":0.5,"tags":["a","c"]}` {
		t.Fatalf("the MCP server received %s %s", form.Action, got)
	}

	// A link this Desktop would not open is not one to answer as opened,
	// whatever Node sent it.
	for _, row := range []struct {
		data any
		open bool
	}{
		{protocol.ConversationQuestionInputUrlPayload{Url: authorizeURL}, true},
		{&protocol.ConversationQuestionInputUrlPayload{Url: "http://127.0.0.1:8080/callback"}, true},
		{protocol.ConversationQuestionInputUrlPayload{Url: "http://fixture.test/authorize"}, false},
		{protocol.ConversationQuestionInputUrlPayload{Url: "file:///etc/passwd"}, false},
		{protocol.ConversationQuestionInputUrlPayload{Url: "https://user@fixture.test/"}, false},
		{protocol.ConversationQuestionInputUrlPayload{}, false},
		{nil, false},
	} {
		if link, ok := questionLink(protocol.ConversationQuestionInput{Kind: "url", Data: row.data}); ok != row.open || ok != (link != "") {
			t.Fatalf("link question %+v opens %q, %v", row.data, link, ok)
		}
	}

	// An authorization link: Open opens the system browser and answers.
	submit(session, "scenario:link")
	asked("Authorize the fixture")
	if !tester.HasText(authorizeURL) || !tester.HasText(text("question_decline")) || !tester.HasText(cancelLabel) {
		t.Fatalf("the link question is incomplete: %q", tester.Texts())
	}
	shot("question-link")
	click(text("question_open_url"))
	answered(session, 6, "link-call", "authorize:accept")
	if opened := <-elicited; opened.Action != "accept" || !slices.Equal(tester.OpenedURLs(), []string{authorizeURL}) {
		t.Fatalf("the link answered %s and opened %q", opened.Action, tester.OpenedURLs())
	}

	// Deny takes no reason: the popover has no reason field and Deny no
	// menu (AGENTS.md, Removed UI). The first send loses its outcome: the
	// request stays for Retry, which sends that same request once while it
	// travels.
	approvals := text("approval_live_count", "count", 1)
	submit(session, "scenario:deny")
	wait("the approval", func() bool { return tester.HasText(approvals) })
	fields := func() (count int) {
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleTextField {
				count++
			}
		}
		return count
	}
	closed := fields()
	// The popover opens by itself for a request that arrives.
	if !tester.HasText(text("approval_allow_once")) {
		click(approvals)
	}
	wait("the approval's details", func() bool {
		waiting := r.App.Sidebar.Data.Find(session.String()).Waiting.Approvals
		return len(waiting) == 1 && tester.HasText(text("approval_allow_once")) && !button(text("approval_allow_once")).Disabled
	})
	shot("approval")
	if open := fields(); open != closed {
		t.Fatalf("the approval popover adds a field: %d text fields, %d without it", open, closed)
	}
	setHook(func(request protocol.Request) error {
		if request.Command.Kind == "resolve_approval" {
			return &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the outcome is unknown"}}
		}
		return nil
	})
	click(text("approval_reject"))
	wait("the uncertain decision", func() bool { return tester.HasText(text("chat_action_unknown")) })
	if len(dispatched("resolve_approval")) != 1 || script.result("deny-call") != "" || !tester.HasText(approvals) {
		t.Fatalf("the lost decision was not kept: %d sent, %q", len(dispatched("resolve_approval")), tester.Texts())
	}
	shot("approval-retry")
	release := make(chan struct{})
	free := sync.OnceFunc(func() { close(release) })
	defer free()
	setHook(func(request protocol.Request) error {
		if request.Command.Kind == "resolve_approval" {
			<-release
		}
		return nil
	})
	retry := button(text("chat_retry"))
	tester.ClickAt(retry.Bounds.X+retry.Bounds.W/2, retry.Bounds.Y+retry.Bounds.H/2)
	for end := time.Now().Add(10 * time.Second); len(dispatched("resolve_approval")) < 2 && time.Now().Before(end); time.Sleep(10 * time.Millisecond) {
		frames()
	}
	frames()
	if !tester.HasText(text("approval_reject")) { // the toast's Retry closed the popover
		capsule := button(approvals)
		tester.ClickAt(capsule.Bounds.X+capsule.Bounds.W/2, capsule.Bounds.Y+capsule.Bounds.H/2)
		frames()
	}
	// While the decision travels the request is locked: Deny shows busy
	// and takes no second press.
	busy := button(text("approval_reject"))
	if !busy.Disabled {
		t.Fatalf("Deny is enabled while the decision travels: %q", tester.Texts())
	}
	shot("approval-busy")
	tester.ClickAt(busy.Bounds.X+busy.Bounds.W/2, busy.Bounds.Y+busy.Bounds.H/2)
	tester.ClickAt(retry.Bounds.X+retry.Bounds.W/2, retry.Bounds.Y+retry.Bounds.H/2)
	frames()
	free()
	setHook(nil)
	completed(session, 7)
	requests := dispatched("resolve_approval")
	if len(requests) != 2 || requests[0].ID != requests[1].ID {
		t.Fatalf("the decision was sent %d times, as one request: %v", len(requests), len(requests) == 2 && requests[0].ID == requests[1].ID)
	}
	if got := script.result("deny-call"); got == "" || strings.Contains(got, "Reason:") {
		t.Fatalf("the model received %q for the denied call", got)
	}
	select {
	case value := <-written:
		t.Fatalf("the denied tool ran with %q", value)
	default:
	}
	if tester.HasText(approvals) || tester.HasText(text("approval_receipt")) {
		t.Fatalf("the decided approval still waits: %q", tester.Texts())
	}

	// Allow once runs the call.
	submit(session, "scenario:allow")
	wait("the second approval", func() bool { return tester.HasText(approvals) })
	if !tester.HasText(text("approval_allow_once")) {
		click(approvals)
	}
	wait("the approval's details", func() bool { return !button(text("approval_allow_once")).Disabled })
	click(text("approval_allow_once"))
	completed(session, 8)
	if value := <-written; value != "allowed" || !strings.Contains(script.result("allow-call"), "wrote:allowed") {
		t.Fatalf("the allowed tool wrote %q and returned %q", value, script.result("allow-call"))
	}

	// The window sends no reason, the Node still takes one from a client
	// that does (deny_with_reason) and gives it to the model.
	submit(session, "scenario:reason")
	var pending protocol.ApprovalID
	wait("the third approval", func() bool {
		waiting := r.App.Sidebar.Data.Find(session.String()).Waiting.Approvals
		if len(waiting) != 1 {
			return false
		}
		id, err := protocol.ParseApprovalID(waiting[0].ID)
		pending = id
		return err == nil
	})
	do(protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session, Approval: pending,
		Decision: protocol.ConversationDecision{Kind: "deny_with_reason", Data: "Use the staging bucket"}}})
	completed(session, 9)
	if got := script.result("reason-call"); !strings.Contains(got, "Tool execution was denied. Reason: Use the staging bucket") {
		t.Fatalf("the model received %q for the call denied with a reason", got)
	}
	select {
	case value := <-written:
		t.Fatalf("the denied tool ran with %q", value)
	default:
	}

	// An SSH host key is asked for as its trust prompt.
	trust := projectApproval(protocol.ConversationApproval{Ssh: &protocol.SshTrust{Host: "build.example", Port: 2222, Key: protocol.SshHostKey{Algorithm: "ssh-ed25519", Fingerprint: "SHA256:fixture"}, Changed: true}}, nil, "zh-CN").Trust
	if trust == nil || *trust != (composer.Trust{Host: "build.example", Port: 2222, Algorithm: "ssh-ed25519", Fingerprint: "SHA256:fixture", Changed: true}) {
		t.Fatalf("the SSH trust was projected as %+v", trust)
	}

	// A plan review: Continue keeps planning, Start switches the session to
	// coding and starts the coding turn.
	planning := create(protocol.WorkModePlan).ID
	confirm := text("plan_execute_confirm")
	submit(planning, "scenario:plan")
	asked(confirm)
	if tester.HasText(cancelLabel) || tester.HasText(text("question_decline")) {
		t.Fatalf("the plan review offers more than Continue and Start: %q", tester.Texts())
	}
	shot("question-plan")
	click(text("interaction_continue"))
	wait("the plan review to close", func() bool { return !tester.HasText(reply) })
	if current(planning).Config.Mode != protocol.WorkModePlan || len(runs(planning)) != 1 {
		t.Fatalf("Continue left planning: %s, %d runs", current(planning).Config.Mode, len(runs(planning)))
	}
	submit(planning, "scenario:plan again")
	asked(confirm)
	click(text("interaction_start"))
	completed(planning, 3)
	if mode := current(planning).Config.Mode; mode != protocol.WorkModeCode {
		t.Fatalf("Start coding left the session in %s", mode)
	}
	state, _ := controller.Conversation(planning)
	if got := sentInput(state.Snapshot.Page, runs(planning)[2].Turn).Text; got != text("plan_coding_message") {
		t.Fatalf("the coding turn was sent %q", got)
	}
	if tester.HasText(reply) {
		t.Fatalf("the accepted plan still waits: %q", tester.Texts())
	}
	shot("plan-accepted")
}
