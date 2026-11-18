package live

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestControllerLoginRecoveryProjectsOnlyExecutionNodeCredentials(t *testing.T) {
	node, other := protocol.NodeID{1}, protocol.NodeID{2}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node = node
	snapshot.Page.Session = protocol.SessionID{3}
	run := protocol.DefaultConversationRun()
	run.Session, run.Turn, run.Status = snapshot.Page.Session, protocol.TurnID{4}, protocol.ConversationStatusRunning
	run.Waiting = &protocol.ConversationRunWaiting{
		Kind: "controller_login", Attempt: protocol.RequestID{5}, Provider: protocol.ProviderID{6},
		Credential: protocol.CredentialRef{Node: node, ID: protocol.CredentialID{7}},
	}
	snapshot.Page.Runs = []protocol.ConversationRun{run}
	state := client.ConversationState{Connected: true, Snapshot: snapshot}
	turns, _, _, _ := Conversation(state, "en")
	if len(turns) != 1 || turns[0].Reply.Phase != conversation.WaitingController || !turns[0].Reply.LoginRecovery {
		t.Fatalf("execution Node credential lost its recovery action: %+v", turns)
	}

	run.Waiting.Credential.Node = other
	turns, _, _, _ = Conversation(state, "en")
	if len(turns) != 1 || turns[0].Reply.Phase != conversation.WaitingController || turns[0].Reply.LoginRecovery {
		t.Fatalf("historical remote credential offered an unsupported sign-in: %+v", turns)
	}
}

func TestControllerLoginRecoveryChecksEndedWaitAfterLostCompletion(t *testing.T) {
	for _, tc := range []struct {
		name                string
		cancelBeforeReply   bool
		differentCredential bool
	}{
		{name: "cleared after the reply was lost"},
		{name: "cancelled while the reply still hangs", cancelBeforeReply: true},
		{name: "replaced by another wait", differentCredential: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newControllerLoginFixture(t, "en")
				completion := make(chan link.Response)
				fixture.transport.completion = completion
				outcome := protocol.RequestOutcome{Kind: "admitted"}
				fixture.transport.handle = func(request protocol.Request) (link.Response, bool) {
					return controllerLoginTestOutcome(request, outcome)
				}
				attempt := &liveAuthorization{recovery: fixture.recovery}
				c := fixture.frame()
				fixture.owner.completeControllerLogin(c, attempt, &fixture.replacement)
				fixture.owner.completeControllerLogin(c, attempt, &fixture.replacement)
				fixture.frame()
				original := fixture.recovery.request
				if original == nil {
					t.Fatal("restore did not capture its request")
				}
				input, ok := detail[protocol.CommandResumeControllerLoginPayload](original.Command.Data)
				if !ok || input.Session != fixture.run.Session || input.Turn != fixture.run.Turn || input.Waiting != fixture.run.Waiting.Attempt ||
					input.Previous != fixture.run.Waiting.Credential || input.Replacement != fixture.replacement || original.Target != fixture.replacement.Node {
					t.Fatalf("restore did not capture the exact wait and credentials: %+v", original)
				}
				assertControllerLoginRequests(t, fixture.transport, original, 1, 0)
				inspections := 1
				if !tc.cancelBeforeReply {
					close(completion)
					fixture.frame()
					assertControllerLoginRequests(t, fixture.transport, original, 1, 1)
					inspections++
				}
				ended := fixture.run
				ended.Waiting = nil
				if tc.cancelBeforeReply {
					ended.Status = protocol.ConversationStatusCancelled
				} else if tc.differentCredential {
					waiting := *fixture.run.Waiting
					waiting.Attempt, waiting.Credential.ID = protocol.RequestID{10}, protocol.CredentialID{10}
					ended.Waiting = &waiting
				}
				if fixture.transport.history == nil {
					t.Fatal("the outstanding restore is not observing its wait")
				}
				fixture.transport.history.updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
					Node: original.Target, Session: ended.Session, Sequence: 2, Change: protocol.ConversationChange{Kind: "run", Data: ended},
				}}
				fixture.frame()
				if !fixture.recovery.waitEnded || fixture.recovery.settled || fixture.recovery.request != original || fixture.recovery.releaseWait != nil {
					t.Fatalf("ended wait settled or lost its request without a completion: %+v", fixture.recovery)
				}
				fixture.frame()
				assertControllerLoginRequests(t, fixture.transport, original, 1, inspections)

				result := protocol.Result[protocol.Output, protocol.Fault]{Ok: &protocol.Output{Kind: "run", Data: ended}}
				key := "controller_login_restored"
				if tc.cancelBeforeReply || tc.differentCredential {
					result.Ok, result.Err = nil, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "the original wait changed"}
					key = "controller_login_state_changed"
				}
				outcome = protocol.RequestOutcome{Kind: "completed", Data: result}
				fixture.owner.Refresh()
				fixture.frame()
				if fixture.recovery.settled != (result.Ok != nil) || fixture.recovery.request != nil || fixture.toast != i18n.In("en", key) {
					t.Fatalf("refresh did not report the recorded outcome: %q %+v", fixture.toast, fixture.recovery)
				}
				fixture.owner.retryFailedReads()
				fixture.frame()
				assertControllerLoginRequests(t, fixture.transport, original, 1, inspections+1)
			})
		})
	}
}

