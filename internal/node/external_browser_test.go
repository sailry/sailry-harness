package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/externalbrowser"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestExternalBrowserRouting proves the Node side of the managed browser
// without starting one: the public command's checks and admission, plugin
// authority, the bundled package's tools in a real turn, approval and plan
// mode. Every call here ends before a browser would be acquired.
func TestExternalBrowserRouting(t *testing.T) {
	tool := func(name string) string { return operations.Alias("external-browser", name) }
	worker, planner := &mediaScript{results: map[string]string{}}, &mediaScript{results: map[string]string{}}
	late := &hookedScript{mediaScript: &mediaScript{results: map[string]string{}}}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, Model: func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
			switch binding.Config.Model {
			case "worker":
				return worker, nil
			case "planner":
				return planner, nil
			case "late":
				return late, nil
			}
			return nil, errors.New("unexpected model " + binding.Config.Model)
		},
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("browser routing must not launch a process")
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	if externalbrowser.Installed() == "" {
		t.Skip("no Chrome, Chromium or Edge is installed on this host; every call answers not_configured")
	}
	var declared []string
	for _, declaration := range commandFixture(t, node, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "external-browser"}}).Data.(protocol.PluginInfo).Extension.Tools {
		declared = append(declared, tool(declaration.Name))
	}
	// Discovery loads at most eight tools per call.
	var discover [][3]string
	for chunk := range slices.Chunk(declared, 8) {
		discover = append(discover, [3]string{"discover" + strconv.Itoa(len(discover)), "tool_search", `{"limit":8,"query":"select:` + strings.Join(chunk, ",") + `"}`})
	}
	worker.steps = [][][3]string{
		discover,
		{{"local", tool("browser_navigate"), `{"url":"file:///etc/passwd"}`}},
		{{"mismatch", tool("browser_wait"), `{"seconds":-1}`}},
		{{"close", tool("browser_close_session"), `{}`}},
	}
	planner.steps = [][][3]string{
		{{"discover", "tool_search", `{"query":"select:` + tool("browser_click") + `"}`}},
		{{"click", tool("browser_click"), `{"selector":"#submit"}`}},
	}
	secret := protocol.NewSecret("browser-key")
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Browser fixture", true
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://127.0.0.1:1/v1", "worker"
	for _, id := range []string{"worker", "planner", "late"} {
		entry := protocol.DefaultConversationModel()
		entry.ID, entry.Context, entry.Output, entry.Tools, entry.CustomCapabilities = id, 32000, 8000, true, true
		provider.Models = append(provider.Models, entry)
	}
	commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Browser routing", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	create := func(name string, mode protocol.WorkMode) protocol.Session {
		t.Helper()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, name, mode, protocol.PermissionFull
		return commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	}
	use := func(session protocol.SessionID, worktree protocol.WorktreeID, action protocol.ExternalBrowserAction, arguments string, scope *protocol.PluginContext) (protocol.Output, *protocol.Fault) {
		t.Helper()
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "use_external_browser", Data: &protocol.CommandUseExternalBrowserPayload{
			Session: session, Worktree: worktree, Action: action, Arguments: json.RawMessage(arguments)}})
		request.Plugin = scope
		accepted, err := node.Dispatch(context.Background(), node.ID(), request)
		if err != nil {
			var fault *protocol.Fault
			if !errors.As(err, &fault) {
				t.Fatal(err)
			}
			return protocol.Output{}, fault
		}
		if !accepted.Receipt.Durable {
			t.Fatal("managed browser command was not admitted durably")
		}
		value := <-accepted.Completion
		return value.Output, value.Fault
	}
	refused := func(fault *protocol.Fault, code protocol.ErrorCode, message string) {
		t.Helper()
		if fault == nil || fault.Code != code || !strings.Contains(fault.Message, message) {
			t.Fatalf("fault = %+v, want %s: %s", fault, code, message)
		}
	}
	closed := func(output protocol.Output) {
		t.Helper()
		body, err := json.Marshal(output.Data)
		if err != nil {
			t.Fatal(err)
		}
		var data struct {
			Closed bool            `json:"closed"`
			Node   protocol.NodeID `json:"execution_node"`
		}
		if output.Kind != "external_browser" || json.Unmarshal(body, &data) != nil || !data.Closed || data.Node != node.ID() {
			t.Fatalf("close output = %s %s", output.Kind, body)
		}
	}

	t.Run("public command", func(t *testing.T) {
		session := create("worker", protocol.WorkModeCode)
		output, fault := use(session.ID, session.Worktree, protocol.ExternalBrowserActionCloseSession, `{}`, nil)
		if fault != nil {
			t.Fatal(fault)
		}
		closed(output)
		_, fault = use(session.ID, session.Worktree, protocol.ExternalBrowserActionNavigate, `{"url":"https://user:secret@example.com/"}`, nil)
		refused(fault, protocol.ErrorCodeInvalidRequest, "external browser URLs must use HTTP or HTTPS without credentials")
		_, fault = use(session.ID, session.Worktree, protocol.ExternalBrowserActionClick, `[]`, nil)
		refused(fault, protocol.ErrorCodeInvalidRequest, "browser arguments must be an object of at most 64 KiB")
		_, fault = use(session.ID, session.Worktree, protocol.ExternalBrowserActionEvaluateJs, `{"script":"`+strings.Repeat("a", 64*1024)+`"}`, nil)
		refused(fault, protocol.ErrorCodeInvalidRequest, "browser arguments must be an object of at most 64 KiB")
		_, fault = use(session.ID, protocol.NewWorktreeID(), protocol.ExternalBrowserActionCloseSession, `{}`, nil)
		refused(fault, protocol.ErrorCodePermissionDenied, "resource session belongs to another worktree")
		_, fault = use(protocol.NewSessionID(), session.Worktree, protocol.ExternalBrowserActionCloseSession, `{}`, nil)
		if fault == nil || fault.Code != protocol.ErrorCodeNotFound {
			t.Fatalf("unknown session fault = %+v", fault)
		}
		if sessions := node.externalBrowser.Sessions(); len(sessions) != 0 {
			t.Fatalf("refused calls left browser sessions %v", sessions)
		}

		// A package may use the browser only while it is enabled and only
		// inside the scope it was given.
		read := func() protocol.PluginInfo {
			return commandFixture(t, node, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "external-browser"}}).Data.(protocol.PluginInfo)
		}
		info := read()
		if info.Summary.Enabled {
			t.Fatal("the managed browser package is enabled by default")
		}
		scope := &protocol.PluginContext{Package: plugins.PackageReference(info), Worktree: &session.Worktree, Session: &session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
		_, fault = use(session.ID, session.Worktree, protocol.ExternalBrowserActionCloseSession, `{}`, scope)
		refused(fault, protocol.ErrorCodeNotConfigured, "plugin is disabled")
		commandFixture(t, node, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: "external-browser", ExpectedRevision: info.Summary.Revision, Enabled: true}})
		info = read()
		scope.Package = plugins.PackageReference(info)
		output, fault = use(session.ID, session.Worktree, protocol.ExternalBrowserActionCloseSession, `{}`, scope)
		if fault != nil {
			t.Fatal(fault)
		}
		closed(output)
		other := create("worker", protocol.WorkModeCode)
		_, fault = use(other.ID, other.Worktree, protocol.ExternalBrowserActionCloseSession, `{}`, scope)
		refused(fault, protocol.ErrorCodePermissionDenied, "plugin action is outside the captured resource scope")
		// The declared action follows the browser action, not the command.
		limited := info
		extension := *info.Extension
		extension.Actions = []protocol.PluginAction{protocol.PluginActionReadExternalBrowser}
		limited.Extension = &extension
		check := func(action protocol.ExternalBrowserAction) error {
			request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "use_external_browser", Data: &protocol.CommandUseExternalBrowserPayload{
				Session: session.ID, Worktree: session.Worktree, Action: action, Arguments: json.RawMessage(`{}`)}})
			request.Plugin = scope
			return node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
				return store.CheckPluginRequest(t.Context(), db, request, limited)
			})
		}
		if err := check(protocol.ExternalBrowserActionPageInfo); err != nil {
			t.Fatalf("read action needs more than the read permission: %v", err)
		}
		var denied *protocol.Fault
		if err := check(protocol.ExternalBrowserActionClick); !errors.As(err, &denied) || denied.Code != protocol.ErrorCodePermissionDenied {
			t.Fatalf("control action passed with the read permission: %v", err)
		}
	})

	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	run := func(t *testing.T, name string, mode protocol.WorkMode) {
		t.Helper()
		session := create(name, mode)
		commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Open the page"},
		}})
		for completed := false; !completed; {
			var history protocol.ConversationHistory
			if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, run := range history.Page.Runs {
				switch run.Status {
				case protocol.ConversationStatusCompleted:
					completed = true
				case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
					t.Fatalf("browser turn failed: %+v", run)
				}
			}
			if !completed {
				if _, err := subscription.Next(t.Context()); err != nil {
					t.Fatal(err)
				}
			}
		}
	}

	t.Run("turn offers and executes the package tools", func(t *testing.T) {
		run(t, "worker", protocol.WorkModeCode)
		worker.mu.Lock()
		offered, results := worker.offered, worker.results
		worker.mu.Unlock()
		if len(offered) != 5 {
			t.Fatalf("browser turn made %d model requests: %v", len(offered), results)
		}
		for _, name := range declared {
			// The built-in browser package declares some of the same names;
			// those keep their aliases and the rest use the declared names.
			if !slices.Contains(offered[1], name) && !slices.Contains(offered[1], declaredToolName(name)) {
				t.Fatalf("declared tool %s was not offered: %v", name, offered[1])
			}
		}
		if len(declared) != 49 {
			t.Fatalf("package declares %d tools", len(declared))
		}
		if !strings.Contains(results["local"], "external browser URLs must use HTTP or HTTPS without credentials") {
			t.Fatalf("local URL result = %q", results["local"])
		}
		if !strings.Contains(results["mismatch"], "browser wait seconds must be finite and non-negative") {
			t.Fatalf("negative wait result = %q", results["mismatch"])
		}
		// Closing is a control action: full permission approves it, and it
		// runs through the Node's browser owner.
		var data struct {
			Closed bool            `json:"closed"`
			Node   protocol.NodeID `json:"execution_node"`
		}
		if json.Unmarshal([]byte(results["close"]), &data) != nil || !data.Closed || data.Node != node.ID() {
			t.Fatalf("close result = %q", results["close"])
		}
		if sessions := node.externalBrowser.Sessions(); len(sessions) != 0 {
			t.Fatalf("turn left browser sessions %v", sessions)
		}
	})

	t.Run("plan mode refuses control actions", func(t *testing.T) {
		run(t, "planner", protocol.WorkModePlan)
		planner.mu.Lock()
		offered, results := planner.offered, planner.results
		planner.mu.Unlock()
		if len(offered) != 3 || !strings.Contains(results["click"], "planning mode permits read-only tools") {
			t.Fatalf("plan turn requests = %d, click result = %q", len(offered), results["click"])
		}
		if sessions := node.externalBrowser.Sessions(); len(sessions) != 0 {
			t.Fatalf("plan turn started browser sessions %v", sessions)
		}
	})

	t.Run("a package disabled during its turn stops driving the browser", func(t *testing.T) {
		late.steps = [][][3]string{
			{{"discover", "tool_search", `{"query":"select:` + tool("browser_close_session") + `"}`}},
			{{"enabled", tool("browser_close_session"), `{}`}},
			{{"disabled", tool("browser_close_session"), `{}`}},
		}
		late.before = func(step int) {
			if step != 3 {
				return
			}
			accepted, err := node.Dispatch(context.Background(), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "external-browser"}}))
			if err != nil {
				t.Error(err)
				return
			}
			info := (<-accepted.Completion).Output.Data.(protocol.PluginInfo)
			accepted, err = node.Dispatch(context.Background(), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "set_plugin_enabled",
				Data: protocol.CommandSetPluginEnabledPayload{Name: "external-browser", ExpectedRevision: info.Summary.Revision, Enabled: false}}))
			if err != nil {
				t.Error(err)
				return
			}
			if outcome := <-accepted.Completion; outcome.Fault != nil {
				t.Error(outcome.Fault)
			}
		}
		run(t, "late", protocol.WorkModeCode)
		late.mu.Lock()
		results := late.results
		late.mu.Unlock()
		if !strings.Contains(results["enabled"], `"closed":true`) {
			t.Fatalf("call of the enabled package = %q", results["enabled"])
		}
		if !strings.Contains(results["disabled"], "plugin is disabled") || strings.Contains(results["disabled"], `"closed"`) {
			t.Fatalf("call of the disabled package = %q", results["disabled"])
		}
	})
}
