package node

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// testSSHPrimary exercises the real Node admission, sharing and save paths;
// network effects are translated and authorized without contacting a server.
func testSSHPrimary(t *testing.T, node *Node, ordinary protocol.Session, primary protocol.SshProfile, declarations map[protocol.ToolOperation]operations.Declaration) {
	t.Helper()
	ctx := t.Context()
	const secret = "private-ssh-primary-regression"
	other := commandFixture(t, node, protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{
		Profile:    protocol.SshProfile{ID: protocol.NewSshID(), Name: "Other private host", Host: "other.fixture.test", Port: 22, Username: "deploy", Authentication: protocol.SshAuthenticationPassword},
		Credential: &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(secret)}},
	}}).Data.(protocol.SshProfile)
	config := ordinary.Config
	config.Resource = &protocol.ConnectionResource{Kind: "ssh", Data: primary.ID}
	owner := declarations[protocol.ToolOperationSaveSshConnection].Owner
	config.Assistant = &protocol.PluginConversationBinding{Package: owner, ID: "ssh"}
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data.(protocol.Session)
	controller := protocol.NodeID{93}
	admitted := dispatchFixture(t, node, controller, protocol.NewRequest(node.ID(), protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Manage another SSH connection"}}}))
	if admitted.Fault != nil {
		t.Fatal(admitted.Fault)
	}
	turn := admitted.Output.Data.(protocol.QueuedTurn)
	inv := &store.AgentInvocation{Caller: controller, Session: session, Turn: turn}
	deadline := time.Now().Add(20 * time.Second)
	for {
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			inv.Run, err = store.ReadAgentRun(ctx, db, turn.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if inv.Run.Status == protocol.ConversationStatusRunning {
			break
		}
		if inv.Run.Status != protocol.ConversationStatusQueued || time.Now().After(deadline) {
			t.Fatalf("SSH assistant did not start: %+v", inv.Run)
		}
		time.Sleep(10 * time.Millisecond)
	}
	inv.Worktree.ID = session.Worktree
	var captured []protocol.ConnectionResource
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		ordinaryCatalog, err := store.ReadConnectionCatalog(ctx, db, ordinary.ID)
		if err != nil {
			return err
		}
		for _, profile := range ordinaryCatalog.Ssh {
			if profile.ID == other.ID {
				t.Error("the SSH assistant widened the ordinary project's sharing")
			}
		}
		catalog, err := store.ReadConnectionCatalog(ctx, db, session.ID)
		captured = catalog.Resources()
		return err
	}); err != nil {
		t.Fatal(err)
	}
	input, err := node.connectionTurnInput(ctx, inv, []protocol.ToolOperation{protocol.ToolOperationRunSsh})
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(input)
	if err != nil || !strings.Contains(string(encoded), primary.ID.String()) || !strings.Contains(string(encoded), other.ID.String()) || strings.Contains(string(encoded), secret) {
		t.Fatalf("SSH initializer omitted a private connection or exposed credentials: %s, %v", encoded, err)
	}
	scope := protocol.PluginContext{Package: owner, Turn: &turn.ID, Session: &session.ID, Worktree: &session.Worktree, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
	list := protocol.NewRequest(node.ID(), protocol.Command{Kind: "list_ssh"})
	list.Plugin = &scope
	listed := dispatchFixture(t, node, node.ID(), list)
	if listed.Fault != nil {
		t.Fatal(listed.Fault)
	}
	encoded, err = json.Marshal(listed.Output)
	if err != nil || !strings.Contains(string(encoded), other.ID.String()) || strings.Contains(string(encoded), secret) {
		t.Fatalf("SSH list lost private profiles or exposed credentials: %s, %v", encoded, err)
	}
	translate := func(operation protocol.ToolOperation, args map[string]any) protocol.Command {
		t.Helper()
		body, err := json.Marshal(args)
		if err != nil {
			t.Fatal(err)
		}
		command, err := node.connectionCommand(ctx, inv, declarations[operation], body, captured)
		if err != nil {
			t.Fatal(err)
		}
		return command
	}
	run := translate(protocol.ToolOperationRunSsh, map[string]any{"connection": other.ID, "command": "uptime", "timeout_ms": 1000})
	transfer := translate(protocol.ToolOperationTransferSsh, map[string]any{"connection": other.ID, "path": "local.txt", "remote_path": "/tmp/remote.txt", "direction": "upload", "timeout_ms": 1000})
	for _, command := range []protocol.Command{run, transfer} {
		request := protocol.NewRequest(node.ID(), command)
		request.Plugin = &scope
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			_, _, err := store.PluginConnectionAction(ctx, db, request)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	save := func(command protocol.Command) (protocol.Output, *protocol.Fault) {
		t.Helper()
		response := connectionSaveFixture(t, node, scope, declarations[protocol.ToolOperationSaveSshConnection], command)
		return response.Output, response.Fault
	}
	edit := translate(protocol.ToolOperationSaveSshConnection, map[string]any{"connection": other.ID, "expected_revision": other.Revision, "name": "Renamed private host"})
	unapproved := protocol.NewRequest(node.ID(), edit)
	unapproved.Plugin = &scope
	if _, err := node.Dispatch(ctx, node.ID(), unapproved); !pluginMessageFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("unapproved SSH edit was admitted: %v", err)
	}
	updated, fault := save(edit)
	if fault != nil {
		t.Fatal(fault)
	}
	other = updated.Data.(protocol.SshProfile)
	if other.Name != "Renamed private host" || other.Sharing != nil || other.HostKey != nil {
		t.Fatalf("private SSH edit changed its scope or trust: %+v", other)
	}
	if _, fault := save(edit); fault == nil || fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("stale SSH edit returned %+v", fault)
	}
	create := translate(protocol.ToolOperationSaveSshConnection, map[string]any{"name": "New private host", "host": "new.fixture.test", "port": 22, "username": "deploy", "authentication": "password", "credential": protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("new-ssh-save-proof-secret")}}})
	created, fault := save(create)
	if fault != nil {
		t.Fatal(fault)
	}
	added := created.Data.(protocol.SshProfile)
	if added.ID == primary.ID || added.Sharing != nil || added.HostKey != nil {
		t.Fatalf("new SSH profile changed primary scope or trusted a key: %+v", added)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		created, err := store.ConnectionCreated(ctx, db, turn.ID, owner, protocol.ConnectionResource{Kind: "ssh", Data: added.ID})
		if err == nil && !created {
			t.Error("a remote controller's turn lost its execution Node's SSH save proof")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	command := translate(protocol.ToolOperationRunSsh, map[string]any{"connection": added.ID, "command": "uname -a", "timeout_ms": 1000})
	if command.Data.(*protocol.CommandRunSshPayload).Profile != added.ID {
		t.Fatal("the new connection was not usable in its creating turn")
	}
	trust := protocol.NewRequest(node.ID(), protocol.Command{Kind: "trust_ssh", Data: protocol.CommandTrustSshPayload{Profile: added.ID, ExpectedRevision: added.Revision, Key: protocol.SshHostKey{Algorithm: "ssh-ed25519", Fingerprint: "SHA256:unapproved"}}})
	trust.Plugin = &scope
	if _, err := node.Dispatch(ctx, node.ID(), trust); !pluginMessageFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("SSH key was trusted without its fingerprint approval: %v", err)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		current, err := store.ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		resource, err := json.Marshal(current.Config.Resource)
		if err != nil {
			return err
		}
		primaryResource, err := json.Marshal(session.Config.Resource)
		if err != nil {
			return err
		}
		if current.Worktree != session.Worktree || string(resource) != string(primaryResource) || !reflect.DeepEqual(current.Config.Assistant, session.Config.Assistant) || current.Config.Permission != session.Config.Permission || current.Config.Mode != session.Config.Mode {
			t.Error("SSH operations changed the primary conversation association")
		}
		_, credential, err := store.ReadSsh(ctx, db, other.ID, other.Revision)
		if err == nil {
			password, valid := dispatchPayload[protocol.SshCredentialPasswordPayload](credential.Data)
			if credential.Kind != "password" || !valid || password.Password.Expose() != secret {
				t.Error("an edit without credentials replaced the saved credential")
			}
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

// connectionSaveFixture uses an approval already claimed by the agent loop,
// as the native tool does. Preflight must stay read-only; its proof belongs to
// the committed save and must disappear when the save fails.
func connectionSaveFixture(t *testing.T, node *Node, scope protocol.PluginContext, declaration operations.Declaration, command protocol.Command) link.Response {
	t.Helper()
	ctx := t.Context()
	request := protocol.NewRequest(node.ID(), command)
	request.Plugin = &scope
	approval := protocol.DefaultConversationApproval()
	approval.ID, approval.Session, approval.Turn = protocol.NewApprovalID(), *scope.Session, *scope.Turn
	approval.State, approval.Source = protocol.ConversationApprovalStateApproved, protocol.ConversationApprovalSourceUser
	binding := store.ApprovalBinding{Owner: scope.Package, Tool: declaration.Name, Operation: &declaration.Operation}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		body, err := json.Marshal(approval)
		if err != nil {
			return err
		}
		bound, err := json.Marshal(binding)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,?3,?4,?5,?6,1)", approval.ID.String(), approval.Turn.String(), request.ID.String(), body, bound, request.ID.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := node.validatePluginRequest(ctx, node.ID(), request); err != nil {
		t.Fatalf("approved save failed read-only preflight: %v", err)
	}
	prepared := func() string {
		t.Helper()
		var body string
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(ctx, "SELECT prepared FROM agent_approvals WHERE id=?1", approval.ID.String()).Scan(&body)
		}); err != nil {
			t.Fatal(err)
		}
		return body
	}
	if body := prepared(); body != "null" {
		t.Fatalf("preflight wrote a save proof: %s", body)
	}
	response := dispatchFixture(t, node, node.ID(), request)
	if body := prepared(); response.Fault != nil {
		if body != "null" {
			t.Fatalf("failed save retained its proof: %s", body)
		}
	} else {
		var proof struct {
			Command protocol.Command `json:"command"`
			Digest  string           `json:"digest"`
		}
		if err := json.Unmarshal([]byte(body), &proof); err != nil || proof.Command.Kind != command.Kind || proof.Digest == "" {
			t.Fatalf("committed save lost its proof: %s, %v", body, err)
		}
		encoded, err := json.Marshal(command)
		if err != nil || proof.Digest != fmt.Sprintf("%x", blake3.Sum256(encoded)) {
			t.Fatalf("save proof did not bind the original credential-bearing command: %v", err)
		}
		switch proof.Command.Kind {
		case "save_ssh":
			input, valid := dispatchPayload[protocol.CommandSaveSshPayload](proof.Command.Data)
			if !valid || input.Credential != nil {
				t.Fatal("SSH save proof retained credentials")
			}
		case "save_database":
			input, valid := dispatchPayload[protocol.CommandSaveDatabasePayload](proof.Command.Data)
			if !valid || input.Password != nil {
				t.Fatal("database save proof retained a password")
			}
		}
		replayed := dispatchFixture(t, node, node.ID(), request)
		first, err := json.Marshal(response)
		if err != nil {
			t.Fatal(err)
		}
		again, err := json.Marshal(replayed)
		if err != nil || string(first) != string(again) || body != prepared() {
			t.Fatalf("save replay changed its result or proof: %s, %s, %v", first, again, err)
		}
	}
	return response
}

