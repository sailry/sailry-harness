package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"image"
	"image/png"
	"os"
	"path"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// agentToolScript is a fake model whose calls for each request come from the
// running subtest. It records the offered tools, the context and every result.
type agentToolScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	offered [][]string
	prompts []string
	results map[string]string
	plan    func(step int, results map[string]string) [][3]string
}

// declaredToolName is the name a model reads for a package tool's alias when
// no other captured tool declares the same name.
func declaredToolName(alias string) string {
	if rest, found := strings.CutPrefix(alias, "plugin_"); found && len(rest) > 17 && rest[16] == '_' {
		return rest[17:]
	}
	return alias
}

func (script *agentToolScript) reset(plan func(int, map[string]string) [][3]string) {
	script.mu.Lock()
	defer script.mu.Unlock()
	script.offered, script.prompts, script.results, script.plan = nil, nil, map[string]string{}, plan
}

func (script *agentToolScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	var names []string
	for _, tool := range call.Tools {
		names = append(names, tool.GetName())
	}
	script.offered = append(script.offered, names)
	var prompt strings.Builder
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if text, found := fantasy.AsMessagePart[fantasy.TextPart](part); found {
				prompt.WriteString(text.Text + "\n")
			}
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found {
				if text, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output); found {
					script.results[result.ToolCallID] = text.Text
				}
			}
		}
	}
	script.prompts = append(script.prompts, prompt.String())
	calls := script.plan(len(script.offered), script.results)
	script.mu.Unlock()
	return func(yield func(fantasy.StreamPart) bool) {
		for _, call := range calls {
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: call[0], ToolCallName: call[1], ToolCallInput: call[2]}) {
				return
			}
		}
		if len(calls) == 0 && !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

