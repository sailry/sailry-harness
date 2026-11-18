package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const forwardedKey = "forwarded-key-7f3a"

func forwardingProvider(t *testing.T, node *Node, endpoint string) protocol.ConversationProvider {
	t.Helper()
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Forwarded", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, endpoint
	provider.Enabled, provider.DefaultModel = true, "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output = "fixture", 200000, 8192
	provider.Models = []protocol.ConversationModel{selected}
	secret := protocol.NewSecret(forwardedKey)
	commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	for _, saved := range commandFixture(t, node, protocol.Command{Kind: "list_providers"}).Data.([]protocol.ConversationProvider) {
		if saved.ID == provider.ID {
			return saved
		}
	}
	t.Fatal("saved provider is not listed")
	return provider
}

func nodeText(t *testing.T, node *Node) string {
	t.Helper()
	var text bytes.Buffer
	snapshot, err := json.Marshal(snapshotFixture(t, node))
	if err != nil {
		t.Fatal(err)
	}
	text.Write(snapshot)
	for _, kind := range []string{"list_providers", "list_credentials"} {
		body, err := json.Marshal(commandFixture(t, node, protocol.Command{Kind: kind}))
		if err != nil {
			t.Fatal(err)
		}
		text.Write(body)
	}
	err = node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
		for _, query := range []string{"SELECT body FROM requests", "SELECT coalesce(result,'') FROM requests", "SELECT body FROM events"} {
			rows, err := db.QueryContext(context.Background(), query)
			if err != nil {
				return err
			}
			for rows.Next() {
				var body []byte
				if err := rows.Scan(&body); err != nil {
					rows.Close()
					return err
				}
				text.Write(body)
			}
			if err := rows.Close(); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return text.String()
}

func storedProvider(t *testing.T, node *Node, id protocol.ProviderID) (string, bool) {
	t.Helper()
	var body []byte
	err := node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
		return db.QueryRowContext(context.Background(), "SELECT body FROM providers WHERE id=?1", id.String()).Scan(&body)
	})
	if errors.Is(err, sql.ErrNoRows) {
		return "", false
	}
	if err != nil {
		t.Fatal(err)
	}
	return string(body), true
}

func forwardingControllerFixture(t *testing.T, target *Node, controller protocol.NodeID) context.Context {
	t.Helper()
	var user protocol.User
	if err := target.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := store.PairPeerIdentity(t.Context(), db, target.ID(), controller, link.DirectionController, true); err != nil {
			return err
		}
		var err error
		user, err = store.CallerUser(t.Context(), db, target.ID(), controller)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return store.WithUser(t.Context(), user)
}

