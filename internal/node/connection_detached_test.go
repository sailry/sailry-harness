package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strings"
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

type detachedConnectionScript struct {
	agentToolScript
	declared [][]string
}

func (script *detachedConnectionScript) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	names := make([]string, 0, len(call.Tools))
	for _, tool := range call.Tools {
		names = append(names, tool.GetName())
	}
	script.mu.Lock()
	script.declared = append(script.declared, names)
	script.mu.Unlock()
	return script.agentToolScript.Stream(ctx, call)
}

func testDetachedConnectionTools(t *testing.T) {
	script := &detachedConnectionScript{}
	peer := newSSHLogPeer(t)
	peer.allowPassword.Store(true)
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, MCP: fixtureMCPOptions(),
		Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
	})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), time.Minute)
	t.Cleanup(func() {
		cancel()
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	config := connectionModelConfig(t, node)
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data.(protocol.Session)
	if session.Project != nil || session.Config.Resource != nil || session.Config.Assistant != nil {
		t.Fatalf("ordinary conversation has a binding: %+v", session)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Other project", Path: t.TempDir(), Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	global := &protocol.ConnectionSharing{Kind: "global"}
	projectOnly := &protocol.ConnectionSharing{Kind: "projects", Data: []protocol.ProjectID{project.ID}}

	const pluginName, pluginTool = "detached-fixture", "detached_roles"
	source := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"` + pluginName + `","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["roles.read"],"tools":[{"name":"` + pluginTool + `","contexts":["plugin"],"operation":"roles.list"}]}}}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	fixture, err := node.plugins.manager.Install(ctx, source, "", pluginName)
	if err != nil || len(fixture.Issues) != 0 {
		t.Fatalf("plugin fixture: %v, %v", fixture.Issues, err)
	}
	if fixture.Extension == nil || len(fixture.Extension.Tools) != 1 || fixture.Extension.Tools[0].Initial {
		t.Fatal("roles.list fixture must omit the manifest's initial opt-in")
	}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		fixture, err = store.InstallPlugin(ctx, db, fixture, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	owners := map[string]protocol.PluginReference{}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		for _, name := range []string{"databases", "ssh"} {
			info, err := store.ReadPlugin(ctx, db, name)
			if err != nil {
				return err
			}
			if info != nil {
				owners[name] = plugins.PackageReference(*info)
			}
		}
		return nil
	}); err != nil || len(owners) != 2 {
		t.Fatalf("connection packages: %v, %v", owners, err)
	}
	controller := func(owner string, command protocol.Command) protocol.Output {
		t.Helper()
		scope := protocol.DefaultPluginContext()
		scope.Package = owners[owner]
		request := protocol.NewRequest(node.ID(), command)
		request.Plugin = &scope
		response := dispatchFixture(t, node, node.ID(), request)
		if response.Fault != nil {
			t.Fatal(response.Fault)
		}
		return response.Output
	}
	path := filepath.Join(t.TempDir(), "fixture.db")
	saveDatabase := func(name string, sharing *protocol.ConnectionSharing) protocol.DatabaseProfile {
		return controller("databases", protocol.Command{Kind: "save_database", Data: protocol.CommandSaveDatabasePayload{Profile: protocol.DatabaseProfile{
			ID: protocol.NewDatabaseID(), Name: name, Sharing: sharing,
			Connection: protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{Path: path}},
		}}}).Data.(protocol.DatabaseProfile)
	}
	saveSSH := func(name string, sharing *protocol.ConnectionSharing) protocol.SshProfile {
		return controller("ssh", protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{
			Profile: protocol.SshProfile{ID: protocol.NewSshID(), Name: name, Host: name + ".fixture.invalid", Port: 22, Username: "fixture",
				Authentication: protocol.SshAuthenticationAgent, Sharing: sharing},
			Credential: &protocol.SshCredential{Kind: "agent"},
		}}).Data.(protocol.SshProfile)
	}
	database, privateDB, projectDB := saveDatabase("global", global), saveDatabase("private", nil), saveDatabase("project", projectOnly)
	ssh := controller("ssh", protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{
		Profile: protocol.SshProfile{ID: protocol.NewSshID(), Name: "global", Host: peer.address.IP.String(), Port: uint16(peer.address.Port), Username: sshLogUser,
			Authentication: protocol.SshAuthenticationPassword, Sharing: global},
		Credential: &protocol.SshCredential{Kind: "password", Data: protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(sshLogPassword)}},
	}}).Data.(protocol.SshProfile)
	privateSSH, projectSSH := saveSSH("private", nil), saveSSH("project", projectOnly)
	if database.Sharing == nil || database.Sharing.Kind != "global" || ssh.Sharing == nil || ssh.Sharing.Kind != "global" {
		t.Fatal("Node-wide controller creation failed to retain global sharing")
	}
	commandFixture(t, node, protocol.Command{Kind: "query_database", Data: protocol.CommandQueryDatabasePayload{
		Profile: database.ID, ExpectedRevision: database.Revision, Sql: "CREATE TABLE fixture (value TEXT)", RowLimit: 1, TimeoutMs: 1000,
	}})
	scratchPath := filepath.Join(filepath.Dir(path), "scratch.db")
	fixtureData, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(scratchPath, fixtureData, 0o600); err != nil {
		t.Fatal(err)
	}
	sshRef := func(id protocol.SshID) protocol.ConversationReference {
		return protocol.ConversationReference{Label: "SSH", Target: protocol.ConversationReferenceTarget{Kind: "ssh", Data: &id}}
	}
	dbRef := func(id protocol.DatabaseID) protocol.ConversationReference {
		return protocol.ConversationReference{Label: "Database", Target: protocol.ConversationReferenceTarget{Kind: "database", Data: &protocol.ConversationReferenceTargetDatabasePayload{Connection: id}}}
	}
	for _, ref := range []protocol.ConversationReference{dbRef(privateDB.ID), dbRef(projectDB.ID), sshRef(privateSSH.ID), sshRef(projectSSH.ID)} {
		response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Use this connection", References: []protocol.ConversationReference{ref}},
		}}))
		message := "referenced database is not available to this conversation"
		if ref.Target.Kind == "ssh" {
			message = "referenced SSH connection is not available to this conversation"
		}
		if response.Fault == nil || response.Fault.Code != protocol.ErrorCodePermissionDenied || response.Fault.Message != message {
			t.Fatalf("unshared %s reference returned %+v", ref.Target.Kind, response)
		}
	}

	marshal := func(value any) string {
		body, err := json.Marshal(value)
		if err != nil {
			t.Error(err)
		}
		return string(body)
	}
	call := func(id, name string, args map[string]any) [3]string {
		return [3]string{id, "call_tool", marshal(map[string]any{"name": name, "arguments": args})}
	}
	search := func(names ...string) [3]string {
		return [3]string{"discover", "tool_search", marshal(map[string]any{"query": "select:" + strings.Join(names, ","), "limit": 8})}
	}
	query := func(id string, target protocol.DatabaseID) [3]string {
		return call(id, "database_query", map[string]any{"connection": target, "sql": "SELECT 'detached-query-fixture'"})
	}
	sshRun := func(id string) [3]string {
		return call(id, "ssh_run", map[string]any{"connection": ssh.ID, "command": sshLogPlatform, "timeout_ms": 5000})
	}
	lists := func(dbCall, sshCall string) [][3]string {
		return [][3]string{call(dbCall, "database_connections", map[string]any{}), call(sshCall, "ssh_connections", map[string]any{})}
	}
	deferred := []string{"database_connections", "ssh_connections", "database_query", "database_save_connection", "ssh_save_connection", "ssh_run"}
	started, lateSaved := make(chan struct{}), make(chan struct{})
	usedPrivate, sharesRevoked := make(chan struct{}), make(chan struct{})
	pause := func(reached chan struct{}, resume <-chan struct{}) {
		close(reached)
		select {
		case <-resume:
		case <-ctx.Done():
		}
	}
	awaitPause := func(reached <-chan struct{}) {
		t.Helper()
		select {
		case <-reached:
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	var lateDB protocol.DatabaseProfile
	var createdDB struct {
		Data protocol.DatabaseProfile `json:"data"`
	}
	var createdSSH struct {
		Data protocol.SshProfile `json:"data"`
	}
	script.reset(func(step int, results map[string]string) [][3]string {
		switch step {
		case 1:
			pause(started, lateSaved)
			return [][3]string{{"roles", pluginTool, `{}`}, search(deferred...)}
		case 2:
			return append(lists("db-list", "ssh-list"),
				query("global-query", database.ID), query("private-query", privateDB.ID), query("project-query", projectDB.ID), query("late-query", lateDB.ID))
		case 3:
			return [][3]string{call("edit-db", "database_save_connection", map[string]any{"connection": database.ID, "expected_revision": database.Revision, "name": "Edited database"})}
		case 4:
			return [][3]string{call("edit-ssh", "ssh_save_connection", map[string]any{"connection": ssh.ID, "expected_revision": ssh.Revision, "name": "Edited SSH"})}
		case 5:
			return [][3]string{call("create-db", "database_save_connection", map[string]any{"engine": "sqlite", "path": scratchPath, "name": "Created by the model"})}
		case 6:
			return [][3]string{call("create-ssh", "ssh_save_connection", map[string]any{
				"name": "Created by the model", "host": "created.fixture.invalid", "port": 22, "username": "fixture", "authentication": "agent",
			})}
		case 7:
			if err := json.Unmarshal([]byte(results["create-db"]), &createdDB); err != nil {
				t.Errorf("model creation result did not carry a database profile: %v", err)
			}
			if err := json.Unmarshal([]byte(results["create-ssh"]), &createdSSH); err != nil {
				t.Errorf("model creation result did not carry an SSH profile: %v", err)
			}
			return [][3]string{query("same-turn-query", createdDB.Data.ID), sshRun("global-ssh-run")}
		case 8:
			pause(usedPrivate, sharesRevoked)
			return append(lists("revoked-db-list", "revoked-ssh-list"), query("revoked-query", database.ID), sshRun("revoked-ssh-run"))
		default:
			return nil
		}
	})
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	submit := func(revision uint64, refs ...protocol.ConversationReference) protocol.TurnID {
		return commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: revision, Message: protocol.ConversationInput{Text: "Inspect saved connections", References: refs},
		}}).Data.(protocol.QueuedTurn).ID
	}
	waitFor := func(turn protocol.TurnID, ready func(protocol.ConversationHistory) bool) {
		t.Helper()
		for {
			var history protocol.ConversationHistory
			if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(ctx, db, session.ID, nil, 100)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, run := range history.Page.Runs {
				if run.Turn == turn && slices.Contains([]protocol.ConversationStatus{protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled}, run.Status) {
					t.Fatalf("connection turn failed: %+v", run)
				}
			}
			if ready(history) {
				return
			}
			if _, err := subscription.Next(ctx); err != nil {
				t.Fatal(err)
			}
		}
	}
	waitCompleted := func(turn protocol.TurnID) {
		waitFor(turn, func(history protocol.ConversationHistory) bool {
			return slices.ContainsFunc(history.Page.Runs, func(run protocol.ConversationRun) bool {
				return run.Turn == turn && run.Status == protocol.ConversationStatusCompleted
			})
		})
	}

	turn := submit(session.Revision, dbRef(database.ID), sshRef(ssh.ID))
	awaitPause(started)
	lateDB = saveDatabase("late", global)
	lateSSH := saveSSH("late", global)
	close(lateSaved)
	approved := map[protocol.ApprovalID]bool{}
	hostKeyApproved := false
	waitFor(turn, func(history protocol.ConversationHistory) bool {
		for _, approval := range history.Page.Approvals {
			if approval.Turn != turn || approval.State != protocol.ConversationApprovalStatePending || approved[approval.ID] {
				continue
			}
			if trust := approval.Ssh; trust != nil {
				want := protocol.SshTrust{Profile: ssh.ID, ExpectedRevision: ssh.Revision + 1, Host: ssh.Host, Port: ssh.Port, Key: peer.key}
				if hostKeyApproved || *trust != want || approval.Source != protocol.ConversationApprovalSourceUser || approval.CanRemember || peer.passwordSeen.Load() {
					t.Fatalf("unexpected SSH host-key approval: %+v", approval)
				}
				hostKeyApproved = true
			}
			commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{
				Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"},
			}})
			approved[approval.ID] = true
		}
		return len(approved) == 5
	})
	awaitPause(usedPrivate)
	if !hostKeyApproved || !peer.passwordSeen.Load() {
		t.Fatal("ordinary ssh_run did not authenticate after host-key approval")
	}
	if createdDB.Data.ID == (protocol.DatabaseID{}) || createdDB.Data.Sharing != nil || createdDB.Data.Revision != 1 ||
		createdSSH.Data.ID == (protocol.SshID{}) || createdSSH.Data.Sharing != nil || createdSSH.Data.Revision != 1 {
		t.Fatalf("ordinary tool creation should save private profiles: %+v, %+v", createdDB.Data, createdSSH.Data)
	}
	var editedDB protocol.DatabaseProfile
	var editedSSH protocol.SshProfile
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		current, err := store.ReadDatabase(ctx, db, database.ID)
		if err != nil {
			return err
		}
		if current != nil {
			editedDB = *current
		}
		editedSSH, _, err = store.ReadSsh(ctx, db, ssh.ID, ssh.Revision+2)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if editedDB.Name != "Edited database" || editedDB.Revision != database.Revision+1 || editedDB.Sharing == nil || editedDB.Sharing.Kind != "global" ||
		editedSSH.Name != "Edited SSH" || editedSSH.Sharing == nil || editedSSH.Sharing.Kind != "global" {
		t.Fatalf("approved ordinary edits did not retain global sharing: %+v, %+v", editedDB, editedSSH)
	}
	if editedSSH.HostKey == nil || *editedSSH.HostKey != peer.key {
		t.Fatalf("SSH host key did not match the approved fingerprint: %+v", editedSSH.HostKey)
	}
	approvedSave := testDetachedConnectionAuthority(t, node, session, turn, owners, editedDB, editedSSH, createdDB.Data)
	staged := admitDetachedConnectionEffects(t, node, session, turn, owners, editedDB, editedSSH)
	assertDetachedEffectFault(t, node, staged[0], protocol.ErrorCodeRevisionConflict)
	editedDB.Sharing, editedSSH.Sharing = nil, projectOnly
	controller("databases", protocol.Command{Kind: "save_database", Data: protocol.CommandSaveDatabasePayload{Profile: editedDB, ExpectedRevision: editedDB.Revision}})
	controller("ssh", protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{Profile: editedSSH, ExpectedRevision: editedSSH.Revision}})
	for _, request := range staged[1:] {
		assertDetachedEffectFault(t, node, request, protocol.ErrorCodePermissionDenied)
	}
	revoked := dispatchFixture(t, node, node.ID(), approvedSave)
	if revoked.Fault == nil || revoked.Fault.Code != protocol.ErrorCodePermissionDenied || revoked.Fault.Message != "connection is not available to this captured resource" {
		t.Fatalf("an approved save could still edit a revoked connection: %+v", revoked)
	}
	close(sharesRevoked)
	waitCompleted(turn)
	if len(script.offered) != 9 || !slices.Contains(script.offered[0], "tool_search") || !slices.Contains(script.offered[0], "call_tool") {
		t.Fatalf("unexpected model tool requests: %v", script.offered)
	}
	if !slices.Contains(script.declared[0], pluginTool) || !slices.Equal(script.declared[0], script.declared[1]) {
		t.Fatalf("initial tool or declaration set changed after discovery: %v", script.declared[:2])
	}
	for _, name := range deferred {
		if slices.Contains(script.offered[0], name) || slices.Contains(script.declared[1], name) || !slices.Contains(script.offered[1], name) {
			t.Errorf("%s was not loaded as a deferred tool: %v", name, script.offered[:2])
		}
	}
	responses := detachedToolResponses(t, node, session.ID, turn)
	assertConnectionList(t, responses["db-list"], "database_profiles", database.ID.String())
	assertConnectionList(t, responses["ssh-list"], "ssh_profiles", ssh.ID.String())
	assertConnectionList(t, responses["revoked-db-list"], "database_profiles", createdDB.Data.ID.String())
	assertConnectionList(t, responses["revoked-ssh-list"], "ssh_profiles", createdSSH.Data.ID.String())
	for _, id := range []string{"global-query", "same-turn-query"} {
		if value := responses[id]; value == nil || value.Status != canonical.ToolSuccess || !strings.Contains(string(value.Result.Value), "detached-query-fixture") {
			t.Errorf("%s did not reach the SQLite fixture: %+v", id, value)
		}
	}
	if value := responses["roles"]; value == nil || value.Status != canonical.ToolSuccess || value.Name != operations.Alias(pluginName, pluginTool) {
		t.Errorf("the initially declared plugin tool did not execute: %+v", value)
	}
	if value := responses["global-ssh-run"]; value == nil || value.Status != canonical.ToolSuccess || value.Name != operations.Alias(owners["ssh"].Name, "ssh_run") {
		t.Fatalf("deferred ssh_run did not execute through call_tool: %+v", value)
	}
	assertDetachedSSHResult(t, script.results["global-ssh-run"])
	for _, id := range []string{"private-query", "project-query", "late-query"} {
		assertDetachedToolFault(t, id, responses[id], protocol.ErrorCodeInvalidRequest)
	}
	assertDetachedToolFault(t, "revoked-query", responses["revoked-query"], protocol.ErrorCodePermissionDenied)
	assertDetachedToolFault(t, "revoked-ssh-run", responses["revoked-ssh-run"], protocol.ErrorCodePermissionDenied)

	commandFixture(t, node, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{
		Name: pluginName, ExpectedRevision: fixture.Summary.Revision, Enabled: false,
	}})
	var current protocol.Session
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		current, err = store.ReadSession(ctx, db, session.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	nextStarted, nextChecked := make(chan struct{}), make(chan struct{})
	script.reset(func(step int, _ map[string]string) [][3]string {
		switch step {
		case 1:
			pause(nextStarted, nextChecked)
			return [][3]string{search(pluginTool, "database_query", "ssh_connections")}
		case 2:
			return [][3]string{query("other-turn-query", createdDB.Data.ID), query("newly-captured-query", lateDB.ID),
				call("newly-captured-ssh", "ssh_connections", map[string]any{}), call("disabled", pluginTool, map[string]any{})}
		default:
			return nil
		}
	})
	turn = submit(current.Revision)
	awaitPause(nextStarted)
	scope := detachedConnectionScope(current, turn, owners["databases"])
	approvedSave.Plugin = &scope
	wrongTurn := dispatchFixture(t, node, node.ID(), approvedSave)
	if wrongTurn.Fault == nil || wrongTurn.Fault.Code != protocol.ErrorCodePermissionDenied || wrongTurn.Fault.Message != "connection save has no claimed approval" {
		t.Fatalf("another turn reused a save approval: %+v", wrongTurn)
	}
	close(nextChecked)
	waitCompleted(turn)
	responses = detachedToolResponses(t, node, session.ID, turn)
	if len(script.offered) != 3 || slices.Contains(script.offered[0], pluginTool) || slices.Contains(script.offered[1], pluginTool) {
		t.Fatalf("disabled plugin appeared in discovery: %v", script.offered)
	}
	assertConnectionList(t, responses["newly-captured-ssh"], "ssh_profiles", lateSSH.ID.String())
	assertDetachedToolFault(t, "other-turn-query", responses["other-turn-query"], protocol.ErrorCodeInvalidRequest)
	if value := responses["disabled"]; value == nil || value.Status != canonical.ToolErrorStatus {
		t.Errorf("call_tool ran a disabled plugin: %+v", value)
	}
	if value := responses["newly-captured-query"]; value == nil || value.Status != canonical.ToolSuccess || !strings.Contains(string(value.Result.Value), "detached-query-fixture") {
		t.Errorf("next turn did not capture the later global database: %+v", value)
	}
}

