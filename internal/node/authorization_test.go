package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommandAuthorizationCoverage(t *testing.T) {
	registered := map[string]bool{}
	for _, kind := range protocol.CommandKinds() {
		registered[kind] = true
		if commandPolicies[kind] == 0 {
			t.Errorf("command %s has no authorization policy", kind)
		}
	}
	for kind := range commandPolicies {
		if !registered[kind] {
			t.Errorf("authorization policy names unregistered command %s", kind)
		}
	}
	if _, ok := commandPolicies["unregistered_command"]; ok {
		t.Fatal("unknown command has a policy")
	}
}

func TestProjectUsersAndSubscriptionRevocation(t *testing.T) {
	node := nodeFixture(t)
	ctx := t.Context()
	owner, other := protocol.NodeID{31}, protocol.NodeID{32}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := store.PairLinkPeer(ctx, db, owner, link.DirectionController); err != nil {
			return err
		}
		return store.PairLinkPeer(ctx, db, other, link.DirectionController)
	}); err != nil {
		t.Fatal(err)
	}
	send := func(caller protocol.NodeID, command protocol.Command) link.Response {
		return dispatchFixture(t, node, caller, protocol.NewRequest(node.ID(), command))
	}
	draft := protocol.DefaultProjectsDraft()
	draft.Name = "Private"
	draft.Source = protocol.ProjectsSource{Kind: "local"}
	draft.Path = t.TempDir()
	created := send(owner, protocol.Command{Kind: "create_project", Data: draft})
	if created.Fault != nil {
		t.Fatal(created.Fault)
	}
	project := created.Output.Data.(protocol.Project)
	snapshot := func(caller protocol.NodeID) protocol.Snapshot {
		t.Helper()
		value := send(caller, protocol.Command{Kind: "snapshot"})
		if value.Fault != nil {
			t.Fatal(value.Fault)
		}
		return value.Output.Data.(protocol.Snapshot)
	}
	if got := snapshot(other); len(got.Projects) != 0 || len(got.Worktrees) != 0 {
		t.Fatalf("private resources leaked: %+v", got.Projects)
	}
	own := snapshot(owner)
	if own.User == nil || len(own.Projects) != 1 || len(own.Worktrees) != 1 {
		t.Fatal("owner snapshot missing resources or identity")
	}
	tree := own.Worktrees[0].ID
	denied := send(other, protocol.Command{Kind: "read_worktree_catalog", Data: protocol.CommandReadWorktreeCatalogPayload{Worktree: tree}})
	if denied.Fault == nil || denied.Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("private raw ID access: %+v", denied)
	}
	if stream, err := node.Subscribe(ctx, other, protocol.Topic{Kind: "files", Data: tree}); err == nil {
		stream.Close()
		t.Fatal("private subscription allowed")
	}
	shared := protocol.ProjectVisibilityShared
	result := send(owner, protocol.Command{Kind: "update_project", Data: protocol.CommandUpdateProjectPayload{Expected: project, Name: project.Name, Path: project.Path, Appearance: project.Appearance, Visibility: &shared}})
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	project = result.Output.Data.(protocol.Project)
	if len(snapshot(other).Projects) != 1 {
		t.Fatal("shared project absent")
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = protocol.NewProviderID(), "fixture"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	createdSession := send(owner, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}})
	if createdSession.Fault != nil {
		t.Fatal(createdSession.Fault)
	}
	session := createdSession.Output.Data.(protocol.Session)
	rename := protocol.NewRequest(node.ID(), protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: session.Revision, Title: "Shared edit"}})
	if result := dispatchFixture(t, node, other, rename); result.Fault != nil {
		t.Fatal(result.Fault)
	}
	stream, err := node.Subscribe(ctx, other, protocol.Topic{Kind: "files", Data: tree})
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	if _, err = stream.Next(ctx); err != nil {
		t.Fatal(err)
	}
	private := protocol.ProjectVisibilityPrivate
	result = send(owner, protocol.Command{Kind: "update_project", Data: protocol.CommandUpdateProjectPayload{Expected: project, Name: project.Name, Path: project.Path, Appearance: project.Appearance, Visibility: &private}})
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	timed, cancel := context.WithTimeout(ctx, time.Second)
	defer cancel()
	if _, err = stream.Next(timed); err == nil {
		t.Fatal("detail observation survived lost access")
	}
	if len(snapshot(other).Projects) != 0 {
		t.Fatal("revoked project remains projected")
	}
	body, err := json.Marshal(rename)
	if err != nil {
		t.Fatal(err)
	}
	inspection := send(other, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: rename.ID, Digest: admission.Digest(body)}})
	if inspection.Fault != nil {
		t.Fatal(inspection.Fault)
	}
	outcome := inspection.Output.Data.(protocol.OutputRequestOutcomePayload).Outcome
	resultBody, err := json.Marshal(outcome)
	if err != nil {
		t.Fatal(err)
	}
	var decoded protocol.RequestOutcome
	if err = json.Unmarshal(resultBody, &decoded); err != nil {
		t.Fatal(err)
	}
	denial := decoded.Data.(*protocol.Result[protocol.Output, protocol.Fault]).Err
	if decoded.Kind != "completed" || denial == nil || denial.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("private result disclosed by inspection: %s", resultBody)
	}
	if replay := dispatchFixture(t, node, other, rename); replay.Fault == nil || replay.Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("private result disclosed by replay: %+v", replay)
	}
	admin := send(other, protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{}})
	if admin.Fault == nil || admin.Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("non-admin host mutation: %+v", admin)
	}
}
