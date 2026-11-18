package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
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

func TestControllerLoginRecoveryRetainsExactRequestAfterLostCompletion(t *testing.T) {
	source, target := protocol.NodeID{1}, protocol.NodeID{2}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = target
	transport := &conversationTestTransport{snapshot: snapshot}
	var requests []protocol.RequestID
	completed := false
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "resume_controller_login":
			requests = append(requests, request.ID)
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "completion was lost"}}, true
		case "inspect_request":
			input, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if completed {
				output := protocol.Output{Kind: "run", Data: protocol.DefaultConversationRun()}
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}}
			}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: input.ID, Outcome: outcome}}}, true
		}
		t.Errorf("task recovery sent an unrelated command: %s", request.Command.Kind)
		return link.Response{}, false
	}
	sourceSnapshot := protocol.DefaultSnapshot()
	sourceSnapshot.Node = source
	root := &Runtime{ctx: t.Context(), controller: client.New(&conversationTestTransport{snapshot: sourceSnapshot})}
	executor := &Runtime{ctx: t.Context(), controller: client.New(transport), parent: root}
	recovery := &controllerLoginRecovery{executor: executor, input: protocol.CommandResumeControllerLoginPayload{
		Session: protocol.SessionID{3}, Turn: protocol.TurnID{4}, Waiting: protocol.RequestID{5},
		Previous: protocol.CredentialRef{Node: target, ID: protocol.CredentialID{6}},
	}}
	attempt := &liveAuthorization{recovery: recovery}
	replacement := protocol.CredentialRef{Node: target, ID: protocol.CredentialID{7}}
	var context *ui.Context
	tester := ui.NewTester(func(c *ui.Context) {
		context = c
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, post := range queue {
			post(c)
		}
		ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
	}, 800, 500)
	tester.SetLocale("en")
	tester.Frame()
	executor.completeControllerLogin(context, attempt, &replacement)
	executor.completeControllerLogin(context, attempt, &replacement)
	root.workers.Wait()
	tester.Frame()
	if recovery.request == nil || recovery.settled || len(requests) != 1 {
		t.Fatalf("uncertain recovery was lost or duplicated: %+v requests=%v", recovery, requests)
	}
	input, ok := detail[protocol.CommandResumeControllerLoginPayload](recovery.request.Command.Data)
	if !ok || input.Session != recovery.input.Session || input.Turn != recovery.input.Turn || input.Waiting != recovery.input.Waiting || input.Previous != recovery.input.Previous || input.Replacement != replacement || recovery.request.Target != target {
		t.Fatalf("recovery did not retain the exact task and credentials: %+v", recovery.request)
	}
	completed = true
	root.restoreControllerLogin(recovery)
	root.workers.Wait()
	tester.Frame()
	if !recovery.settled || len(requests) != 2 || requests[0] != requests[1] {
		t.Fatalf("completion recovery changed request identity: settled=%v requests=%v", recovery.settled, requests)
	}
	root.restoreControllerLogin(recovery)
	executor.completeControllerLogin(context, attempt, &replacement)
	root.workers.Wait()
	if len(requests) != 2 {
		t.Fatal("duplicate completion started a second recovery")
	}
}

func TestControllerLoginRecoveryExplainsDifferentAccount(t *testing.T) {
	for _, language := range []string{"en", "zh-CN"} {
		t.Run(language, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{2}
			transport := &conversationTestTransport{snapshot: snapshot}
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "account mismatch", Details: &protocol.FaultDetails{
					Kind: "controller_login", Data: protocol.FaultDetailsControllerLoginPayload{Reason: "account_mismatch"},
				}}}, true
			}
			root := &Runtime{ctx: t.Context()}
			executor := &Runtime{controller: client.New(transport)}
			request := executor.controller.Prepare(protocol.Command{Kind: "resume_controller_login", Data: protocol.CommandResumeControllerLoginPayload{}})
			recovery := &controllerLoginRecovery{executor: executor, request: &request}
			var shown string
			tester := ui.NewTester(func(c *ui.Context) {
				root.mu.Lock()
				queue := root.queue
				root.queue = nil
				root.mu.Unlock()
				for _, post := range queue {
					post(c)
				}
				ui.ToastViewportBase(c, func(_ ui.Element, toasts []ui.Toast) {
					if len(toasts) > 0 {
						shown = toasts[len(toasts)-1].Title
					}
				})
			}, 800, 500)
			tester.SetLocale(language)
			root.restoreControllerLogin(recovery)
			root.workers.Wait()
			tester.Frame()
			if shown != i18n.In(language, "controller_login_account_mismatch") || recovery.settled || recovery.request != nil {
				t.Fatalf("different account was not explained without accepting recovery: %q %+v", shown, recovery)
			}
		})
	}
}