func assertDetachedSSHResult(t *testing.T, text string) {
	t.Helper()
	var result struct {
		Kind string              `json:"kind"`
		Data protocol.SshOutcome `json:"data"`
	}
	if err := json.Unmarshal([]byte(text), &result); err != nil || result.Kind != "ssh_outcome" || result.Data.Kind != "completed" {
		t.Fatalf("model received an unexpected SSH outcome: %q, %v", text, err)
	}
	completed, valid := dispatchPayload[protocol.SshOutcomeCompletedPayload](result.Data.Data)
	if !valid || completed.Stdout != "Linux\nx86_64\n" || completed.Stderr != "" || completed.ExitCode != 0 || completed.Truncated {
		t.Fatalf("loopback SSH result did not reach the model: %q", text)
	}
}

func assertDetachedToolFault(t *testing.T, id string, response *canonical.ToolResponse, code protocol.ErrorCode) {
	t.Helper()
	var failure struct {
		Data protocol.Fault `json:"data"`
	}
	if response == nil || response.Status != canonical.ToolErrorStatus {
		t.Fatalf("%s returned %+v, want %s", id, response, code)
	}
	if err := json.Unmarshal(response.Result.Value, &failure); err != nil || failure.Data.Code != code {
		t.Errorf("%s returned %s, want %s: %v", id, response.Result.Value, code, err)
	}
}

