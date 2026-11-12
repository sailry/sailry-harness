package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const forwardedKey = "forwarded-key-7f3a"

// forwardingProvider saves a key-based provider and returns it as listed.
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
	commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	for _, saved := range commandFixture(t, node, protocol.Command{Kind: "list_providers"}).Data.([]protocol.ConversationProvider) {
		if saved.ID == provider.ID {
			return saved
		}
	}
	t.Fatal("saved provider is not listed")
	return provider
}

// nodeText returns everything a controller can read from the Node, together
// with its admission ledger and committed events.
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

// storedProvider reads one provider row as it is stored, secret included.
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

// inspectForward asks the Node what its ledger holds for a request of its own
// controller.
func inspectForward(t *testing.T, node *Node, request protocol.Request) protocol.RequestOutcome {
	t.Helper()
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	query := protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: request.ID, Digest: admission.Digest(body)}}
	return commandFixture(t, node, query).Data.(protocol.OutputRequestOutcomePayload).Outcome
}

// A session created on a paired Node carries the local provider, its
// transferable secret and the captured roles. The target keeps them private to
// that session. Over 1 s: each Node start installs and verifies the bundled
// packages.
func TestSessionForwarding(t *testing.T) {
	t.Parallel()
	authorizations := make(chan string, 16)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case authorizations <- r.Header.Get("Authorization"):
		default:
		}
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\ndata: [DONE]\n\n")
	}))
	t.Cleanup(server.Close)
	a, b := nodeFixture(t), nodeFixture(t)
	sends := 0
	paired := func(ctx context.Context, request protocol.Request) (link.Admission, error) {
		sends++
		return b.Dispatch(ctx, a.ID(), request)
	}
	a.forward = paired
	provider := forwardingProvider(t, a, server.URL+"/v1")
	if provider.Credential == nil {
		t.Fatal("saved provider has no credential")
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = provider.ID, "fixture", provider.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	forward := func(revision uint64, selected protocol.SessionConfig) protocol.Request {
		return protocol.NewRequest(a.ID(), protocol.Command{Kind: "create_session_at", Data: protocol.CommandCreateSessionAtPayload{Target: b.ID(), Config: &selected, ProviderRevision: revision}})
	}
	fails := func(t *testing.T, result link.Response, code protocol.ErrorCode, message string) {
		t.Helper()
		if result.Fault == nil || result.Fault.Code != code || result.Fault.Message != message {
			t.Fatalf("result = %#v, fault %v; want %s %q", result.Output, result.Fault, code, message)
		}
	}
	sessions := func(node *Node) int { return len(snapshotFixture(t, node).Sessions) }

	request := forward(provider.Revision, config)
	created := dispatchFixture(t, a, a.ID(), request)
	if created.Fault != nil {
		t.Fatal(created.Fault)
	}
	session := created.Output.Data.(protocol.Session)

	t.Run("target stores a private provider", func(t *testing.T) {
		stored := commandFixture(t, b, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: session.ID}}).Data.(protocol.Session)
		if stored.Profile == nil || stored.Profile.Source != a.ID() || stored.Config.Provider == provider.ID || stored.Profile.Provider.ID != stored.Config.Provider ||
			stored.Config.Credential == nil || stored.Config.Credential.Node != b.ID() || stored.Config.Credential.ID == provider.Credential.ID {
			t.Fatalf("imported session = %#v", stored)
		}
		if sessions(a) != 0 || sessions(b) != 1 {
			t.Fatalf("sessions: source %d, target %d", sessions(a), sessions(b))
		}
		for _, listed := range commandFixture(t, b, protocol.Command{Kind: "list_providers"}).Data.([]protocol.ConversationProvider) {
			if listed.ID == stored.Config.Provider || listed.Name == provider.Name {
				t.Fatalf("forwarded provider is listed on the target: %#v", listed)
			}
		}
		body, exists := storedProvider(t, b, stored.Config.Provider)
		if !exists || !strings.Contains(body, `"listed":false`) || !strings.Contains(body, forwardedKey) {
			t.Fatalf("private provider row = %s", body)
		}
	})

	t.Run("secret reaches no controller, ledger or event", func(t *testing.T) {
		for name, node := range map[string]*Node{"source": a, "target": b} {
			if text := nodeText(t, node); strings.Contains(text, forwardedKey) {
				t.Fatalf("%s exposes the forwarded secret: %s", name, text)
			}
		}
		// The target retains the import by digest, which still answers inspection.
		var body []byte
		caller := a.ID()
		err := b.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
			return db.QueryRowContext(context.Background(), "SELECT body FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&body)
		})
		digest, retained := strings.CutPrefix(string(body), "credential-v1:")
		if err != nil || !retained || len(digest) != 64 {
			t.Fatalf("import admission = %q, %v", body, err)
		}
		inspected := dispatchFixture(t, b, a.ID(), protocol.NewRequest(b.ID(), protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: request.ID, Digest: digest}}))
		if inspected.Fault != nil || inspected.Output.Data.(protocol.OutputRequestOutcomePayload).Outcome.Kind != "completed" {
			t.Fatalf("import inspection = %#v, %v", inspected.Output, inspected.Fault)
		}
	})

	t.Run("replay returns the original session", func(t *testing.T) {
		before := sends
		replayed := dispatchFixture(t, a, a.ID(), request)
		if replayed.Fault != nil {
			t.Fatal(replayed.Fault)
		}
		body, err := json.Marshal(replayed.Output.Data)
		if err != nil {
			t.Fatal(err)
		}
		var recovered protocol.Session
		if err := json.Unmarshal(body, &recovered); err != nil {
			t.Fatal(err)
		}
		if recovered.ID != session.ID || sends != before || sessions(b) != 1 {
			t.Fatalf("replay: session %s, %d new sends, %d target sessions", recovered.ID, sends-before, sessions(b))
		}
		changed := request
		other := config
		other.Model = "another"
		changed.Command = forward(provider.Revision, other).Command
		if _, err := a.Dispatch(context.Background(), a.ID(), changed); err == nil || err.(*protocol.Fault).Code != protocol.ErrorCodeConflict {
			t.Fatalf("reused request identifier: %v", err)
		}
	})

	t.Run("turn authorizes with the forwarded credential", func(t *testing.T) {
		input := protocol.DefaultConversationInput()
		input.Text = "Say hello"
		queued := dispatchFixture(t, b, a.ID(), protocol.NewRequest(b.ID(), protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: input}}))
		if queued.Fault != nil {
			t.Fatal(queued.Fault)
		}
		select {
		case authorization := <-authorizations:
			if authorization != "Bearer "+forwardedKey {
				t.Fatalf("model request authorization = %q", authorization)
			}
		case <-t.Context().Done():
			t.Fatal(context.Cause(t.Context()))
		}
	})

	t.Run("source refuses", func(t *testing.T) {
		before, target := sends, sessions(b)
		fails(t, dispatchFixture(t, a, a.ID(), forward(provider.Revision+1, config)), protocol.ErrorCodeRevisionConflict, "source provider revision changed")
		fails(t, dispatchFixture(t, a, b.ID(), forward(provider.Revision, config)), protocol.ErrorCodePermissionDenied, "only the local controller can forward configuration to another Node")
		missing := config
		missing.Provider = protocol.NewProviderID()
		missing.Credential = nil
		fails(t, dispatchFixture(t, a, a.ID(), forward(provider.Revision, missing)), protocol.ErrorCodeNotFound, "source provider does not exist")

		// An account sign-in stays with the Node that performed it.
		account := provider
		account.ID, account.Name, account.Authentication = protocol.NewProviderID(), "Account", protocol.AuthenticationClaudeOAuth
		account.Credential = &protocol.CredentialRef{Node: a.ID(), ID: protocol.NewCredentialID()}
		public, err := json.Marshal(account)
		if err != nil {
			t.Fatal(err)
		}
		row := fmt.Sprintf(`{"id":%q,"provider":%s,"listed":true,"authorizations":{%q:{"authentication":"claude_oauth","revision":1,"key":"account-grant"}},"quota":null}`,
			account.ID.String(), public, account.Credential.ID.String())
		if err := a.profile.Store.Node.Write(context.Background(), func(db *sql.Conn) error {
			_, err := db.ExecContext(context.Background(), "INSERT INTO providers(id,body) VALUES(?1,?2)", account.ID.String(), []byte(row))
			return err
		}); err != nil {
			t.Fatal(err)
		}
		signed := config
		signed.Provider, signed.Credential = account.ID, account.Credential
		fails(t, dispatchFixture(t, a, a.ID(), forward(account.Revision, signed)), protocol.ErrorCodeNotConfigured, "Claude sign-in is required on the execution Node")
		// The explicit selection is authoritative: no credential forwards no secret.
		unsigned := config
		unsigned.Credential = nil
		bare := dispatchFixture(t, a, a.ID(), forward(provider.Revision, unsigned))
		if bare.Fault != nil || bare.Output.Data.(protocol.Session).Config.Credential != nil {
			t.Fatalf("forward without a credential = %#v, %v", bare.Output, bare.Fault)
		}
		if sends != before+1 || sessions(b) != target+1 {
			t.Fatalf("refusals reached the target: %d sends, %d sessions", sends-before, sessions(b)-target)
		}
	})

	t.Run("target refuses", func(t *testing.T) {
		before := sessions(b)
		bundle := protocol.DefaultSessionImport()
		bundle.Source, bundle.Config, bundle.Provider = a.ID(), config, provider
		secret := protocol.NewSecret(forwardedKey)
		bundle.Secret = &secret
		bundle.Roles.Execution = protocol.ConversationExecutionModel
		send := func(caller protocol.NodeID, bundle protocol.SessionImport) link.Response {
			return dispatchFixture(t, b, caller, protocol.NewRequest(b.ID(), protocol.Command{Kind: "import_session", Data: bundle}))
		}
		fails(t, send(b.ID(), bundle), protocol.ErrorCodePermissionDenied, "configuration source does not match the paired caller")
		local := bundle
		local.Source = b.ID()
		fails(t, send(b.ID(), local), protocol.ErrorCodePermissionDenied, "configuration source does not match the paired caller")
		unsecured := bundle
		unsecured.Secret = nil
		fails(t, send(a.ID(), unsecured), protocol.ErrorCodeInvalidRequest, "configuration import is inconsistent")
		account := bundle
		account.Provider.Authentication = protocol.AuthenticationChatGptOAuth
		fails(t, send(a.ID(), account), protocol.ErrorCodeNotConfigured, "ChatGPT sign-in is required on the execution Node")
		roles := bundle
		roles.RoleCredentials = []protocol.RoleProviderSecret{{Provider: protocol.NewProviderID(), Secret: secret}}
		fails(t, send(a.ID(), roles), protocol.ErrorCodeInvalidRequest, "role credential import is inconsistent")
		if sessions(b) != before {
			t.Fatalf("a refused import created %d sessions", sessions(b)-before)
		}
		if text := nodeText(t, b); strings.Contains(text, forwardedKey) {
			t.Fatalf("a refused import retained its secret: %s", text)
		}
		var rows int
		if err := b.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
			return db.QueryRowContext(context.Background(), "SELECT count(*) FROM providers WHERE instr(body,?1)>0", forwardedKey).Scan(&rows)
		}); err != nil || rows != 1 {
			t.Fatalf("%d provider rows hold the forwarded secret, %v", rows, err)
		}
	})

	t.Run("a replay resolves a lost outcome", func(t *testing.T) {
		const unknown = "remote session creation could not be confirmed; send the same request again to resolve it"
		before, calls := sessions(b), 0
		// The target admits the import and its answer never arrives.
		a.forward = func(ctx context.Context, request protocol.Request) (link.Admission, error) {
			calls++
			accepted, err := b.Dispatch(ctx, a.ID(), request)
			if err != nil {
				return link.Admission{}, err
			}
			<-accepted.Completion
			lost := make(chan link.Response)
			close(lost)
			return link.Admission{Completion: lost}, nil
		}
		uncertain := forward(provider.Revision, config)
		for range 2 {
			fails(t, dispatchFixture(t, a, a.ID(), uncertain), protocol.ErrorCodeOutcomeUnknown, unknown)
		}
		if calls != 2 || sessions(b) != before+1 {
			t.Fatalf("unknown forward: %d sends, %d sessions", calls, sessions(b)-before)
		}
		// A target that holds other content under the identifier settles nothing.
		a.forward = func(context.Context, protocol.Request) (link.Admission, error) {
			refused := make(chan link.Response, 1)
			refused <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "request identifier was already used for different content"}}
			close(refused)
			return link.Admission{Completion: refused}, nil
		}
		fails(t, dispatchFixture(t, a, a.ID(), uncertain), protocol.ErrorCodeOutcomeUnknown, unknown)
		a.forward = nil
		fails(t, dispatchFixture(t, a, a.ID(), uncertain), protocol.ErrorCodeOutcomeUnknown, unknown)
		a.forward = paired
		resolved := dispatchFixture(t, a, a.ID(), uncertain)
		// The target answers from its ledger, which decodes to a pointer.
		recorded, valid := resolved.Output.Data.(*protocol.Session)
		if resolved.Fault != nil || !valid {
			t.Fatalf("resolved = %#v, %v", resolved.Output, resolved.Fault)
		}
		stored := commandFixture(t, b, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: recorded.ID}}).Data.(protocol.Session)
		if stored.Profile == nil || stored.Profile.Source != a.ID() || sessions(b) != before+1 {
			t.Fatalf("resolved session = %#v, %d created", stored, sessions(b)-before)
		}
		// The target's answer replaced the unknown outcome in the source
		// ledger: a replay and an inspection read it without another send.
		sent := sends
		replayed := dispatchFixture(t, a, a.ID(), uncertain)
		again, valid := replayed.Output.Data.(*protocol.Session)
		if replayed.Fault != nil || !valid || again.ID != recorded.ID || sends != sent {
			t.Fatalf("replay after the answer = %#v, %v, %d sends", replayed.Output, replayed.Fault, sends-sent)
		}
		if outcome := inspectForward(t, a, uncertain); outcome.Kind != "completed" || outcome.Data.(protocol.Result[protocol.Output, protocol.Fault]).Ok == nil {
			t.Fatalf("source ledger after the answer = %#v", outcome)
		}
		// A send that fails here is not the target's answer and settles nothing.
		lost := forward(provider.Revision, config)
		a.forward = func(context.Context, protocol.Request) (link.Admission, error) {
			closed := make(chan link.Response)
			close(closed)
			return link.Admission{Completion: closed}, nil
		}
		fails(t, dispatchFixture(t, a, a.ID(), lost), protocol.ErrorCodeOutcomeUnknown, unknown)
		a.forward = func(context.Context, protocol.Request) (link.Admission, error) {
			return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "target Node is not paired"}
		}
		fails(t, dispatchFixture(t, a, a.ID(), lost), protocol.ErrorCodeOutcomeUnknown, unknown)
		if outcome := inspectForward(t, a, lost); outcome.Kind != "completed" || outcome.Data.(protocol.Result[protocol.Output, protocol.Fault]).Err == nil {
			t.Fatalf("source ledger without an answer = %#v", outcome)
		}
		a.forward = nil
		fails(t, dispatchFixture(t, a, a.ID(), forward(provider.Revision, config)), protocol.ErrorCodeBusy, "Node operation queue is unavailable")
		a.forward = paired
	})

	t.Run("a result that could not be stored is written again", func(t *testing.T) {
		// The writer's connection refuses to store a result until the gate opens.
		gate := func(statement string) {
			t.Helper()
			if err := a.profile.Store.Node.Write(context.Background(), func(db *sql.Conn) error {
				_, err := db.ExecContext(context.Background(), statement)
				return err
			}); err != nil {
				t.Fatal(err)
			}
		}
		gate("CREATE TEMP TABLE forward_gate AS SELECT 1 AS closed")
		gate("CREATE TEMP TRIGGER forward_refused BEFORE UPDATE OF status ON requests WHEN (SELECT closed FROM forward_gate)=1 BEGIN SELECT RAISE(ABORT,'result write refused'); END")
		defer gate("DROP TRIGGER forward_refused")
		before := sessions(b)
		unrecorded := forward(provider.Revision, config)
		accepted, err := a.Dispatch(context.Background(), a.ID(), unrecorded)
		if err != nil {
			t.Fatal(err)
		}
		for sessions(b) == before && t.Context().Err() == nil {
		}
		select {
		case early := <-accepted.Completion:
			t.Fatalf("answered before the result was stored: %#v, %v", early.Output, early.Fault)
		default:
		}
		if outcome := inspectForward(t, a, unrecorded); outcome.Kind != "admitted" {
			t.Fatalf("while the result is unrecorded = %#v", outcome)
		}
		gate("UPDATE forward_gate SET closed=0")
		result := <-accepted.Completion
		if _, valid := result.Output.Data.(protocol.Session); result.Fault != nil || !valid {
			t.Fatalf("result = %#v, %v", result.Output, result.Fault)
		}
		if outcome := inspectForward(t, a, unrecorded); outcome.Kind != "completed" || outcome.Data.(protocol.Result[protocol.Output, protocol.Fault]).Ok == nil {
			t.Fatalf("stored outcome = %#v", outcome)
		}
	})

	t.Run("removal deletes the private provider", func(t *testing.T) {
		current := commandFixture(t, b, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: session.ID}}).Data.(protocol.Session)
		private := current.Config.Provider
		for {
			removed := dispatchFixture(t, b, a.ID(), protocol.NewRequest(b.ID(), protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: session.ID, ExpectedRevision: current.Revision}}))
			if removed.Fault == nil {
				break
			}
			// The turn above may still be finishing.
			if removed.Fault.Code != protocol.ErrorCodeBusy || t.Context().Err() != nil {
				t.Fatal(removed.Fault)
			}
		}
		if body, exists := storedProvider(t, b, private); exists {
			t.Fatalf("private provider outlived its session: %s", body)
		}
		for _, credential := range commandFixture(t, b, protocol.Command{Kind: "list_credentials"}).Data.([]protocol.Credential) {
			if credential.Provider == private {
				t.Fatalf("private credential outlived its session: %#v", credential)
			}
		}
		if _, exists := storedProvider(t, a, provider.ID); !exists {
			t.Fatal("removal on the target touched the source provider")
		}
	})

	t.Run("associated sessions page newest first", func(t *testing.T) {
		association := protocol.ConversationAssociation{Namespace: "fixture", Kind: "ticket", ID: "42"}
		local := config
		var started []protocol.SessionID
		for index := range 3 {
			draft := protocol.DefaultConversationStart()
			draft.Config, draft.Title, draft.Execution = &local, fmt.Sprintf("Ticket %d", index), protocol.ConversationExecutionModel
			draft.Message.Text = "Say hello"
			draft.Association = &association
			if index == 2 {
				draft.Association = &protocol.ConversationAssociation{Namespace: "fixture", Kind: "ticket", ID: "43"}
			}
			started = append(started, commandFixture(t, a, protocol.Command{Kind: "start_session", Data: draft}).Data.(protocol.QueuedTurn).Session)
		}
		list := func(before *protocol.SessionID, limit uint16) link.Response {
			return dispatchFixture(t, a, a.ID(), protocol.NewRequest(a.ID(), protocol.Command{Kind: "list_associated_sessions", Data: protocol.CommandListAssociatedSessionsPayload{Association: association, Before: before, Limit: limit}}))
		}
		first := list(nil, 1)
		if first.Fault != nil {
			t.Fatal(first.Fault)
		}
		page := first.Output.Data.(protocol.ConversationAssociationAssociatedSessions)
		if len(page.Sessions) != 1 || page.Sessions[0].ID != started[1] || page.NextBefore == nil || *page.NextBefore != started[1] {
			t.Fatalf("first page = %#v", page)
		}
		second := list(page.NextBefore, 1)
		if second.Fault != nil {
			t.Fatal(second.Fault)
		}
		page = second.Output.Data.(protocol.ConversationAssociationAssociatedSessions)
		if len(page.Sessions) != 1 || page.Sessions[0].ID != started[0] || page.NextBefore != nil {
			t.Fatalf("second page = %#v", page)
		}
		fails(t, list(nil, 0), protocol.ErrorCodeInvalidRequest, "session page limit must be between 1 and 100")
		fails(t, list(nil, 101), protocol.ErrorCodeInvalidRequest, "session page limit must be between 1 and 100")
		fails(t, list(&started[2], 1), protocol.ErrorCodeWrongTarget, "session cursor belongs to another association")
	})

	t.Run("search model binding captures a valid configuration", func(t *testing.T) {
		unsecured := provider
		unsecured.Credential = nil
		binding, err := store.ResolveSearchBinding(context.Background(), nil, a.ID(), unsecured, "fixture", protocol.NewSessionID())
		if err != nil || binding.Config.Model != "fixture" || binding.Config.Provider != provider.ID {
			t.Fatalf("search binding = %#v, %v", binding.Config, err)
		}
	})
}