func TestControllerLoginRecoveryUsesOnlyOriginalNodeAfterReconnect(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newControllerLoginFixture(t, "en")
		completion := make(chan link.Response)
		fixture.transport.completion = completion
		var inspectionFault *protocol.Fault
		fixture.transport.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind == "inspect_request" && inspectionFault != nil {
				return link.Response{Fault: inspectionFault}, true
			}
			return controllerLoginTestOutcome(request, protocol.RequestOutcome{Kind: "unknown"})
		}
		c := fixture.frame()
		fixture.owner.completeControllerLogin(c, &liveAuthorization{recovery: fixture.recovery}, &fixture.replacement)
		close(completion)
		c = fixture.frame()
		original := fixture.recovery.request
		if original == nil || fixture.recovery.settled {
			t.Fatal("a final unknown outcome let go of the original request")
		}
		assertControllerLoginRequests(t, fixture.transport, original, 1, 1)

		other, otherTransport := controllerLoginTestRuntime(t, fixture.root, protocol.NodeID{9}, fixture.run)
		fixture.root.bindings[other.controller.Target().String()] = other
		other.retryFailedReads()
		fixture.frame()
		assertControllerLoginRequests(t, otherTransport, original, 0, 0)
		assertControllerLoginRequests(t, fixture.transport, original, 1, 1)

		inspectionFault = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "the inspection was refused"}
		fixture.owner.readsConnected = false
		c = fixture.frame()
		if fixture.recovery.request != original || fixture.recovery.settled {
			t.Fatal("an inspection failure was mistaken for a known write failure")
		}
		assertControllerLoginRequests(t, fixture.transport, original, 1, 2)
		actions := &conversationActions{r: fixture.owner, c: c, cv: &viewmodel.Conversation{
			ID: fixture.run.Session.String(), Node: original.Target.String(),
		}}
		actions.ResumeLogin(fixture.run.Turn.String())
		fixture.frame()
		assertControllerLoginRequests(t, fixture.transport, original, 1, 3)

		ended := fixture.run
		ended.Waiting = nil
		fixture.owner.cancel()
		replacement, replacementTransport := controllerLoginTestRuntime(t, fixture.root, original.Target, ended)
		replacementTransport.handle = func(request protocol.Request) (link.Response, bool) {
			return controllerLoginTestOutcome(request, protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{
				Ok: &protocol.Output{Kind: "run", Data: ended},
			}})
		}
		fixture.root.bindings[original.Target.String()] = replacement
		fixture.frame()
		if !fixture.recovery.settled || fixture.toast != i18n.In("en", "controller_login_restored") {
			t.Fatalf("the replacement route lost the original outcome: %q %+v", fixture.toast, fixture.recovery)
		}
		assertControllerLoginRequests(t, replacementTransport, original, 0, 1)
		assertControllerLoginRequests(t, otherTransport, original, 0, 0)
		assertControllerLoginRequests(t, fixture.transport, original, 1, 3)
	})
}

