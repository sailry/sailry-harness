package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// connectionHeldModel keeps its turn running until the test releases it, so
// connection tools can be bound and called against a live captured turn.
type connectionHeldModel struct {
	fantasy.LanguageModel
	once    sync.Once
	started chan struct{}
	release chan struct{}
}

func (held *connectionHeldModel) Stream(ctx context.Context, _ fantasy.Call) (fantasy.StreamResponse, error) {
	held.once.Do(func() { close(held.started) })
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-held.release:
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

var connectionOperations = []protocol.ToolOperation{
	protocol.ToolOperationListDatabaseConnections, protocol.ToolOperationSaveDatabaseConnection, protocol.ToolOperationBrowseDatabase,
	protocol.ToolOperationQueryDatabase, protocol.ToolOperationExecuteDatabase,
	protocol.ToolOperationListSshConnections, protocol.ToolOperationSaveSshConnection, protocol.ToolOperationRunSsh, protocol.ToolOperationTransferSsh,
}

func testConnectionTools(t *testing.T) {
	t.Run("classification", func(t *testing.T) {
		readOnly := []protocol.ToolOperation{protocol.ToolOperationListDatabaseConnections, protocol.ToolOperationListSshConnections,
			protocol.ToolOperationBrowseDatabase, protocol.ToolOperationQueryDatabase}
		for _, operation := range connectionOperations {
			if !operations.ConnectionOperation(operation) {
				t.Errorf("%s is not a connection operation", operation)
			}
			read, concurrent := operations.Policy(operation)
			if read != slices.Contains(readOnly, operation) || concurrent {
				t.Errorf("%s policy = read-only %v, concurrent %v", operation, read, concurrent)
			}
		}
		if operations.ConnectionOperation(protocol.ToolOperationWriteFile) {
			t.Error("a file operation was classified as a connection operation")
		}
	})

	t.Run("contexts", func(t *testing.T) {
		database := operations.Declaration{Contexts: []string{"workspace", "database"}}
		for _, entry := range []struct {
			name        string
			declaration operations.Declaration
			project     bool
			bound       string
			available   bool
		}{
			{"unrestricted in a database conversation", operations.Declaration{}, false, "database", true},
			{"database tool in a project", database, true, "", true},
			{"database tool in a database conversation", database, false, "database", true},
			{"database tool in an SSH conversation", database, false, "ssh", false},
			{"database tool in a plain conversation", database, false, "", false},
			{"workspace tool in a database conversation", operations.Declaration{Contexts: []string{"workspace"}}, false, "database", false},
			{"plugin tool in a plain conversation", operations.Declaration{Contexts: []string{"plugin"}}, false, "", true},
			{"plugin tool in an SSH conversation", operations.Declaration{Contexts: []string{"plugin"}}, false, "ssh", false},
			{"bound resource inside a project", database, true, "database", false},
		} {
			if got := entry.declaration.Available(entry.project, entry.bound); got != entry.available {
				t.Errorf("%s: available = %v", entry.name, got)
			}
		}
		if !database.Available(true) || database.Available(false) {
			t.Error("the unbound form no longer follows the project rule")
		}
	})

	t.Run("arguments", func(t *testing.T) {
		id := protocol.NewDatabaseID()
		for _, entry := range []struct {
			name      string
			operation protocol.ToolOperation
			arguments string
			message   string
		}{
			{"list rejects fields", protocol.ToolOperationListSshConnections, `{"connection":"x"}`, "invalid connection operation: "},
			{"query requires a limit", protocol.ToolOperationQueryDatabase, `{"connection":"` + id.String() + `","sql":"select 1","timeout_ms":1}`, "invalid connection operation: "},
			{"query rejects unknown fields", protocol.ToolOperationQueryDatabase, `{"connection":"` + id.String() + `","sql":"select 1","row_limit":1,"timeout_ms":1,"password":"x"}`, "invalid connection operation: "},
			{"target must be an identifier", protocol.ToolOperationRunSsh, `{"connection":"production","command":"uptime","timeout_ms":1}`, "invalid connection operation: "},
			{"transfer direction", protocol.ToolOperationTransferSsh, `{"connection":"` + id.String() + `","path":"a","remote_path":"/a","direction":"sideways","timeout_ms":1}`, "invalid connection operation: "},
			{"save rejects null edits", protocol.ToolOperationSaveDatabaseConnection, `{"name":null}`, "connection edit fields cannot be null"},
			{"save rejects unknown credentials", protocol.ToolOperationSaveSshConnection, `{"credential":{"kind":"token"}}`, "invalid SSH credential"},
			{"save rejects scalar credentials", protocol.ToolOperationSaveSshConnection, `{"credential":"secret"}`, "invalid SSH credential"},
			{"other operations", protocol.ToolOperationWriteFile, `{}`, "native operation is unavailable"},
		} {
			_, err := operations.ConnectionCall(entry.operation, json.RawMessage(entry.arguments))
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || !strings.HasPrefix(fault.Message, entry.message) {
				t.Errorf("%s: error = %v", entry.name, err)
			}
		}
		input, err := operations.ConnectionCall(protocol.ToolOperationSaveDatabaseConnection, json.RawMessage(`{"connection":null,"expected_revision":null,"name":"Local"}`))
		if err != nil || string(input.Save) != `{"connection":null,"expected_revision":null,"name":"Local"}` || input.Target != nil {
			t.Errorf("save arguments = %#v %v", input, err)
		}
		input, err = operations.ConnectionCall(protocol.ToolOperationBrowseDatabase, json.RawMessage(`{"connection":"`+id.String()+`","database":null}`))
		if err != nil || input.Target == nil || input.Target.Kind != "database" || input.Target.Data != any(id) || input.Database != nil {
			t.Errorf("catalog arguments = %#v %v", input, err)
		}
	})

	held := &connectionHeldModel{started: make(chan struct{}), release: make(chan struct{})}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	node, err := StartWithOptions(context.Background(), filepath.Join(t.TempDir(), "profile"), Options{MCP: fixtureMCPOptions(), Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return held, nil }})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		close(held.release)
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	ctx := t.Context()
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: "Fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance()}}).Data.(protocol.Project)
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Revision = 1
	provider.Enabled = true
	provider.Name = "Fixture"
	provider.Api = protocol.ConversationModelApiChatCompletions
	provider.Authentication = protocol.AuthenticationApiKey
	provider.Endpoint = "http://fixture.invalid/v1"
	provider.DefaultModel = "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID = "fixture"
	selected.Context = 32000
	selected.Output = 8000
	selected.Tools = true
	provider.Models = []protocol.ConversationModel{selected}
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}

	const databasePassword, sshPassword = "fixture-database-password", "fixture-ssh-password"
	network := func(host string) protocol.DatabaseConnection {
		return protocol.DatabaseConnection{Kind: "network", Data: &protocol.DatabaseConnectionNetworkPayload{
			Engine: protocol.DatabaseEnginePostgres, Host: host, Port: 5432, Database: "app", Username: "reader", Tls: protocol.DatabaseTlsRequire}}
	}
	saveDatabase := func(name, host string, sharing *protocol.ConnectionSharing) protocol.DatabaseProfile {
		password := protocol.NewSecret(databasePassword)
		output := commandFixture(t, node, protocol.Command{Kind: "save_database", Data: protocol.CommandSaveDatabasePayload{
			Profile: protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: name, Connection: network(host), ReadOnly: true, Sharing: sharing}, Password: &password}})
		profile, valid := dispatchPayload[protocol.DatabaseProfile](output.Data)
		if !valid {
			t.Fatalf("saved database = %#v", output)
		}
		return profile
	}
	shared := saveDatabase("Shared", "shared.fixture.test", &protocol.ConnectionSharing{Kind: "global"})
	private := saveDatabase("Private", "private.fixture.test", nil)
	output := commandFixture(t, node, protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{
		Profile: protocol.SshProfile{ID: protocol.NewSshID(), Name: "Bastion", Host: "bastion.fixture.test", Port: 22, Username: "deploy",
			Authentication: protocol.SshAuthenticationPassword, Sharing: &protocol.ConnectionSharing{Kind: "projects", Data: &[]protocol.ProjectID{project.ID}}},
		Credential: &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(sshPassword)}}}})
	bastion, valid := dispatchPayload[protocol.SshProfile](output.Data)
	if !valid {
		t.Fatalf("saved SSH connection = %#v", output)
	}

	config := protocol.DefaultSessionConfig()
	config.Provider = provider.ID
	config.Model = "fixture"
	config.Permission = protocol.PermissionAsk
	config.Mode = protocol.WorkModeCode
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Inspect the database"}}})
	select {
	case <-held.started:
	case <-time.After(time.Minute):
		t.Fatal("the held turn never reached its model")
	}

	inv := &store.AgentInvocation{Caller: node.ID()}
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		history, err := store.ReadConversation(ctx, db, session.ID, nil, 100)
		if err != nil {
			return err
		}
		if len(history.Page.Runs) != 1 {
			return errors.New("the held turn has no run")
		}
		if inv.Run, err = store.ReadAgentRun(ctx, db, history.Page.Runs[0].Turn); err != nil {
			return err
		}
		if inv.Session, err = store.ReadSession(ctx, db, session.ID); err != nil {
			return err
		}
		var plugins []byte
		if err = db.QueryRowContext(ctx, "SELECT plugins FROM turns WHERE id=?1", inv.Run.Turn.String()).Scan(&plugins); err != nil {
			return err
		}
		return json.Unmarshal(plugins, &inv.Turn.Plugins)
	})
	if err != nil {
		t.Fatal(err)
	}
	if inv.Run.Status != protocol.ConversationStatusRunning {
		t.Fatalf("held turn status = %s", inv.Run.Status)
	}
	inv.Worktree = protocol.Worktree{ID: inv.Session.Worktree, Project: inv.Session.Project, Path: root, Main: true}
	inv.Turn.ID, inv.Turn.Session, inv.Turn.Config = inv.Run.Turn, inv.Session.ID, inv.Session.Config
	catalog, err := operations.CaptureCatalog(ctx, node.profile.Ownership.Path, inv.Turn.Plugins)
	if err != nil {
		t.Fatal(err)
	}
	declarations := map[protocol.ToolOperation]operations.Declaration{}
	for _, declaration := range catalog.Tools {
		if operations.ConnectionOperation(declaration.Operation) {
			declarations[declaration.Operation] = declaration
		}
	}
	if len(declarations) != len(connectionOperations) {
		t.Fatalf("captured connection declarations = %d", len(declarations))
	}
	captured := []protocol.ConnectionResource{{Kind: "database", Data: shared.ID}, {Kind: "ssh", Data: bastion.ID}}
	bind := func(t *testing.T, inv *store.AgentInvocation) map[protocol.ToolOperation]engine.Tool {
		t.Helper()
		tools, err := node.connectionTools(ctx, inv, catalog)
		if err != nil {
			t.Fatal(err)
		}
		bound := map[protocol.ToolOperation]engine.Tool{}
		for _, tool := range tools {
			if tool.Binding == nil || tool.Binding.Operation == nil || tool.Definition.Name != declarations[*tool.Binding.Operation].Name {
				t.Fatalf("connection tool binding = %#v", tool.Binding)
			}
			bound[*tool.Binding.Operation] = tool
		}
		return bound
	}

	t.Run("visibility", func(t *testing.T) {
		databaseKind := func(operation protocol.ToolOperation) bool { return strings.HasPrefix(string(operation), "databases.") }
		conversation := func(project bool, resource *protocol.ConnectionResource) *store.AgentInvocation {
			value := *inv
			value.Turn.Config.Resource = resource
			if !project {
				value.Worktree.Project = nil
			}
			return &value
		}
		for _, entry := range []struct {
			name    string
			inv     *store.AgentInvocation
			visible func(protocol.ToolOperation) bool
		}{
			{"project", inv, func(protocol.ToolOperation) bool { return true }},
			{"database", conversation(false, &protocol.ConnectionResource{Kind: "database", Data: shared.ID}), databaseKind},
			{"ssh", conversation(false, &protocol.ConnectionResource{Kind: "ssh", Data: bastion.ID}), func(operation protocol.ToolOperation) bool { return !databaseKind(operation) }},
			{"plain", conversation(false, nil), func(protocol.ToolOperation) bool { return false }},
		} {
			bound := bind(t, entry.inv)
			for _, operation := range connectionOperations {
				tool, exists := bound[operation]
				if exists != entry.visible(operation) {
					t.Errorf("%s conversation: %s visible = %v", entry.name, operation, exists)
				}
				if exists && (tool.ReadOnly != declarations[operation].ReadOnly || tool.Approval == tool.ReadOnly || tool.Concurrent) {
					t.Errorf("%s conversation: %s read-only %v, approval %v", entry.name, operation, tool.ReadOnly, tool.Approval)
				}
			}
		}
		withoutPackages := *inv
		withoutPackages.Turn.Plugins = nil
		if bound := bind(t, &withoutPackages); len(bound) != 0 {
			t.Errorf("a turn without the packages bound %d connection tools", len(bound))
		}
	})

	t.Run("commands", func(t *testing.T) {
		database := "reports"
		for _, entry := range []struct {
			operation protocol.ToolOperation
			arguments map[string]any
			command   protocol.Command
		}{
			{protocol.ToolOperationListDatabaseConnections, map[string]any{}, protocol.Command{Kind: "list_databases"}},
			{protocol.ToolOperationListSshConnections, map[string]any{}, protocol.Command{Kind: "list_ssh"}},
			{protocol.ToolOperationBrowseDatabase, map[string]any{"connection": shared.ID, "database": database},
				protocol.Command{Kind: "browse_database", Data: &protocol.CommandBrowseDatabasePayload{Profile: shared.ID, ExpectedRevision: shared.Revision, Database: &database}}},
			{protocol.ToolOperationQueryDatabase, map[string]any{"connection": shared.ID, "sql": "select 1", "row_limit": 1000, "timeout_ms": 30000},
				protocol.Command{Kind: "query_database", Data: &protocol.CommandQueryDatabasePayload{ReadOnly: true, Profile: shared.ID, ExpectedRevision: shared.Revision, Sql: "select 1", RowLimit: 1000, TimeoutMs: 30000}}},
			{protocol.ToolOperationExecuteDatabase, map[string]any{"connection": shared.ID, "database": database, "sql": "delete from logs", "row_limit": 10, "timeout_ms": 5000},
				protocol.Command{Kind: "query_database", Data: &protocol.CommandQueryDatabasePayload{Database: &database, Profile: shared.ID, ExpectedRevision: shared.Revision, Sql: "delete from logs", RowLimit: 10, TimeoutMs: 5000}}},
			{protocol.ToolOperationRunSsh, map[string]any{"connection": bastion.ID, "command": "uptime", "timeout_ms": 120000},
				protocol.Command{Kind: "run_ssh", Data: &protocol.CommandRunSshPayload{Profile: bastion.ID, ExpectedRevision: bastion.Revision, Command: "uptime", TimeoutMs: 120000}}},
			{protocol.ToolOperationTransferSsh, map[string]any{"connection": bastion.ID, "path": "build.tar", "remote_path": "/tmp/build.tar", "direction": "upload", "timeout_ms": 120000},
				protocol.Command{Kind: "transfer_ssh", Data: &protocol.CommandTransferSshPayload{Profile: bastion.ID, ExpectedRevision: bastion.Revision, TimeoutMs: 120000,
					Transfer: protocol.SshTransfer{Worktree: inv.Run.Worktree, Path: "build.tar", RemotePath: "/tmp/build.tar", Direction: protocol.SshDirectionUpload}}}},
		} {
			arguments, err := json.Marshal(entry.arguments)
			if err != nil {
				t.Fatal(err)
			}
			command, err := node.connectionCommand(ctx, inv, declarations[entry.operation], arguments, captured)
			if err != nil || !reflect.DeepEqual(command, entry.command) {
				t.Errorf("%s command = %#v %v", entry.operation, command, err)
			}
		}
	})

	t.Run("sharing", func(t *testing.T) {
		for _, entry := range []struct {
			name      string
			operation protocol.ToolOperation
			arguments map[string]any
			captured  []protocol.ConnectionResource
		}{
			{"unshared database", protocol.ToolOperationQueryDatabase, map[string]any{"connection": private.ID, "sql": "select 1", "row_limit": 1, "timeout_ms": 1000}, captured},
			{"unshared database named as captured", protocol.ToolOperationQueryDatabase, map[string]any{"connection": private.ID, "sql": "select 1", "row_limit": 1, "timeout_ms": 1000},
				append(slices.Clone(captured), protocol.ConnectionResource{Kind: "database", Data: private.ID})},
			{"shared database outside the capture", protocol.ToolOperationBrowseDatabase, map[string]any{"connection": shared.ID}, nil},
			{"unknown SSH connection", protocol.ToolOperationRunSsh, map[string]any{"connection": protocol.NewSshID(), "command": "uptime", "timeout_ms": 1000}, captured},
		} {
			arguments, err := json.Marshal(entry.arguments)
			if err != nil {
				t.Fatal(err)
			}
			command, err := node.connectionCommand(ctx, inv, declarations[entry.operation], arguments, entry.captured)
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodePermissionDenied {
				t.Errorf("%s: command = %#v, error = %v", entry.name, command, err)
			}
		}
		tool := bind(t, inv)[protocol.ToolOperationQueryDatabase]
		arguments, _ := json.Marshal(map[string]any{"connection": private.ID, "sql": "select 1", "row_limit": 1, "timeout_ms": 1000})
		err := tool.Prepare(ctx, canonical.ToolRequest{ID: "refused", Name: tool.Definition.Name, Arguments: arguments})
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodePermissionDenied {
			t.Errorf("prepared an unshared connection: %v", err)
		}
		if result := tool.Execute(ctx, canonical.ToolRequest{ID: "refused", Name: tool.Definition.Name, Arguments: arguments}); result.Failure == nil {
			t.Error("executed a call that was never prepared")
		}
	})

	t.Run("results", func(t *testing.T) {
		encode := func(value any) string {
			t.Helper()
			body, err := json.Marshal(value)
			if err != nil {
				t.Fatal(err)
			}
			if text := string(body); strings.Contains(text, databasePassword) || strings.Contains(text, sshPassword) || strings.Contains(text, `"password":`) {
				t.Fatalf("model-visible result carries a secret: %s", text)
			}
			return string(body)
		}
		bound := bind(t, inv)
		for _, entry := range []struct {
			operation protocol.ToolOperation
			kind      string
			present   []string
			absent    []string
		}{
			{protocol.ToolOperationListDatabaseConnections, "database_profiles", []string{shared.ID.String(), `"credentials_required":false`}, []string{private.ID.String()}},
			{protocol.ToolOperationListSshConnections, "ssh_profiles", []string{bastion.ID.String()}, []string{"credentials_required"}},
		} {
			tool := bound[entry.operation]
			call := canonical.ToolRequest{ID: string(entry.operation), Name: tool.Definition.Name, Arguments: json.RawMessage(`{}`)}
			if err := tool.Prepare(ctx, call); err != nil {
				t.Fatalf("%s prepare: %v", entry.operation, err)
			}
			result := tool.Execute(ctx, call)
			if result.Failure != nil {
				t.Fatalf("%s failed: %v", entry.operation, result.Failure)
			}
			text := encode(result.Value)
			if !strings.Contains(text, `"kind":"`+entry.kind+`"`) {
				t.Errorf("%s result = %s", entry.operation, text)
			}
			for _, value := range entry.present {
				if !strings.Contains(text, value) {
					t.Errorf("%s result omits %s: %s", entry.operation, value, text)
				}
			}
			for _, value := range entry.absent {
				if strings.Contains(text, value) {
					t.Errorf("%s result exposes %s: %s", entry.operation, value, text)
				}
			}
		}

		owner := declarations[protocol.ToolOperationListDatabaseConnections].Owner
		value, err := node.connectionResult(ctx, inv, owner, protocol.Output{Kind: "database_profiles", Data: []protocol.DatabaseProfile{private, shared}}, captured)
		if text := encode(value); err != nil || strings.Contains(text, private.ID.String()) || !strings.Contains(text, shared.ID.String()) {
			t.Errorf("filtered list = %s %v", text, err)
		}
		value, err = node.connectionResult(ctx, inv, owner, protocol.Output{Kind: "database_profile", Data: shared}, captured)
		if text := encode(value); err != nil || !strings.Contains(text, `"credentials_required":false`) || !strings.Contains(text, `"kind":"database_profile"`) {
			t.Errorf("saved profile = %s %v", text, err)
		}
		_, err = node.connectionResult(ctx, inv, owner, protocol.Output{Kind: "database_profile", Data: private}, captured)
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodePermissionDenied {
			t.Errorf("unreachable saved profile: %v", err)
		}
		outcome := protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: "connected"}}
		if value, err = node.connectionResult(ctx, inv, owner, outcome, captured); err != nil || !reflect.DeepEqual(value, outcome) {
			t.Errorf("effect outcome = %#v %v", value, err)
		}

		input, err := node.connectionTurnInput(ctx, inv, []protocol.ToolOperation{protocol.ToolOperationQueryDatabase, protocol.ToolOperationWriteFile})
		text := encode(input)
		if err != nil || !strings.Contains(text, `"engine":"postgres"`) || !strings.Contains(text, `"database":"app"`) || !strings.Contains(text, shared.ID.String()) ||
			strings.Contains(text, private.ID.String()) || strings.Contains(text, bastion.ID.String()) || strings.Contains(text, "shared.fixture.test") {
			t.Errorf("turn input = %s %v", text, err)
		}
	})
}