// TestAgentManagementTools runs real turns in which the model discovers and
// calls the plugin, MCP and skill managers, the role and team catalog tools,
// a package's roles.* operations and the files package's OCR model tools.
func TestAgentManagementTools(t *testing.T) {
	script := &agentToolScript{}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("management tools must not launch a process")
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	fixture := t.TempDir()
	if err := os.WriteFile(filepath.Join(fixture, "plugin.json"), []byte(`{"$schema":"`+plugins.ManifestSchema+`","name":"roles-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["roles.read","roles.write"],"tools":[{"name":"roster","operation":"roles.list"},{"name":"keep","operation":"roles.save"},{"name":"drop","operation":"roles.remove"},{"name":"assign","operation":"roles.apply"}]}}}`), 0o600); err != nil {
		t.Fatal(err)
	}
	flows := t.TempDir()
	for name, body := range map[string]string{
		"plugin.json":                 `{"$schema":"` + plugins.ManifestSchema + `","name":"flow-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","scope":"host","actions":["files.write"],"host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"handlers":["step"]},"tools":[{"name":"rewrite","description":"Rewrite a file in steps","handler":{"name":"step","flow":{"operations":["files.read","files.write"]},"parameters":{"type":"object"}}}]}}}`,
		"dev.sailry.platform/main.js": `export function step(input){return input.step===0?{call:{operation:"files.write",arguments:{path:"note.txt",text:"flow",expected_revision:null}}}:{result:{done:true}};}`,
	} {
		filename := filepath.Join(flows, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filename, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	for name, source := range map[string]string{"roles-fixture": fixture, "flow-fixture": flows} {
		info, err := node.plugins.manager.Install(t.Context(), source, "", name)
		if err != nil || len(info.Issues) != 0 || info.Extension == nil {
			t.Fatalf("%s is invalid: %+v, %v", name, info.Issues, err)
		}
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			_, err := store.InstallPlugin(t.Context(), db, info, 0, true)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Management tools", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()

	managers := []string{"manage_skills", "manage_mcp", "manage_plugins"}
	catalog := []string{"list_agents", "add_agent", "edit_agent", "delete_agent", "apply_agents", "list_teams", "add_team", "edit_team", "delete_team", "apply_team"}
	roles := func(name string) string { return operations.Alias("roles-fixture", name) }
	ocr := func(name string) string { return operations.Alias("files", name) }
	scoped := []string{roles("roster"), roles("keep"), roles("drop"), roles("assign"), ocr("list_ocr_models"), ocr("install_ocr_model"), ocr("uninstall_ocr_model")}
	// tool_search loads at most eight tools per call.
	all := slices.Concat(managers, catalog, scoped)
	var discover [][3]string
	for index, names := range [][]string{all[:8], all[8:16], all[16:]} {
		discover = append(discover, [3]string{"discover-" + string(rune('a'+index)), "tool_search", `{"limit":8,"query":"select:` + strings.Join(names, ",") + `"}`})
	}
	field := func(t *testing.T, results map[string]string, call string, path ...string) string {
		t.Helper()
		var value any
		decoder := json.NewDecoder(strings.NewReader(results[call]))
		decoder.UseNumber()
		if err := decoder.Decode(&value); err != nil {
			t.Fatalf("%s result %q is not JSON: %v", call, results[call], err)
		}
		for _, key := range path {
			object, valid := value.(map[string]any)
			if !valid {
				t.Fatalf("%s result %q has no %v", call, results[call], path)
			}
			value = object[key]
		}
		switch value := value.(type) {
		case string:
			return value
		case json.Number:
			return value.String()
		}
		t.Fatalf("%s result %q has no scalar %v", call, results[call], path)
		return ""
	}
	// turn submits one message and settles it, resolving every pending
	// approval with decide. It returns the approvals the turn recorded.
	turn := func(t *testing.T, mode protocol.WorkMode, permission protocol.Permission, decide func(protocol.ConversationApproval) string, plan func(int, map[string]string) [][3]string) (protocol.Session, []protocol.ConversationApproval, protocol.ConversationHistory) {
		t.Helper()
		script.reset(plan)
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", mode, permission
		session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
		commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Manage the catalog"},
		}})
		for {
			var history protocol.ConversationHistory
			if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, approval := range history.Page.Approvals {
				if approval.State == protocol.ConversationApprovalStatePending {
					commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: decide(approval)}}})
				}
			}
			for _, run := range history.Page.Runs {
				switch run.Status {
				case protocol.ConversationStatusCompleted:
					return session, history.Page.Approvals, history
				case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
					t.Fatalf("management turn failed: %+v", run)
				}
			}
			if _, err := subscription.Next(t.Context()); err != nil {
				t.Fatal(err)
			}
		}
	}
	expect := func(t *testing.T, results map[string]string, expected map[string][]string) {
		t.Helper()
		for call, fragments := range expected {
			for _, fragment := range fragments {
				if !strings.Contains(results[call], fragment) {
					t.Errorf("%s result %.300q omits %s", call, results[call], fragment)
				}
			}
		}
	}
	unknown := strings.Repeat("a", 64)

	t.Run("full permission", func(t *testing.T) {
		var results map[string]string
		_, approvals, history := turn(t, protocol.WorkModeCode, protocol.PermissionFull, func(protocol.ConversationApproval) string {
			t.Fatal("full permission must not ask")
			return "deny"
		}, func(step int, current map[string]string) [][3]string {
			results = current
			switch step {
			case 1:
				return discover
			case 2:
				return [][3]string{
					{"plugins", "manage_plugins", `{"action":"list"}`},
					{"inspect", "manage_plugins", `{"action":"inspect","target":{"kind":"installed","name":"roles-fixture"}}`},
					{"mcp-install", "manage_mcp", `{"action":"install","name":"mcp-fixture","definition":{"type":"stdio","command":"fixture-mcp","args":["serve"],"env":["TOKEN"]}}`},
					{"skills", "manage_skills", `{"action":"list"}`},
					{"skill-uninstall", "manage_skills", `{"action":"uninstall","name":"roles-fixture","expected_revision":1}`},
					{"agents", "list_agents", `{}`},
					{"agent", "add_agent", `{"key":"reviewer","name":"Reviewer","instructions":"Review the change"}`},
					{"ocr-models", ocr("list_ocr_models"), `{}`},
					{"ocr-install", ocr("install_ocr_model"), `{"id":"` + unknown + `"}`},
					{"ocr-remove", ocr("uninstall_ocr_model"), `{"id":"` + unknown + `"}`},
				}
			case 3:
				id, revision := field(t, current, "agent", "id"), field(t, current, "agent", "revision")
				return [][3]string{
					{"mcp-list", "manage_mcp", `{"action":"list"}`},
					{"mcp-update", "manage_mcp", `{"action":"update","name":"mcp-fixture","expected_revision":` + field(t, current, "mcp-install", "revision") + `,"clear_credentials":["TOKEN"]}`},
					{"plugin-remove", "manage_plugins", `{"action":"uninstall","name":"mcp-fixture","expected_revision":1}`},
					{"edit", "edit_agent", `{"id":"` + id + `","expected_revision":"` + revision + `","description":"Reviews changes"}`},
					{"stale", "edit_agent", `{"id":"` + id + `","expected_revision":"` + revision + `","description":"Stale"}`},
					{"team", "add_team", `{"key":"review","name":"Review","members":["` + id + `"]}`},
					{"teams", "list_teams", `{}`},
					{"roster", roles("roster"), `{}`},
				}
			case 4:
				id, revision := field(t, current, "edit", "id"), field(t, current, "edit", "revision")
				captured := strings.SplitN(strings.SplitN(script.prompts[0], "Captured conversation revision: ", 2)[1], ".", 2)[0]
				return [][3]string{
					{"mcp-remove", "manage_mcp", `{"action":"uninstall","name":"mcp-fixture","expected_revision":` + field(t, current, "mcp-update", "revision") + `}`},
					{"apply", "apply_agents", `{"agents":[{"id":"` + id + `","revision":"` + revision + `"}],"expected_session_revision":"` + captured + `"}`},
					{"apply-stale", "apply_agents", `{"agents":[],"expected_session_revision":"` + captured + `"}`},
				}
			case 5:
				return [][3]string{
					{"apply-team", "apply_team", `{"team":{"id":"` + field(t, current, "team", "id") + `","revision":"` + field(t, current, "team", "revision") + `"},"expected_session_revision":"` + field(t, current, "apply", "revision") + `"}`},
					{"keep", roles("keep"), `{"role":{"id":"` + field(t, current, "edit", "id") + `","revision":"` + field(t, current, "edit", "revision") + `","key":"reviewer","name":"Lead reviewer","description":"","read_only":true,"skills":[],"instructions":"Review"},"expected_revision":"` + field(t, current, "edit", "revision") + `"}`},
				}
			case 6:
				return [][3]string{
					{"assign", roles("assign"), `{"roles":[],"expected_revision":"` + field(t, current, "apply-team", "revision") + `"}`},
					{"delete-team", "delete_team", `{"id":"` + field(t, current, "team", "id") + `","expected_revision":"` + field(t, current, "team", "revision") + `"}`},
					{"drop", roles("drop"), `{"role":"` + field(t, current, "edit", "id") + `","expected_revision":"` + field(t, current, "keep", "data", "revision") + `"}`},
					{"delete-missing", "delete_agent", `{"id":"` + field(t, current, "edit", "id") + `","expected_revision":"1"}`},
				}
			}
			return nil
		})
		script.mu.Lock()
		offered, prompts := script.offered, script.prompts
		script.mu.Unlock()
		for _, name := range slices.Concat(managers, catalog, scoped[1:]) {
			// The model reads a package tool under its declared name.
			name = declaredToolName(name)
			if slices.Contains(offered[0], name) {
				t.Fatalf("%s must be deferred until discovery: %v", name, offered[0])
			}
			if !slices.Contains(offered[1], name) {
				t.Fatalf("%s was not offered after discovery: %v", name, offered[1])
			}
		}
		if !strings.Contains(prompts[0], "Catalog changes do not change this turn's roster") {
			t.Fatalf("turn context omits the captured conversation revision: %q", prompts[0])
		}
		expect(t, results, map[string][]string{
			"plugins":         {`"plugins":[`, `"name":"files"`, `"name":"roles-fixture"`, `"origin"`},
			"inspect":         {`"summary":{`, `"name":"roles-fixture"`, `"issues":[]`},
			"mcp-install":     {`"name":"mcp-fixture"`, `"standalone":true`, `"credential_slots":["TOKEN"]`, `"transport":"stdio"`},
			"skills":          {`{"skills":[]}`},
			"skill-uninstall": {"permission_denied", "only standalone skill packages can be managed by this tool"},
			"agents":          {`{"agents":[`, `"key":"explore"`, `"revision":"1"`},
			"agent":           {`"key":"reviewer"`, `"revision":"1"`},
			"ocr-models":      {`"models"`},
			"ocr-install":     {"not_found", "local model is not registered"},
			"ocr-remove":      {"not_found", "local model is not registered"},
			"mcp-list":        {`"servers":[`, `"name":"mcp-fixture"`},
			"mcp-update":      {`"name":"mcp-fixture"`, `"credential_slots":["TOKEN"]`, `"standalone":true`},
			"plugin-remove":   {"invalid_request", "use manage_skills or manage_mcp for standalone packages"},
			"edit":            {`"description":"Reviews changes"`, `"revision":"2"`},
			"stale":           {"revision_conflict", "configuration revision changed; reload before editing"},
			"team":            {`"key":"review"`, `"revision":"1"`, `"members":["`},
			"teams":           {`"teams":[`, `"key":"review"`},
			"roster":          {`"kind":"roles"`, `"key":"reviewer"`, `"revision":"2"`},
			"mcp-remove":      {`"servers":[`, `"name":"github"`},
			"apply":           {`"session":"`, `"roles":[{"id":"`, `"revision":"2"`},
			"apply-stale":     {"revision_conflict"},
			"apply-team":      {`"team":{"id":"`, `"roles":[`},
			"keep":            {`"kind":"role"`, `"name":"Lead reviewer"`, `"revision":"3"`},
			"assign":          {`"kind":"session_roles"`, `"roles":[]`},
			"delete-team":     {`[]`},
			"drop":            {`"kind":"roles"`, `"key":"explore"`},
			"delete-missing":  {"not_found"},
		})
		for _, call := range []string{"ocr-install", "ocr-remove"} {
			if strings.Contains(results[call], "permission_denied") {
				t.Fatalf("%s was refused instead of reaching the local model owner: %q", call, results[call])
			}
		}
		for _, approval := range approvals {
			if approval.Source != protocol.ConversationApprovalSourceFull || approval.State != protocol.ConversationApprovalStateApproved || approval.CanRemember {
				t.Fatalf("full permission approval is not automatic: %+v", approval)
			}
		}
		// Every non-read call that passed its pre-approval checks was
		// authorized: the two refused package removals fail before approval,
		// and list_agents, list_teams, the roster and the OCR listing are
		// read-only and never ask.
		if len(approvals) != 21 {
			t.Fatalf("full permission recorded %d approvals", len(approvals))
		}
		body, err := json.Marshal(history)
		if err != nil {
			t.Fatal(err)
		}
		for _, fragment := range []string{`"label":"Plugins"`, `"label":"MCP"`, `"label":"Skills"`, `"label":"Add agent"`, `"label":"Apply team"`, `"zh-CN":"列出代理"`} {
			if !strings.Contains(string(body), fragment) {
				t.Fatalf("recorded tool displays omit %s", fragment)
			}
		}
	})

	t.Run("ask permission", func(t *testing.T) {
		var results map[string]string
		_, approvals, _ := turn(t, protocol.WorkModeCode, protocol.PermissionAsk, func(approval protocol.ConversationApproval) string {
			script.mu.Lock()
			defer script.mu.Unlock()
			// add_agent is approved; the next request's add_team and OCR
			// removal are refused. Only the package operation, which has an
			// approval binding, can be remembered.
			_, added := script.results["agent"]
			if approval.Source != protocol.ConversationApprovalSourceUser || approval.CanRemember != (added && approval.Index == 1) {
				t.Fatalf("unexpected management approval: %+v", approval)
			}
			if added {
				return "deny"
			}
			return "approve"
		}, func(step int, current map[string]string) [][3]string {
			results = current
			switch step {
			case 1:
				return discover
			case 2:
				return [][3]string{{"teams", "list_teams", `{}`}, {"agent", "add_agent", `{"key":"writer","name":"Writer","instructions":"Write"}`}}
			case 3:
				return [][3]string{{"denied", "add_team", `{"key":"writers","name":"Writers"}`}, {"ocr-remove", ocr("uninstall_ocr_model"), `{"id":"` + unknown + `"}`}}
			case 4:
				// An invalid definition is refused before any approval is
				// requested: none of these calls reaches the user.
				return [][3]string{
					{"agents", "list_agents", `{}`},
					{"invalid-agent", "add_agent", `{"key":"Bad Key","name":"Invalid","instructions":"Write"}`},
					{"invalid-edit", "edit_agent", `{"id":"` + field(t, current, "agent", "id") + `","expected_revision":"` + field(t, current, "agent", "revision") + `","name":" "}`},
					{"invalid-team", "add_team", `{"key":"default","name":"Default"}`},
					{"invalid-members", "add_team", `{"key":"pair","name":"Pair","members":["` + field(t, current, "agent", "id") + `","` + field(t, current, "agent", "id") + `"]}`},
					{"invalid-keep", roles("keep"), `{"role":{"id":"` + field(t, current, "agent", "id") + `","revision":"` + field(t, current, "agent", "revision") + `","key":"Bad Key","name":"Writer","description":"","read_only":false,"skills":[],"instructions":"Write"},"expected_revision":"` + field(t, current, "agent", "revision") + `"}`},
				}
			}
			return nil
		})
		expect(t, results, map[string][]string{
			"teams":           {`{"teams":[]}`},
			"agent":           {`"key":"writer"`, `"revision":"1"`},
			"agents":          {`"key":"writer"`},
			"invalid-agent":   {"invalid_request", "role key, name or turn limit is invalid"},
			"invalid-edit":    {"invalid_request", "role key, name or turn limit is invalid"},
			"invalid-team":    {"invalid_request", "team key or name is invalid"},
			"invalid-members": {"invalid_request", "team members must be distinct"},
			"invalid-keep":    {"invalid_request", "role key, name or turn limit is invalid"},
		})
		if strings.Contains(results["denied"], `"key":"writers"`) || strings.Contains(results["ocr-remove"], "not_found") {
			t.Fatalf("denied calls executed: %q, %q", results["denied"], results["ocr-remove"])
		}
		states := map[protocol.ConversationApprovalState]int{}
		for _, approval := range approvals {
			states[approval.State]++
		}
		// The invalid definitions of the last request asked for nothing.
		if len(approvals) != 3 || states[protocol.ConversationApprovalStateApproved] != 1 {
			t.Fatalf("ask permission recorded %+v", approvals)
		}
		teams := commandFixture(t, node, protocol.Command{Kind: "list_teams"}).Data.([]protocol.TeamProfile)
		if len(teams) != 0 {
			t.Fatalf("a denied team was created: %+v", teams)
		}
	})

	// An artifact belongs to its conversation and a package's values to the
	// package: neither asks the user at any permission level. A plain script
	// handler asks for nothing either. Only the artifact survives Plan mode.
	for _, level := range []struct {
		name       string
		mode       protocol.WorkMode
		permission protocol.Permission
	}{{"ask", protocol.WorkModeCode, protocol.PermissionAsk}, {"project", protocol.WorkModeCode, protocol.PermissionProject}, {"plan", protocol.WorkModePlan, protocol.PermissionAsk}} {
		t.Run("own storage under "+level.name, func(t *testing.T) {
			var results map[string]string
			_, approvals, _ := turn(t, level.mode, level.permission, func(approval protocol.ConversationApproval) string {
				t.Errorf("a write to own storage asked the user: %+v", approval)
				return "deny"
			}, func(step int, current map[string]string) [][3]string {
				results = current
				switch step {
				case 1:
					return [][3]string{{"discover", "tool_search", `{"limit":8,"query":"select:write_brief,create_goal,update_goal,save_memory"}`}}
				case 2:
					calls := [][3]string{
						{"brief", "write_brief", `{"title":"Notes","html":"<p>Outline</p>"}`},
						{"memory", "save_memory", `{"kind":"user","title":"Editor theme","body":"Prefers a dark theme"}`},
					}
					// The goals package offers no writing tool to a planning
					// turn. Its plain update handler asks for nothing; with no
					// goal set it answers that, so no goal turns follow.
					// create_goal declares that it asks and is covered with
					// the goal loop.
					if level.mode != protocol.WorkModePlan {
						calls = append(calls, [3]string{"goal", "update_goal", `{"status":"paused"}`})
					}
					return calls
				}
				return nil
			})
			planning := level.mode == protocol.WorkModePlan
			if !strings.Contains(results["brief"], `"title":"Notes"`) || strings.Contains(results["brief"], "permission_denied") {
				t.Errorf("artifact was not written: %.300q", results["brief"])
			}
			if refused := strings.Contains(results["memory"], "planning mode permits read-only tools"); refused != planning {
				t.Errorf("memory under %s returned %.300q", level.name, results["memory"])
			}
			if !planning && (strings.Contains(results["memory"], "permission_denied") || !strings.Contains(results["goal"], "No goal is set")) {
				t.Errorf("own storage was not reached: %.300q, %.300q", results["memory"], results["goal"])
			}
			// The artifact and, outside Plan, the memory transaction record
			// their storage authority; the plain goal handlers record none.
			expected := 2
			if planning {
				expected = 1
			}
			if len(approvals) != expected {
				t.Errorf("recorded %d approvals: %+v", len(approvals), approvals)
			}
			for _, approval := range approvals {
				if approval.Source != protocol.ConversationApprovalSourceStorage || approval.State != protocol.ConversationApprovalStateApproved || approval.CanRemember {
					t.Errorf("storage write was not authorized by its scope: %+v", approval)
				}
			}
		})
	}

	// With the sandbox setting off a command is an ordinary approval the
	// session can remember, and each command reads the setting as it is now.
	// The fixture launches nothing: the calls fail after their approval.
	t.Run("commands follow the sandbox setting", func(t *testing.T) {
		sandbox := func(enabled bool) {
			current := commandFixture(t, node, protocol.Command{Kind: "read_sandbox_settings"}).Data.(protocol.SandboxSettings)
			current.Enabled = enabled
			commandFixture(t, node, protocol.Command{Kind: "save_sandbox_settings", Data: &current})
		}
		sandbox(false)
		defer sandbox(true)
		asked := 0
		_, approvals, _ := turn(t, protocol.WorkModeCode, protocol.PermissionAsk, func(approval protocol.ConversationApproval) string {
			asked++
			if !approval.CanRemember || approval.Prefix == nil || *approval.Prefix != "git status" || approval.Access == nil || *approval.Access != protocol.ProcessAccessUnrestricted {
				t.Errorf("command without a sandbox is not an ordinary approval: %+v", approval)
			}
			return "approve_session"
		}, func(step int, _ map[string]string) [][3]string {
			switch step {
			case 1:
				return [][3]string{{"first", "run_command", `{"command":"git status --short"}`}}
			case 2:
				return [][3]string{{"second", "run_command", `{"command":"git log"}`}}
			case 3:
				// The user turns the sandbox back on while the turn runs.
				sandbox(true)
				return [][3]string{{"third", "run_command", `{"command":"git diff"}`}}
			}
			return nil
		})
		if asked != 1 || len(approvals) != 3 || approvals[1].Source != protocol.ConversationApprovalSourceSessionRule || approvals[1].State != protocol.ConversationApprovalStateApproved {
			t.Fatalf("session rule did not answer the second command: asked %d, %+v", asked, approvals)
		}
		if runtime.GOOS == "darwin" || runtime.GOOS == "linux" {
			if last := approvals[2]; last.Source != protocol.ConversationApprovalSourceSandbox || last.Access == nil || *last.Access != protocol.ProcessAccessWorkspace {
				t.Fatalf("command ignored the sandbox turned on during its turn: %+v", last)
			}
		}
	})

	// A flow step has no approval of its own: a flow that declares a workspace
	// change is refused before the user is asked.
	t.Run("flow with a workspace change", func(t *testing.T) {
		rewrite := operations.Alias("flow-fixture", "rewrite")
		var results map[string]string
		_, approvals, _ := turn(t, protocol.WorkModeCode, protocol.PermissionAsk, func(approval protocol.ConversationApproval) string {
			t.Errorf("flow asked for an approval it cannot use: %+v", approval)
			return "deny"
		}, func(step int, got map[string]string) [][3]string {
			results = got
			switch step {
			case 1:
				return [][3]string{{"discover", "tool_search", `{"query":"select:` + rewrite + `"}`}}
			case 2:
				return [][3]string{{"rewrite", declaredToolName(rewrite), `{}`}}
			}
			return nil
		})
		if len(approvals) != 0 || !strings.Contains(results["rewrite"], "plugin flows cannot run files.write") {
			t.Fatalf("flow result = %q, approvals %+v", results["rewrite"], approvals)
		}
		if _, err := os.Stat(filepath.Join(root, "note.txt")); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("flow changed the workspace: %v", err)
		}
	})

	// A capture is a read. Saving it asks first, is refused while planning
	// and publishes the checked pixels below assets/generated once.
	t.Run("browser capture saving", func(t *testing.T) {
		var picture bytes.Buffer
		if err := png.Encode(&picture, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
			t.Fatal(err)
		}
		controller := protocol.NodeID{7}
		stream, err := node.browsers.subscribe(controller)
		if err != nil {
			t.Fatal(err)
		}
		defer stream.Close()
		go func() {
			for {
				update, err := stream.Next(t.Context())
				if err != nil {
					return
				}
				reply, _ := json.Marshal(map[string]any{"url": "https://example.com/", "path": "/etc/passwd", "base64_image": base64.StdEncoding.EncodeToString(picture.Bytes())})
				raw := json.RawMessage(reply)
				_ = node.browsers.complete(controller, update.Data.(protocol.BrowserCall).ID, protocol.BrowserResult{Ok: &raw})
			}
		}()
		inv := &store.AgentInvocation{Caller: controller}
		inv.Session.ID = protocol.NewSessionID()
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var id string
			if err := db.QueryRowContext(t.Context(), "SELECT id FROM worktrees WHERE project=?1", project.ID.String()).Scan(&id); err != nil {
				return err
			}
			var err error
			inv.Run.Worktree, err = protocol.ParseWorktreeID(id)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		var schema map[string]any
		if err := json.Unmarshal(browserToolSchema, &schema); err != nil {
			t.Fatal(err)
		}
		capture := func(id string, save bool) canonical.ToolRequest {
			arguments, _ := json.Marshal(map[string]any{"action": "screenshot", "arguments": map[string]any{"tab": 1, "format": "png", "save": save}})
			return canonical.ToolRequest{ID: id, Name: "capture", Arguments: arguments}
		}
		tool := node.browserTool(inv, protocol.PluginReference{}, "capture", browserReadOperation, schema)
		if tool.Approval || !tool.ReadOnly {
			t.Fatalf("capture tool is not a read: %+v", tool)
		}
		for _, save := range []bool{false, true} {
			call := capture("capture", save)
			if err := tool.Prepare(t.Context(), call); err != nil {
				t.Fatal(err)
			}
			if tool.Confirm(call) != save {
				t.Fatalf("save=%v confirms %v", save, !save)
			}
			result := tool.Execute(t.Context(), call)
			if result.Failure != nil {
				t.Fatalf("save=%v: %+v", save, result.Failure)
			}
			var facts map[string]any
			if err := json.Unmarshal(result.Value.(map[string]any)["data"].(json.RawMessage), &facts); err != nil {
				t.Fatal(err)
			}
			if facts["mime_type"] != "image/png" || facts["inline_data"] == nil || facts["base64_image"] != nil {
				t.Fatalf("save=%v capture = %v", save, facts)
			}
			saved, _ := facts["path"].(string)
			if !save {
				if saved != "" {
					t.Fatalf("unsaved capture names a path: %q", saved)
				}
				continue
			}
			if matched, _ := path.Match("assets/generated/browser-*.png", saved); !matched {
				t.Fatalf("saved capture path = %q", saved)
			}
			if written, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(saved))); err != nil || !bytes.Equal(written, picture.Bytes()) {
				t.Fatalf("saved capture differs from its pixels: %v", err)
			}
		}
		// A published capture is never replaced.
		id := protocol.NewRequestID()
		input := protocol.CommandUseBrowserPayload{Session: inv.Session.ID, Worktree: inv.Run.Worktree, Action: protocol.BrowserAction{Kind: "screenshot", Data: &protocol.BrowserActionScreenshotPayload{Tab: 1, Scale: protocol.BrowserScaleCss, Format: protocol.BrowserFormatPng, Save: true}}}
		if _, err := node.useBrowser(t.Context(), controller, id, input); err != nil {
			t.Fatal(err)
		}
		if _, err := node.useBrowser(t.Context(), controller, id, input); err == nil {
			t.Fatal("a second capture replaced the saved file")
		}
		inv.Turn.Config.Mode = protocol.WorkModePlan
		var refused *protocol.Fault
		if err := tool.Prepare(t.Context(), capture("planned", true)); !errors.As(err, &refused) || refused.Message != "planning turns cannot save browser captures" {
			t.Fatalf("planning turn prepared a saved capture: %v", err)
		}
		if err := tool.Prepare(t.Context(), capture("planned", false)); err != nil {
			t.Fatalf("planning turn refused a plain capture: %v", err)
		}
	})

	t.Run("plan mode", func(t *testing.T) {
		var results map[string]string
		_, approvals, _ := turn(t, protocol.WorkModePlan, protocol.PermissionFull, func(protocol.ConversationApproval) string {
			t.Fatal("plan mode must not ask")
			return "deny"
		}, func(step int, current map[string]string) [][3]string {
			results = current
			switch step {
			case 1:
				return discover
			case 2:
				return [][3]string{
					{"plugins", "manage_plugins", `{"action":"list"}`},
					{"plugin-remove", "manage_plugins", `{"action":"uninstall","name":"roles-fixture","expected_revision":1}`},
					{"mcp-list", "manage_mcp", `{"action":"list"}`},
					{"mcp-install", "manage_mcp", `{"action":"install","name":"mcp-plan","definition":{"type":"stdio","command":"fixture-mcp","args":[],"env":[]}}`},
					{"skills", "manage_skills", `{"action":"list"}`},
					{"skill-remove", "manage_skills", `{"action":"uninstall","name":"roles-fixture","expected_revision":1}`},
					{"agents", "list_agents", `{}`},
					{"agent", "add_agent", `{"key":"planner","name":"Planner","instructions":"Plan"}`},
					{"keep", roles("drop"), `{"role":"` + protocol.NewRoleID().String() + `","expected_revision":"1"}`},
					{"ocr-install", ocr("install_ocr_model"), `{"id":"` + unknown + `"}`},
				}
			}
			return nil
		})
		expect(t, results, map[string][]string{
			"plugins":       {`"name":"roles-fixture"`},
			"plugin-remove": {"permission_denied", "planning turns cannot change plugins"},
			"mcp-list":      {`"servers":[`, `"name":"github"`, `"standalone":false`},
			"mcp-install":   {"permission_denied", "MCP mutations are unavailable in Plan mode"},
			"skills":        {`{"skills":[]}`},
			"skill-remove":  {"permission_denied", "skill installation changes are unavailable in Plan mode"},
			"agents":        {`"key":"writer"`},
			"agent":         {"permission_denied", "planning mode permits read-only tools"},
			"keep":          {"permission_denied", "planning mode permits read-only tools"},
			"ocr-install":   {"permission_denied", "planning mode permits read-only tools"},
		})
		if len(approvals) != 0 {
			t.Fatalf("plan mode recorded approvals: %+v", approvals)
		}
		installed := commandFixture(t, node, protocol.Command{Kind: "list_plugins"}).Data.([]protocol.PluginSummary)
		if !slices.ContainsFunc(installed, func(entry protocol.PluginSummary) bool { return entry.Name == "roles-fixture" }) ||
			slices.ContainsFunc(installed, func(entry protocol.PluginSummary) bool { return entry.Name == "mcp-plan" }) {
			t.Fatalf("plan mode changed the installed packages: %+v", installed)
		}
	})
}
