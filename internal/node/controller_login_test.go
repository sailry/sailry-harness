package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func controllerLoginRunFixture(t *testing.T) (*Node, context.Context, *store.AgentInvocation, protocol.RuntimeCredential, func(protocol.RuntimeCredential)) {
	t.Helper()
	node, ctx, provider, reference, write := runtimeCredentialFixture(t)
	provider.Revision, provider.Enabled, provider.DefaultModel, provider.Name = 1, true, "fixture", "Login fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output, model.Tools = "fixture", 32000, 8000, true
	provider.Models, provider.Credential = []protocol.ConversationModel{model}, &reference
	expired := uint64(1)
	runtime := protocol.RuntimeCredential{Kind: "provider", ID: reference.ID.String(), Generation: reference.ID.String(), Revision: 1, State: "ready",
		Provider: &provider.ID, Authentication: provider.Authentication, Endpoint: provider.Endpoint, Access: protocol.NewSecret("expired-access"), ExpiresAtMs: &expired,
		AccountIdentity: "trusted-account-workspace", AuthenticationTarget: "captured-target"}
	write(runtime)
	var invocation *store.AgentInvocation
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		if err := store.PairLinkPeer(ctx, db, reference.Node, link.DirectionController); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE link_peers SET user=?2 WHERE identity=?1", reference.Node[:], user.ID.String()); err != nil {
			return err
		}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, model.ID, protocol.WorkModeCode, protocol.PermissionProject
		config.Credential = &reference
		roles := protocol.DefaultRoleSnapshot()
		roles.Execution = protocol.ConversationExecutionModel
		captured := protocol.SessionProfile{Source: reference.Node, Provider: provider}
		localConfig := config
		localProvider := provider
		localProvider.Authentication = protocol.AuthenticationApiKey
		localProvider.Api = protocol.ConversationModelApiResponses
		localProvider.Credential = nil
		localProvider.Preset, localProvider.Oauth = nil, nil
		localProvider.Models = []protocol.ConversationModel{model}
		localProvider.Enabled = true
		localSecret := protocol.NewSecret("controller-login-local-fixture-key")
		localOutput, _, err := store.ExecuteProviderCommand(ctx, db, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: localProvider, Secret: &localSecret}}), nil)
		if err != nil {
			return err
		}
		localSaved := localOutput.Data.(protocol.ConversationProvider)
		localConfig.Credential = localSaved.Credential
		output, _, err := store.ExecuteCommand(ctx, db, node.ID(), reference.Node, node.profile.Ownership.Path,
			protocol.NewRequest(node.ID(), protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &localConfig}}), store.PreparedPaths{})
		if err != nil {
			return err
		}
		session := output.Data.(protocol.Session)
		session, err = store.ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Original admitted input"},
		}})
		encoded, err := json.Marshal(request)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", reference.Node[:], request.ID.String(), encoded); err != nil {
			return err
		}
		admitted, _, err := store.ExecuteAgentCommand(ctx, db, node.ID(), node.profile.Ownership.Path, reference.Node, request, time.Now())
		if err != nil {
			return err
		}
		queued, ok := admitted.Data.(protocol.QueuedTurn)
		if !ok {
			return fmt.Errorf("fixture turn was not admitted")
		}
		configBody, err := json.Marshal(config)
		if err != nil {
			return err
		}
		profileBody, err := json.Marshal(captured)
		if err != nil {
			return err
		}
		rolesBody, err := json.Marshal(roles)
		if err != nil {
			return err
		}
		providerBody, err := json.Marshal(provider)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE session_revisions SET config=?3,profile=?4,roles=?5 WHERE session=?1 AND revision=?2", session.ID.String(), session.Revision, configBody, profileBody, rolesBody); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE agent_runs SET provider=?2 WHERE turn=?1", queued.ID.String(), providerBody); err != nil {
			return err
		}
		invocation, _, err = store.ClaimNext(ctx, db, time.Now())
		if err == nil && invocation == nil {
			return fmt.Errorf("fixture task was not claimed")
		}
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := node.setCredentialWait(ctx, invocation.Run.Turn, &protocol.ConversationRunWaiting{Kind: "controller_login", Credential: reference, Provider: provider.ID}); err != nil {
		t.Fatal(err)
	}
	return node, ctx, invocation, runtime, write
}