func TestSessionCreationUsesExecutionNodeProvider(t *testing.T) {
	t.Parallel()
	a, b := nodeFixture(t), nodeFixture(t)
	owner := forwardingControllerFixture(t, b, a.ID())
	trustedCallerFixture(t, a, b.ID())
	provider := forwardingProvider(t, a, "https://controller.invalid/v1")

	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = provider.ID, provider.DefaultModel, provider.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	remoteCreate := protocol.NewRequest(a.ID(), protocol.Command{Kind: "create_session_at", Data: &protocol.CommandCreateSessionAtPayload{
		Target: b.ID(), Config: &config, ProviderRevision: provider.Revision,
	}})
	result := dispatchFixture(t, a, a.ID(), remoteCreate)
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeNotConfigured || result.Fault.Message != "select a model channel configured on the execution Node" {
		t.Fatalf("controller model was borrowed for a new remote session: %v", result.Fault)
	}
	if body, exists := storedProvider(t, b, provider.ID); exists {
		t.Fatalf("controller provider was materialized on the execution Node: %s", body)
	}

	// The paired user can save an independent replica under the same channel ID.
	local := provider
	local.Owner, local.Source, local.Consent = nil, nil, nil
	local.Revision, local.Shared = 0, false
	local.Name, local.Endpoint, local.Credential = "Execution channel", "https://execution.invalid/v1", nil
	key := protocol.NewSecret("execution-local-key")
	savedResponse := dispatchFixture(t, b, a.ID(), protocol.NewRequest(b.ID(), protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{
		Provider: local, ExpectedRevision: 0, Secret: &key,
	}}))
	if savedResponse.Fault != nil {
		t.Fatal(savedResponse.Fault)
	}
	saved, ok := dispatchPayload[protocol.ConversationProvider](savedResponse.Output.Data)
	if !ok || saved.Credential == nil || saved.Credential.Node != b.ID() || saved.ID != provider.ID {
		t.Fatalf("execution Node channel is not independent: %+v", savedResponse.Output)
	}
	for name, current := range map[string]*Node{"controller": a, "execution": b} {
		if text := nodeText(t, current); strings.Contains(text, forwardedKey) || strings.Contains(text, key.Expose()) {
			t.Fatalf("%s exposed a provider secret in requests, events or public projections", name)
		}
	}
	config.Provider, config.Model, config.Credential = saved.ID, saved.DefaultModel, saved.Credential
	created := dispatchFixture(t, b, a.ID(), protocol.NewRequest(b.ID(), protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &config}}))
	if created.Fault != nil {
		t.Fatal(created.Fault)
	}
	session, ok := dispatchPayload[protocol.Session](created.Output.Data)
	if !ok || session.Config.Credential == nil || session.Config.Credential.Node != b.ID() {
		t.Fatal("execution Node did not capture its local credential")
	}
	if err := b.profile.Store.Node.Read(owner, func(db *sql.Conn) error {
		output, err := store.ReadProviderCommand(owner, db, b.ID(), protocol.Command{Kind: "read_provider_key", Data: &protocol.CommandReadProviderKeyPayload{Provider: saved.ID, ExpectedRevision: saved.Revision}})
		if err != nil {
			return err
		}
		secret, ok := output.Data.(*protocol.Secret)
		if !ok || secret == nil || secret.Expose() != key.Expose() {
			return errors.New("execution session resolved another Node's credential")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	bundle := protocol.DefaultSessionImport()
	bundle.Source, bundle.Config, bundle.Provider = a.ID(), config, provider
	imported := dispatchFixture(t, b, a.ID(), protocol.NewRequest(b.ID(), protocol.Command{Kind: "import_session", Data: &bundle}))
	if imported.Fault == nil || imported.Fault.Code != protocol.ErrorCodeNotConfigured {
		t.Fatalf("direct import bypassed execution-Node model ownership: %v", imported.Fault)
	}
}

func TestPreviouslyAdmittedImportReplayUsesTargetLedger(t *testing.T) {
	t.Parallel()
	target := nodeFixture(t)
	controller := protocol.NodeID{71}
	owner := forwardingControllerFixture(t, target, controller)
	var project protocol.Project
	if err := target.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
		path := t.TempDir()
		draft := &protocol.ProjectsDraft{Name: "Replay fixture", Path: path, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}}
		output, _, err := store.ExecuteCommand(owner, db, target.ID(), target.ID(), target.profile.Ownership.Path,
			protocol.NewRequest(target.ID(), protocol.Command{Kind: "create_project", Data: draft}), store.PreparedPaths{Path: path})
		if err != nil {
			return err
		}
		project = output.Data.(protocol.Project)
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	bundle := protocol.DefaultSessionImport()
	bundle.Source, bundle.Project = controller, &project.ID
	bundle.Config = protocol.DefaultSessionConfig()
	bundle.Config.Provider, bundle.Config.Model = protocol.NewProviderID(), "fixture"
	bundle.Config.Mode, bundle.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	bundle.Provider = protocol.DefaultConversationProvider()
	bundle.Provider.ID = bundle.Config.Provider
	bundle.Provider.Api, bundle.Provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	bundle.Roles = protocol.DefaultRoleSnapshot()
	request := protocol.NewRequest(target.ID(), protocol.Command{Kind: "import_session", Data: &bundle})
	if err := target.profile.Store.Node.Read(owner, func(db *sql.Conn) error {
		return target.authorizeCommandDB(owner, db, controller, request)
	}); err != nil {
		t.Fatalf("request resources are not accessible before ledger replay: %v", err)
	}
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	want := protocol.Output{Kind: "providers", Data: []protocol.ConversationProvider{}}
	if err := target.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
		if _, err := admission.Admit(owner, db, controller, request, body); err != nil {
			return err
		}
		return admission.Complete(owner, db, controller, request.ID, link.Response{Output: want})
	}); err != nil {
		t.Fatal(err)
	}
	replayed := dispatchFixture(t, target, controller, request)
	if replayed.Fault != nil || replayed.Output.Kind != want.Kind {
		t.Fatalf("target did not resolve the exact prior request: fault=%v output=%#v", replayed.Fault, replayed.Output)
	}
}

