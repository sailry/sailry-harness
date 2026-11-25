package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginAssistantScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	tools   []string
	prompt  string
	pending []fantasy.StreamPart
}

func (script *pluginAssistantScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	prompt, err := json.Marshal(call.Prompt)
	if err != nil {
		return nil, err
	}
	script.mu.Lock()
	script.prompt, script.tools = string(prompt), nil
	for _, tool := range call.Tools {
		script.tools = append(script.tools, tool.GetName())
	}
	pending := script.pending
	script.pending = nil
	script.mu.Unlock()
	return func(yield func(fantasy.StreamPart) bool) {
		if len(pending) > 0 {
			for _, part := range pending {
				if !yield(part) {
					return
				}
			}
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
			return
		}
		if yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Assistant answered"}) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

// pluginAssistantAcceptance runs one assistant turn through the real Node: the
// declared resource, shared workspace, frozen packages, tools and context.
func pluginAssistantAcceptance(t *testing.T) {
	script := &pluginAssistantScript{}
	var launches atomic.Int32
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	assembly := hostassembly.Harness()
	assembly.BundledDefaults = []plugins.DefaultPackage{{Name: "files", Enabled: true}}
	assembly.Modules = append(assembly.Modules, serviceTestModule("ssh"))
	assembly.SSH = func(moduleapi.SSHServiceOptions) moduleapi.SSHService { return &serviceTestSSH{} }
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Assembly: assembly, MCP: fixtureMCPOptions(), Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(_ context.Context, launch process.Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
			if launch.Command != "fixture inherited command" || launch.Access != protocol.ProcessAccessUnrestricted {
				return protocol.ProcessCompletion{}, errors.New("assistant command lost its captured input or access")
			}
			launches.Add(1)
			emit(protocol.ProcessOutputOutputStreamStdout, []byte("Inherited command ran"))
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	imported := installNativeApprovalFixture(t, node)
	source := t.TempDir()
	entry := "dev.sailry.platform/desktop/main.js"
	for name, body := range map[string]string{
		"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"assistant-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["files.read"],"tools":[{"name":"read_note","operation":"files.read"},{"name":"list_notes","operation":"files.list"}],"desktop":{"entry":"` + entry + `","resources":["` + entry + `"],"conversations":[{"id":"notes","resource":"plugin","context":"Fixture assistant context","tools":[{"kind":"plugin","name":"read_note"},{"kind":"package","package":"native-approval-fixture","name":"write"},{"kind":"builtin","name":"ask_user"}]},{"id":"project","resource":"workspace","context":"","tools":[]},{"id":"delegating","resource":"plugin","context":"","tools":[{"kind":"builtin","name":"spawn_agent"}]},{"id":"ssh","resource":"ssh","context":"Fixture SSH context","tools":[]}]}}}}`,
		entry:         `export default function main(){return null;}`,
	} {
		filename := filepath.Join(source, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filename, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	owner, err := node.plugins.manager.Install(t.Context(), source, "", "assistant-fixture")
	if err != nil || len(owner.Issues) != 0 || owner.Extension == nil || owner.Extension.Desktop == nil {
		t.Fatalf("assistant fixture is invalid: %+v, %v", owner.Issues, err)
	}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		owner, err = store.InstallPlugin(t.Context(), db, owner, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
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
	binding := protocol.PluginConversationBinding{Package: plugins.PackageReference(owner), ID: "notes"}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	config.Assistant = &binding
	create := func(config protocol.SessionConfig, project *protocol.ProjectID) (protocol.Session, *protocol.Fault) {
		response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{
			Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: project, Config: &config},
		}))
		if response.Fault != nil {
			return protocol.Session{}, response.Fault
		}
		return response.Output.Data.(protocol.Session), nil
	}
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Assistant acceptance", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	stale, workspace, unknown := config, config, config
	stale.Assistant = &protocol.PluginConversationBinding{Package: binding.Package, ID: "notes"}
	stale.Assistant.Package.Digest = strings.Repeat("0", 64)
	workspace.Assistant = &protocol.PluginConversationBinding{Package: binding.Package, ID: "project"}
	unknown.Assistant = &protocol.PluginConversationBinding{Package: binding.Package, ID: "missing"}
	for _, rejected := range []struct {
		name    string
		config  protocol.SessionConfig
		project *protocol.ProjectID
		code    protocol.ErrorCode
	}{
		{"changed package version", stale, nil, protocol.ErrorCodeRevisionConflict},
		{"plugin assistant in a project", config, &project.ID, protocol.ErrorCodePermissionDenied},
		{"workspace assistant without a project", workspace, nil, protocol.ErrorCodePermissionDenied},
		{"undeclared assistant", unknown, nil, protocol.ErrorCodeNotConfigured},
	} {
		if _, failure := create(rejected.config, rejected.project); failure == nil || failure.Code != rejected.code {
			t.Fatalf("%s was admitted: %+v", rejected.name, failure)
		}
	}
	session, failure := create(config, nil)
	second, repeated := create(config, nil)
	if failure != nil || repeated != nil || session.Worktree != second.Worktree || session.Project != nil {
		t.Fatalf("assistant conversations did not share their plugin workspace: %+v %+v", failure, repeated)
	}
	expected, err := core.AssistantWorkspace(node.profile.Ownership.Path, binding)
	if err != nil {
		t.Fatal(err)
	}
	if root, err := node.workspaceRoot(t.Context(), session.Worktree, false); err != nil || root != expected {
		t.Fatalf("assistant workspace preparation = %q, %v", root, err)
	}
	if err := core.PrivateDirectory(expected); err != nil {
		t.Fatalf("prepared assistant workspace is not private: %v", err)
	}
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	queued := commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Read my note"},
	}}).Data.(protocol.QueuedTurn)
	if !slices.Contains(queued.Plugins, binding.Package) || !slices.Contains(queued.Plugins, plugins.PackageReference(imported)) || len(queued.Plugins) <= 2 {
		t.Fatalf("assistant turn lost its ordinary captured catalog: %+v", queued.Plugins)
	}
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
				t.Fatalf("assistant conversation failed: %+v", run)
			}
		}
		if !completed {
			if _, err := subscription.Next(t.Context()); err != nil {
				t.Fatal(err)
			}
		}
	}
	script.mu.Lock()
	tools, prompt := slices.Clone(script.tools), script.prompt
	script.mu.Unlock()
	for _, name := range []string{"ask_user", "set_session_title", "read_note", "list_notes", "write", "read_tool_output", "read_file", "run_command", "tool_search", "load_skill"} {
		if !slices.Contains(tools, name) {
			t.Fatalf("assistant omitted preferred or ordinary tool %s: %v", name, tools)
		}
	}
	if slices.Contains(tools, "read_session") || !strings.Contains(prompt, "read_session") {
		t.Fatal("assistant bypassed ordinary deferred tool discovery")
	}
	if !strings.Contains(prompt, "Fixture assistant context") {
		t.Fatal("assistant request omitted its declared context")
	}
	var path string
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		return db.QueryRowContext(t.Context(), "SELECT path FROM worktrees WHERE id=?1", session.Worktree.String()).Scan(&path)
	}); err != nil {
		t.Fatal(err)
	}
	if info, err := os.Stat(expected); path != expected || err != nil || !info.IsDir() {
		t.Fatalf("assistant workspace is not its private profile directory: %s, %v", path, err)
	}

	// A bound SSH assistant can use ordinary local tools with normal approvals.
	ssh := commandFixture(t, node, protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{
		Profile:    protocol.SshProfile{ID: protocol.NewSshID(), Name: "Fixture", Host: "fixture.invalid", Port: 22, Username: "fixture", Authentication: protocol.SshAuthenticationPassword},
		Credential: &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("fixture-password")}},
	}}).Data.(protocol.SshProfile)
	sshConfig := config
	sshConfig.Assistant = &protocol.PluginConversationBinding{Package: binding.Package, ID: "ssh"}
	sshConfig.Resource = &protocol.ConnectionResource{Kind: "ssh", Data: ssh.ID}
	sshSession, failure := create(sshConfig, nil)
	if failure != nil {
		t.Fatal(failure)
	}
	sshRoot, err := node.workspaceRoot(t.Context(), sshSession.Worktree, false)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sshRoot, "local.txt"), []byte("Inherited local file"), 0o600); err != nil {
		t.Fatal(err)
	}
	script.mu.Lock()
	script.pending = []fantasy.StreamPart{
		{Type: fantasy.StreamPartTypeToolCall, ID: "local-read", ToolCallName: "read_file", ToolCallInput: `{"path":"local.txt"}`},
		{Type: fantasy.StreamPartTypeToolCall, ID: "local-command", ToolCallName: "run_command", ToolCallInput: `{"command":"fixture inherited command","access":"unrestricted"}`},
	}
	script.mu.Unlock()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: sshSession.ID, ExpectedRevision: sshSession.Revision, Message: protocol.ConversationInput{Text: "Read local.txt and run the local fixture command"}}})
	approved := false
	for completed := false; !completed; {
		var history protocol.ConversationHistory
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(t.Context(), db, sshSession.ID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State == protocol.ConversationApprovalStatePending {
				if launches.Load() != 0 {
					t.Fatal("the inherited command ran before ordinary approval")
				}
				commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: sshSession.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}})
				approved = true
			}
		}
		for _, run := range history.Page.Runs {
			if run.Status == protocol.ConversationStatusCompleted {
				completed = true
			} else if run.Status == protocol.ConversationStatusFailed || run.Status == protocol.ConversationStatusInterrupted || run.Status == protocol.ConversationStatusCancelled {
				t.Fatalf("inherited assistant tools failed: %+v", run)
			}
		}
		if !completed {
			if _, err := subscription.Next(t.Context()); err != nil {
				t.Fatal(err)
			}
		}
	}
	script.mu.Lock()
	inherited := script.prompt
	script.mu.Unlock()
	if !approved || launches.Load() != 1 || !strings.Contains(inherited, "Inherited local file") || !strings.Contains(inherited, "Inherited command ran") {
		t.Fatalf("SSH assistant did not execute ordinary tools with normal authority: approved=%t launches=%d prompt=%s", approved, launches.Load(), inherited)
	}
	// Agent references use the ordinary roster even without a preferred delegation tool.
	delegating := config
	delegating.Assistant = &protocol.PluginConversationBinding{Package: binding.Package, ID: "delegating"}
	delegated, failure := create(delegating, nil)
	if failure != nil || len(delegated.Roles.Profiles) == 0 {
		t.Fatalf("delegating assistant conversation has no roster: %+v, %+v", delegated.Roles, failure)
	}
	for _, target := range []protocol.Session{session, delegated} {
		var current protocol.Session
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var err error
			current, err = store.ReadSession(t.Context(), db, target.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		role := current.Roles.Profiles[0]
		response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: current.ID, ExpectedRevision: current.Revision, Message: protocol.ConversationInput{Text: "Delegate this", References: []protocol.ConversationReference{{
				Target: protocol.ConversationReferenceTarget{Kind: "agent", Data: &protocol.RoleReference{ID: role.ID, Revision: role.Revision}}, Label: role.Name,
			}}},
		}}))
		var runs int
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			return db.QueryRowContext(t.Context(), "SELECT count(*) FROM turns WHERE session=?1", current.ID.String()).Scan(&runs)
		}); err != nil {
			t.Fatal(err)
		}
		wantRuns := 1
		if target.ID == session.ID {
			wantRuns = 2
		}
		if response.Fault != nil || runs != wantRuns {
			t.Fatalf("assistant rejected an ordinary agent reference: %+v, %d turns", response.Fault, runs)
		}
	}
	// Enabled package references are shared with ordinary conversations.
	foreign := imported.Summary.Name
	response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{
		Session: second.ID, ExpectedRevision: second.Revision, Message: protocol.ConversationInput{Text: "Use this", References: []protocol.ConversationReference{{
			Target: protocol.ConversationReferenceTarget{Kind: "plugin", Data: &foreign}, Label: foreign,
		}}},
	}}))
	if response.Fault != nil {
		t.Fatalf("assistant conversation rejected an ordinary package reference: %+v", response.Fault)
	}
}