func TestControllerLoginRecoveryRequiresRetainedReplacementAndReplaysReceipt(t *testing.T) {
	t.Parallel()
	node, ctx, invocation, old, write := controllerLoginRunFixture(t)
	var run protocol.ConversationRun
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		run, err = store.ReadAgentRun(ctx, db, invocation.Run.Turn)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	replacement := protocol.CredentialRef{Node: run.Waiting.Credential.Node, ID: protocol.NewCredentialID()}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "resume_controller_login", Data: &protocol.CommandResumeControllerLoginPayload{
		Session: run.Session, Turn: run.Turn, Waiting: run.Waiting.Attempt, Previous: run.Waiting.Credential, Replacement: replacement,
	}})
	operation, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	authorized := make(chan error, 1)
	go func() { authorized <- node.authorizeTurnProvider(operation, invocation) }()
	_, err := node.Dispatch(operation, replacement.Node, request)
	var failure *protocol.Fault
	if !errors.As(err, &failure) || failure.ConfigurationReason() != protocol.ConfigurationRuntimeCredentialMissing {
		t.Fatalf("missing replacement was not refused before admission: %v", err)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var count int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM requests WHERE caller=?1 AND id=?2", replacement.Node[:], request.ID.String()).Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			return fmt.Errorf("missing replacement was durably admitted")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	future := uint64(time.Now().Add(time.Hour).UnixMilli())
	next := old
	next.ID, next.Generation, next.Access, next.ExpiresAtMs = replacement.ID.String(), replacement.ID.String(), protocol.NewSecret("fresh-access"), &future
	write(next)
	accepted, err := node.Dispatch(operation, replacement.Node, request)
	if err != nil {
		t.Fatal(err)
	}
	var original link.Response
	select {
	case original = <-accepted.Completion:
	case <-operation.Done():
		t.Fatal(operation.Err())
	}
	if original.Fault != nil {
		t.Fatal(original.Fault)
	}
	// Discarding this receipt and delivering the same request again must return
	// that recorded result, even though the wait has already been cleared.
	replayed := dispatchFixture(t, node, replacement.Node, request)
	originalBody, _ := json.Marshal(original.Output)
	replayedBody, _ := json.Marshal(replayed.Output)
	if replayed.Fault != nil || string(originalBody) != string(replayedBody) {
		t.Fatalf("lost receipt recovery changed the result: %+v", replayed)
	}
	select {
	case err := <-authorized:
		if err != nil {
			t.Fatal(err)
		}
	case <-operation.Done():
		t.Fatal(operation.Err())
	}
	if invocation.EffectiveCredential == nil || *invocation.EffectiveCredential != replacement {
		t.Fatal("the original invocation did not resolve its explicit replacement")
	}
	if invocation.Turn.Config.Credential == nil || *invocation.Turn.Config.Credential == replacement {
		t.Fatal("recovery overwrote admission capture")
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var recoveries, turns int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM agent_credential_recoveries WHERE turn=?1", run.Turn.String()).Scan(&recoveries); err != nil {
			return err
		}
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM turns WHERE session=?1", run.Session.String()).Scan(&turns); err != nil {
			return err
		}
		if recoveries != 1 || turns != 1 {
			return fmt.Errorf("recovery duplicated work: recoveries=%d turns=%d", recoveries, turns)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestControllerLoginRecoveryRejectsStoppedTask(t *testing.T) {
	t.Parallel()
	node, ctx, invocation, _, _ := controllerLoginRunFixture(t)
	var run protocol.ConversationRun
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		run, err = store.ReadAgentRun(ctx, db, invocation.Run.Turn)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	input := protocol.CommandResumeControllerLoginPayload{Session: run.Session, Turn: run.Turn, Waiting: run.Waiting.Attempt,
		Previous: run.Waiting.Credential, Replacement: protocol.CredentialRef{Node: run.Waiting.Credential.Node, ID: protocol.NewCredentialID()}}
	stopped := dispatchFixture(t, node, input.Previous.Node, protocol.NewRequest(node.ID(), protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: run.Turn}}))
	if stopped.Fault != nil {
		t.Fatal(stopped.Fault)
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "resume_controller_login", Data: input})
	result := dispatchFixture(t, node, input.Previous.Node, request)
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("stopped task accepted a new recovery: %+v", result)
	}
}