func TestUncertainForwardReplayKeepsFrozenImport(t *testing.T) {
	t.Parallel()
	a, b := nodeFixture(t), nodeFixture(t)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = protocol.NewProviderID(), "captured-model"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	request := protocol.NewRequest(a.ID(), protocol.Command{Kind: "create_session_at", Data: &protocol.CommandCreateSessionAtPayload{
		Target: b.ID(), Config: &config, ProviderRevision: 1,
	}})
	caller := a.ID()
	bundle := protocol.DefaultSessionImport()
	bundle.Source, bundle.Config = a.ID(), config
	bundle.Provider = protocol.DefaultConversationProvider()
	bundle.Provider.ID = config.Provider
	bundle.Provider.Api, bundle.Provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	bundle.Roles = protocol.DefaultRoleSnapshot()
	frozen := protocol.Request{Version: request.Version, ID: request.ID, Target: b.ID(), Command: protocol.Command{Kind: "import_session", Data: &bundle}}
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(frozen)
	if err != nil {
		t.Fatal(err)
	}
	if err := a.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		if _, err := admission.Admit(t.Context(), db, a.ID(), request, body); err != nil {
			return err
		}
		if _, err := db.ExecContext(t.Context(), "INSERT INTO session_forwards(caller,request,body) VALUES(?1,?2,?3)", caller[:], request.ID.String(), encoded); err != nil {
			return err
		}
		replayed, err := store.PrepareSessionForward(t.Context(), db, a.ID(), a.ID(), request)
		if err != nil {
			return err
		}
		replayedBody, err := json.Marshal(replayed)
		if err != nil {
			return err
		}
		if string(replayedBody) != string(encoded) {
			return errors.New("uncertain request did not reuse its frozen import")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestAssociatedSessionsPagesNewestFirst(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	provider := forwardingProvider(t, node, "https://models.invalid/v1")
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = provider.ID, provider.DefaultModel, provider.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	association := protocol.ConversationAssociation{Namespace: "fixture", Kind: "ticket", ID: "42"}
	var started []protocol.SessionID
	for index := range 3 {
		draft := protocol.DefaultConversationStart()
		draft.Config, draft.Title, draft.Execution = &config, fmt.Sprintf("Ticket %d", index), protocol.ConversationExecutionModel
		draft.Message.Text = "Test request"
		draft.Association = &association
		if index == 2 {
			draft.Association = &protocol.ConversationAssociation{Namespace: "fixture", Kind: "ticket", ID: "43"}
		}
		queued, ok := commandFixture(t, node, protocol.Command{Kind: "start_session", Data: draft}).Data.(protocol.QueuedTurn)
		if !ok {
			t.Fatal("start_session returned an unexpected result")
		}
		started = append(started, queued.Session)
	}
	list := func(before *protocol.SessionID, limit uint16) link.Response {
		return dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "list_associated_sessions", Data: &protocol.CommandListAssociatedSessionsPayload{Association: association, Before: before, Limit: limit}}))
	}
	first := list(nil, 1)
	if first.Fault != nil {
		t.Fatal(first.Fault)
	}
	page, ok := first.Output.Data.(protocol.ConversationAssociationAssociatedSessions)
	if !ok || len(page.Sessions) != 1 || page.Sessions[0].ID != started[1] || page.NextBefore == nil || *page.NextBefore != started[1] {
		t.Fatalf("first page = %#v", first.Output.Data)
	}
	second := list(page.NextBefore, 1)
	if second.Fault != nil {
		t.Fatal(second.Fault)
	}
	page, ok = second.Output.Data.(protocol.ConversationAssociationAssociatedSessions)
	if !ok || len(page.Sessions) != 1 || page.Sessions[0].ID != started[0] || page.NextBefore != nil {
		t.Fatalf("second page = %#v", second.Output.Data)
	}
	for _, limit := range []uint16{0, 101} {
		invalid := list(nil, limit)
		if invalid.Fault == nil || invalid.Fault.Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("limit %d returned %v", limit, invalid.Fault)
		}
	}
	wrongCursor := list(&started[2], 1)
	if wrongCursor.Fault == nil || wrongCursor.Fault.Code != protocol.ErrorCodeWrongTarget {
		t.Fatalf("wrong association cursor returned %v", wrongCursor.Fault)
	}
}

func TestSearchModelBindingCapturesUnsecuredLocalProvider(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	provider := forwardingProvider(t, node, "https://models.invalid/v1")
	provider.Credential = nil
	binding, err := store.ResolveSearchBinding(context.Background(), nil, node.ID(), provider, "fixture", protocol.NewSessionID())
	if err != nil || binding.Config.Model != "fixture" || binding.Config.Provider != provider.ID {
		t.Fatalf("search binding = %#v, %v", binding.Config, err)
	}
}