func testDatabaseSaveProof(t *testing.T, node *Node, inv *store.AgentInvocation, declarations map[protocol.ToolOperation]operations.Declaration) {
	t.Helper()
	ctx := t.Context()
	declaration := declarations[protocol.ToolOperationSaveDatabaseConnection]
	scope := protocol.PluginContext{Package: declaration.Owner, Turn: &inv.Run.Turn, Session: &inv.Session.ID, Worktree: &inv.Session.Worktree, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
	password := protocol.NewSecret("database-save-proof-secret")
	profile := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: "", ReadOnly: true, Connection: protocol.DatabaseConnection{Kind: "network", Data: &protocol.DatabaseConnectionNetworkPayload{
		Engine: protocol.DatabaseEnginePostgres, Host: "proof.fixture.test", Port: 5432, Username: "reader", Database: "app", Tls: protocol.DatabaseTlsRequire,
	}}}
	command := protocol.Command{Kind: "save_database", Data: &protocol.CommandSaveDatabasePayload{Profile: profile, Password: &password}}
	failed := connectionSaveFixture(t, node, scope, declaration, command)
	if failed.Fault == nil || failed.Fault.Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("invalid database save returned %+v", failed)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		stored, err := store.ReadDatabase(ctx, db, profile.ID)
		if err == nil && stored != nil {
			t.Error("failed database save retained a profile")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	profile.Name = "Created in this turn"
	command.Data = &protocol.CommandSaveDatabasePayload{Profile: profile, Password: &password}
	result := connectionSaveFixture(t, node, scope, declaration, command)
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	saved, valid := dispatchPayload[protocol.DatabaseProfile](result.Output.Data)
	if !valid || saved.ID != profile.ID || saved.Revision != 1 {
		t.Fatalf("database save did not commit once: %+v", result)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		created, err := store.ConnectionCreated(ctx, db, inv.Run.Turn, declaration.Owner, protocol.ConnectionResource{Kind: "database", Data: saved.ID})
		if err == nil && !created {
			t.Error("the completed database save is unavailable in its creating turn")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	args, err := json.Marshal(map[string]any{"connection": saved.ID, "sql": "select 1", "row_limit": 1, "timeout_ms": 1000})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := node.connectionCommand(ctx, inv, declarations[protocol.ToolOperationQueryDatabase], args, nil); err != nil {
		t.Fatalf("new database requires another turn to be captured: %v", err)
	}
}