func TestControllerLoginNaturalRenewalClearsManualRecoveryWait(t *testing.T) {
	t.Parallel()
	node, ctx, invocation, runtime, write := controllerLoginRunFixture(t)
	operation, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	var run protocol.ConversationRun
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		run, err = store.ReadAgentRun(ctx, db, invocation.Run.Turn)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	input := protocol.CommandResumeControllerLoginPayload{Session: run.Session, Turn: run.Turn, Waiting: run.Waiting.Attempt,
		Previous: run.Waiting.Credential, Replacement: protocol.CredentialRef{Node: run.Waiting.Credential.Node, ID: protocol.NewCredentialID()}}
	waiting, clearing, releaseClear := make(chan struct{}, 1), make(chan struct{}), make(chan struct{})
	continued := make(chan error, 1)
	go func() {
		_, _, err := node.awaitProviderAuthorization(operation, *invocation.Provider, func() (protocol.CredentialRef, error) { return input.Previous, nil }, func(_ protocol.CredentialRef, pending bool) error {
			if pending {
				select {
				case waiting <- struct{}{}:
				default:
				}
				return nil
			}
			close(clearing)
			select {
			case <-operation.Done():
				return operation.Err()
			case <-releaseClear:
			}
			return node.setCredentialWait(context.WithoutCancel(operation), run.Turn, nil)
		})
		continued <- err
	}()
	select {
	case <-waiting:
	case <-operation.Done():
		t.Fatal(operation.Err())
	}
	future := uint64(time.Now().Add(time.Hour).UnixMilli())
	runtime.ExpiresAtMs, runtime.Revision = &future, runtime.Revision+1
	write(runtime)
	select {
	case <-clearing:
	case <-operation.Done():
		t.Fatal("ordinary renewal did not resume the original model authorization")
	}
	var failure *protocol.Fault
	err := node.checkControllerLoginReplacement(operation, input.Previous.Node, input)
	if !errors.As(err, &failure) || failure.ConfigurationReason() != protocol.ConfigurationRuntimeCredentialMissing {
		t.Fatalf("manual replacement check returned %v", err)
	}
	node.auth.mu.Lock()
	stateChanged := node.auth.changed
	node.auth.mu.Unlock()
	close(releaseClear)
	select {
	case err := <-continued:
		if err != nil {
			t.Fatal(err)
		}
	case <-operation.Done():
		t.Fatal(operation.Err())
	}
	select {
	case <-stateChanged:
	default:
		t.Fatal("naturally clearing the login wait did not notify credential listeners")
	}
	err = node.checkControllerLoginReplacement(operation, input.Previous.Node, input)
	if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("manual recovery did not notice natural task continuation: %v", err)
	}
}

func TestControllerLoginWaitingRefreshesSameReferenceLegacyMetadata(t *testing.T) {
	t.Parallel()
	node, ctx, invocation, runtime, write := controllerLoginRunFixture(t)
	operation, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	trustedAccount, trustedTarget := runtime.AccountIdentity, runtime.AuthenticationTarget
	runtime.AccountIdentity, runtime.AuthenticationTarget, runtime.Revision = "", "", runtime.Revision+1
	write(runtime)
	if err := node.setCredentialWait(ctx, invocation.Run.Turn, nil); err != nil {
		t.Fatal(err)
	}
	subscription, err := node.Subscribe(operation, invocation.Caller, protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	continued := make(chan error, 1)
	go func() { continued <- node.authorizeTurnProvider(operation, invocation) }()
	wait := func(ready func(protocol.ConversationRunWaiting) bool) protocol.ConversationRunWaiting {
		t.Helper()
		for {
			var run protocol.ConversationRun
			if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				run, err = store.ReadAgentRun(ctx, db, invocation.Run.Turn)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if run.Waiting != nil && ready(*run.Waiting) {
				return *run.Waiting
			}
			if _, err := subscription.Next(operation); err != nil {
				t.Fatal(err)
			}
		}
	}
	original := wait(func(waiting protocol.ConversationRunWaiting) bool {
		return waiting.AccountIdentity == "" && waiting.AuthenticationTarget == ""
	})
	runtime.AccountIdentity, runtime.AuthenticationTarget, runtime.Revision = trustedAccount, trustedTarget, runtime.Revision+1
	write(runtime)
	enriched := wait(func(waiting protocol.ConversationRunWaiting) bool {
		return waiting.AccountIdentity == trustedAccount && waiting.AuthenticationTarget == trustedTarget
	})
	if enriched.Attempt != original.Attempt || enriched.Credential != original.Credential {
		t.Fatal("trusted metadata replaced the original login wait identity")
	}
	future := uint64(time.Now().Add(time.Hour).UnixMilli())
	replacement := protocol.CredentialRef{Node: original.Credential.Node, ID: protocol.NewCredentialID()}
	runtime.ID, runtime.Generation, runtime.Access, runtime.ExpiresAtMs = replacement.ID.String(), replacement.ID.String(), protocol.NewSecret("same-account-login"), &future
	write(runtime)
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "resume_controller_login", Data: protocol.CommandResumeControllerLoginPayload{
		Session: invocation.Run.Session, Turn: invocation.Run.Turn, Waiting: original.Attempt, Previous: original.Credential, Replacement: replacement,
	}})
	if result := dispatchFixture(t, node, replacement.Node, request); result.Fault != nil {
		t.Fatal(result.Fault)
	}
	select {
	case err := <-continued:
		if err != nil {
			t.Fatal(err)
		}
	case <-operation.Done():
		t.Fatal("the enriched original wait did not resume")
	}
}

