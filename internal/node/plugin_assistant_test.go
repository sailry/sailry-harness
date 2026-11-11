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
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginAssistantScript struct {
	fantasy.LanguageModel
	mu     sync.Mutex
	tools  []string
	prompt string
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
	script.mu.Unlock()
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Assistant answered"}) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

// pluginAssistantAcceptance runs one assistant turn through the real Node: the
// declared resource, shared workspace, frozen packages, tools and context.
func pluginAssistantAcceptance(t *testing.T) {
	script := &pluginAssistantScript{}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("assistant acceptance must not launch a process")
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
		"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"assistant-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["files.read"],"tools":[{"name":"read_note","operation":"files.read"},{"name":"list_notes","operation":"files.list"}],"desktop":{"entry":"` + entry + `","resources":["` + entry + `"],"conversations":[{"id":"notes","resource":"plugin","context":"Fixture assistant context","tools":[{"kind":"plugin","name":"read_note"},{"kind":"package","package":"native-approval-fixture","name":"write"},{"kind":"builtin","name":"ask_user"}]},{"id":"project","resource":"workspace","context":"","tools":[]},{"id":"delegating","resource":"plugin","context":"","tools":[{"kind":"builtin","name":"spawn_agent"}]}]}}}}`,
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
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	queued := commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Read my note"},
	}}).Data.(protocol.QueuedTurn)
	if !slices.Equal(queued.Plugins, []protocol.PluginReference{binding.Package, plugins.PackageReference(imported)}) {
		t.Fatalf("assistant turn froze other packages: %+v", queued.Plugins)
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
	slices.Sort(tools)
	allowed := []string{"ask_user", "read_note", "write", "read_tool_output"}
	slices.Sort(allowed)
	if !slices.Equal(tools, allowed) {
		t.Fatalf("assistant request offered %v, declared %v", tools, allowed)
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

	// An agent reference is admitted only when the assistant declares delegation.
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
		if target.ID == session.ID {
			if response.Fault == nil || response.Fault.Code != protocol.ErrorCodePermissionDenied || response.Fault.Message != "plugin assistant does not allow delegation" || runs != 1 {
				t.Fatalf("assistant without delegation admitted an agent reference: %+v, %d turns", response.Fault, runs)
			}
		} else if response.Fault != nil || runs != 1 {
			t.Fatalf("delegating assistant rejected an agent reference: %+v, %d turns", response.Fault, runs)
		}
	}
	// An assistant conversation references only its own package.
	foreign := imported.Summary.Name
	response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{
		Session: second.ID, ExpectedRevision: second.Revision, Message: protocol.ConversationInput{Text: "Use this", References: []protocol.ConversationReference{{
			Target: protocol.ConversationReferenceTarget{Kind: "plugin", Data: &foreign}, Label: foreign,
		}}},
	}}))
	if response.Fault == nil || response.Fault.Code != protocol.ErrorCodeInvalidRequest || response.Fault.Message != "referenced plugin is disabled or unavailable" {
		t.Fatalf("assistant conversation admitted another package's reference: %+v", response.Fault)
	}
}