func TestControllerLoginRecoveryExplainsDifferentAccount(t *testing.T) {
	for _, language := range []string{"en", "zh-CN"} {
		t.Run(language, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newControllerLoginFixture(t, language)
				failure := &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "account mismatch", Details: &protocol.FaultDetails{
					Kind: "controller_login", Data: protocol.FaultDetailsControllerLoginPayload{Reason: "account_mismatch"},
				}}
				fixture.transport.handle = func(request protocol.Request) (link.Response, bool) {
					return link.Response{Fault: failure}, request.Command.Kind == "resume_controller_login"
				}
				c := fixture.frame()
				fixture.owner.completeControllerLogin(c, &liveAuthorization{recovery: fixture.recovery}, &fixture.replacement)
				first := fixture.recovery.request
				c = fixture.frame()
				if fixture.toast != i18n.In(language, "controller_login_account_mismatch") || fixture.recovery.settled || fixture.recovery.request != nil {
					t.Fatalf("different account was not explained without accepting recovery: %q %+v", fixture.toast, fixture.recovery)
				}
				assertControllerLoginRequests(t, fixture.transport, first, 1, 0)
				resumed := fixture.run
				resumed.Waiting = nil
				fixture.transport.handle = func(request protocol.Request) (link.Response, bool) {
					return link.Response{Output: protocol.Output{Kind: "run", Data: resumed}}, request.Command.Kind == "resume_controller_login"
				}
				credential := fixture.replacement
				credential.ID = protocol.CredentialID{11}
				attempt := &liveAuthorization{recovery: fixture.recovery}
				fixture.owner.completeControllerLogin(c, attempt, &credential)
				second := fixture.recovery.request
				c = fixture.frame()
				fixture.owner.completeControllerLogin(c, attempt, &credential)
				fixture.frame()
				if second == nil || second.ID == first.ID || !fixture.recovery.settled || len(fixture.transport.requests) != 2 {
					t.Fatalf("a known rejection did not allow exactly one explicit new attempt: %+v", fixture.recovery)
				}
			})
		})
	}
}

type controllerLoginTestTransport struct {
	conversationTestTransport
	history    *conversationTestSubscription
	completion <-chan link.Response
}

func (transport *controllerLoginTestTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "conversation" {
		stream := &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
		stream.updates <- protocol.Update{Kind: "conversation_snapshot", Data: transport.conversation}
		transport.mu.Lock()
		transport.history = stream
		transport.mu.Unlock()
		return stream, nil
	}
	return transport.conversationTestTransport.Subscribe(ctx, topic)
}

func (transport *controllerLoginTestTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind == "resume_controller_login" && transport.completion != nil {
		transport.mu.Lock()
		transport.requests = append(transport.requests, request)
		transport.mu.Unlock()
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: transport.completion}, nil
	}
	return transport.conversationTestTransport.Dispatch(ctx, request)
}

func controllerLoginTestOutcome(request protocol.Request, outcome protocol.RequestOutcome) (link.Response, bool) {
	query, ok := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
	if request.Command.Kind != "inspect_request" || !ok {
		return link.Response{}, false
	}
	return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: outcome}}}, true
}

func assertControllerLoginRequests(t *testing.T, transport *controllerLoginTestTransport, original *protocol.Request, wantWrites, wantInspections int) {
	t.Helper()
	if original == nil {
		t.Fatal("missing original request")
	}
	body, err := json.Marshal(original)
	if err != nil {
		t.Fatal(err)
	}
	wantDigest := fmt.Sprintf("%x", blake3.Sum256(body))
	writes, inspections := 0, 0
	for _, request := range transport.requests {
		if request.Target != original.Target {
			t.Fatalf("recovery sent a request to another Node: %+v", request)
		}
		switch request.Command.Kind {
		case "resume_controller_login":
			writes++
			if request.ID != original.ID {
				t.Fatalf("restore dispatched a different write: %+v", request)
			}
		case "inspect_request":
			inspections++
			query, ok := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
			if !ok || query.ID != original.ID || query.Digest != wantDigest || request.Command.Durable() {
				t.Fatalf("inspection was not a read of the frozen request: %+v", request)
			}
		default:
			t.Fatalf("recovery sent an unrelated command: %s", request.Command.Kind)
		}
	}
	if writes != wantWrites || inspections != wantInspections {
		t.Fatalf("got %d restore writes and %d inspections, want %d and %d", writes, inspections, wantWrites, wantInspections)
	}
}