type controllerLoginStepModel struct {
	fantasy.LanguageModel
	requests atomic.Int32
	expire   func()
}

func (model *controllerLoginStepModel) Stream(_ context.Context, _ fantasy.Call) (fantasy.StreamResponse, error) {
	step := model.requests.Add(1)
	if step > 2 {
		return nil, fmt.Errorf("recovery repeated a model step")
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if step == 1 {
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: "title-once", ToolCallName: "set_session_title", ToolCallInput: `{"title":"Completed before login"}`}) {
				return
			}
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop}) {
				return
			}
			model.expire()
			return
		}
		if yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Continued the original task"}) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

func TestControllerLoginExplicitRecoveryContinuesAfterCompletedTool(t *testing.T) {
	t.Parallel()
	node, ctx, invocation, runtime, write := controllerLoginRunFixture(t)
	future := uint64(time.Now().Add(time.Hour).UnixMilli())
	runtime.ExpiresAtMs, runtime.Revision = &future, 2
	write(runtime)
	script := &controllerLoginStepModel{expire: func() {
		expired := uint64(1)
		runtime.ExpiresAtMs, runtime.Revision = &expired, 3
		write(runtime)
	}}
	node.modelFactory = func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil }
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		run, err := store.ReadAgentRun(ctx, db, invocation.Run.Turn)
		if err != nil {
			return err
		}
		run.Status, run.Waiting, run.StartedMs = protocol.ConversationStatusQueued, nil, nil
		body, err := json.Marshal(run)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "UPDATE agent_runs SET body=?2 WHERE turn=?1", run.Turn.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	operation, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	subscription, err := node.Subscribe(operation, invocation.Caller, protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	node.agent.Wake()
	read := func() (protocol.ConversationRun, []canonical.Record) {
		t.Helper()
		var run protocol.ConversationRun
		var records []canonical.Record
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			run, err = store.ReadAgentRun(ctx, db, invocation.Run.Turn)
			if err != nil {
				return err
			}
			state, err := store.LoadAgentState(ctx, db, invocation.Session.ID)
			records = state.Records
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return run, records
	}
	wait := func(done func(protocol.ConversationRun) bool) (protocol.ConversationRun, []canonical.Record) {
		t.Helper()
		for {
			run, records := read()
			if done(run) {
				return run, records
			}
			if run.Status == protocol.ConversationStatusFailed || run.Status == protocol.ConversationStatusInterrupted {
				t.Fatalf("fixture failed before acceptance: %+v", run)
			}
			if _, err := subscription.Next(operation); err != nil {
				t.Fatal(err)
			}
		}
	}
	toolCounts := func(records []canonical.Record) (int, int) {
		requests, responses := 0, 0
		for _, record := range records {
			if message := record.Message(); message != nil {
				for _, part := range message.Content {
					switch value := part.Data.(type) {
					case *canonical.ToolRequest:
						if value.ID == "title-once" {
							requests++
						}
					case *canonical.ToolResponse:
						if value.ID == "title-once" {
							responses++
						}
					}
				}
			}
		}
		return requests, responses
	}
	run, records := wait(func(run protocol.ConversationRun) bool {
		return run.Waiting != nil && run.Waiting.Kind == "controller_login"
	})
	requests, responses := toolCounts(records)
	if requests != 1 || responses != 1 || script.requests.Load() != 1 {
		t.Fatalf("task did not wait after its completed tool: requests=%d responses=%d models=%d", requests, responses, script.requests.Load())
	}
	replacement := protocol.CredentialRef{Node: run.Waiting.Credential.Node, ID: protocol.NewCredentialID()}
	next := runtime
	next.ID, next.Generation, next.Access, next.ExpiresAtMs = replacement.ID.String(), replacement.ID.String(), protocol.NewSecret("same-account-new-login"), &future
	write(next)
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "resume_controller_login", Data: protocol.CommandResumeControllerLoginPayload{
		Session: run.Session, Turn: run.Turn, Waiting: run.Waiting.Attempt, Previous: run.Waiting.Credential, Replacement: replacement,
	}})
	result := dispatchFixture(t, node, replacement.Node, request)
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	_, records = wait(func(run protocol.ConversationRun) bool { return run.Status == protocol.ConversationStatusCompleted })
	requests, responses = toolCounts(records)
	if requests != 1 || responses != 1 || script.requests.Load() != 2 {
		t.Fatalf("explicit login recovery replayed completed work: requests=%d responses=%d models=%d", requests, responses, script.requests.Load())
	}
	if replayed := dispatchFixture(t, node, replacement.Node, request); replayed.Fault != nil {
		t.Fatal(replayed.Fault)
	}
	if script.requests.Load() != 2 {
		t.Fatal("replaying recovery restarted a finished task")
	}
}