func detachedToolResponses(t *testing.T, node *Node, session protocol.SessionID, turn protocol.TurnID) map[string]*canonical.ToolResponse {
	t.Helper()
	results := map[string]*canonical.ToolResponse{}
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		state, err := store.LoadAgentState(t.Context(), db, session)
		if err != nil {
			return err
		}
		for _, record := range state.Records {
			if message := record.Message(); record.Turn == turn && message != nil {
				for _, block := range message.Content {
					if response, ok := block.Data.(*canonical.ToolResponse); ok {
						results[response.ID] = response
					}
				}
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	return results
}

func assertConnectionList(t *testing.T, response *canonical.ToolResponse, kind string, ids ...string) {
	t.Helper()
	var output struct {
		Kind string                `json:"kind"`
		Data []struct{ ID string } `json:"data"`
	}
	if response == nil || response.Status != canonical.ToolSuccess {
		t.Fatalf("%s list failed: %+v", kind, response)
	}
	if err := json.Unmarshal(response.Result.Value, &output); err != nil || output.Kind != kind || output.Data == nil || len(output.Data) != len(ids) {
		t.Fatalf("%s list returned %s, want %v: %v", kind, response.Result.Value, ids, err)
	}
	for i, id := range ids {
		if output.Data[i].ID != id {
			t.Errorf("%s list returned %s, want %v", kind, response.Result.Value, ids)
		}
	}
}