type controllerLoginFixture struct {
	root, owner *Runtime
	transport   *controllerLoginTestTransport
	run         protocol.ConversationRun
	recovery    *controllerLoginRecovery
	replacement protocol.CredentialRef
	frame       func() *ui.Context
	toast       string
}

func newControllerLoginFixture(t *testing.T, language string) *controllerLoginFixture {
	t.Helper()
	target := protocol.NodeID{2}
	run := protocol.DefaultConversationRun()
	run.Session, run.Turn, run.Status = protocol.SessionID{3}, protocol.TurnID{4}, protocol.ConversationStatusRunning
	run.Sequence, run.Revision, run.Kind = 1, 1, protocol.ConversationRunKindTask
	run.Waiting = &protocol.ConversationRunWaiting{
		Kind: "controller_login", Attempt: protocol.RequestID{5}, Provider: protocol.ProviderID{6},
		Credential: protocol.CredentialRef{Node: target, ID: protocol.CredentialID{7}},
	}
	root, _ := controllerLoginTestRuntime(t, nil, protocol.NodeID{1}, run)
	owner, transport := controllerLoginTestRuntime(t, root, target, run)
	root.bindings = map[string]*Runtime{target.String(): owner}
	recovery := &controllerLoginRecovery{input: protocol.CommandResumeControllerLoginPayload{
		Session: run.Session, Turn: run.Turn, Waiting: run.Waiting.Attempt, Previous: run.Waiting.Credential,
	}}
	key := target.String() + "/" + run.Turn.String() + "/" + run.Waiting.Attempt.String()
	root.authorizations = &authorizationState{loginRecoveries: map[string]*controllerLoginRecovery{key: recovery}}
	fixture := &controllerLoginFixture{root: root, owner: owner, transport: transport, run: run, recovery: recovery,
		replacement: protocol.CredentialRef{Node: target, ID: protocol.CredentialID{8}}}
	var view *ui.Context
	tester := ui.NewTesterWith(func(c *ui.Context) {
		view = c
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queue {
			apply(c)
		}
		root.resumeReadsOnReconnect()
		for _, bound := range root.bindings {
			bound.resumeReadsOnReconnect()
		}
		ui.ToastViewportBase(c, func(_ ui.Element, toasts []ui.Toast) {
			if len(toasts) > 0 {
				fixture.toast = toasts[len(toasts)-1].Title
			}
		})
	}, 1, 1, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale(language)
	fixture.frame = func() *ui.Context {
		for range 3 {
			synctest.Wait()
			tester.Frame()
		}
		synctest.Wait()
		return view
	}
	return fixture
}

func controllerLoginTestRuntime(t *testing.T, root *Runtime, node protocol.NodeID, run protocol.ConversationRun) (*Runtime, *controllerLoginTestTransport) {
	t.Helper()
	snapshot, session := protocol.DefaultSnapshot(), protocol.DefaultSession()
	session.ID, session.Revision = run.Session, 1
	snapshot.Node, snapshot.Sessions = node, []protocol.Session{session}
	history := protocol.DefaultConversationSnapshot()
	history.Node, history.Sequence = node, 1
	history.Page.Session, history.Page.Revision, history.Page.Runs = run.Session, 1, []protocol.ConversationRun{run}
	transport := &controllerLoginTestTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot, conversation: history}}
	controller := client.New(transport)
	ctx, cancel := context.WithCancel(t.Context())
	t.Cleanup(func() {
		cancel()
		_ = controller.Close()
	})
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	return &Runtime{parent: root, controller: controller, ctx: ctx, cancel: cancel, readsConnected: controller.Connected(), busy: map[string]bool{}}, transport
}
