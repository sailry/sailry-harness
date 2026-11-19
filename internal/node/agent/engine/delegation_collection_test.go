package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestExplicitCollectionDoesNotRedeliverForegroundResult(t *testing.T) {
	const answer = "Unique completed child response"
	f, parent := delegationFixture(t, &scriptedModel{scripts: []script{response(text(answer), finish())}})
	spawn := saveDelegationCall(t, f, parent, "foreground", false)
	result := f.r.spawnChild(parent, spawn, spawn.Arguments)
	if result.Failure != nil {
		t.Fatal(result.Failure)
	}
	f.r.workers.Wait()
	child := parent.foreground[0]
	record := canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, responseBlock(0, spawn, result)), f.r.clock.Now())
	record.Provenance.Author = "tool"
	if err := f.r.save(parent.ctx, parent.inv.Session.ID, record); err != nil {
		t.Fatal(err)
	}
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := store.ReleaseChildren(f.ctx, db, parent.inv.Run.Turn, []protocol.TurnID{child.turn})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	child.released = true
	f.start()
	f.waitRun(child.turn, protocol.ConversationStatusCompleted)
	args, _ := json.Marshal(map[string]any{"session": child.session})
	wait := canonical.ToolRequest{ID: "collect", Name: "wait_agent", Arguments: args}
	record = canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &wait}), f.r.clock.Now())
	if err := f.r.save(parent.ctx, parent.inv.Session.ID, record); err != nil {
		t.Fatal(err)
	}
	parent.timeline = newTimeline(f.r.clock)
	if err := f.r.captureTools(parent); err != nil {
		t.Fatal(err)
	}
	if err := f.r.runTools(parent, f.state(), []canonical.ToolRequest{wait}); err != nil {
		t.Fatal(err)
	}
	if delivered, err := f.r.foregroundStep(parent, true); err != nil || delivered {
		t.Fatalf("collected foreground result was delivered again: %t, %v", delivered, err)
	}
	if !f.r.cancelForeground(parent) {
		t.Fatal("foreground settlement did not finish")
	}
	body, err := json.Marshal(f.state().Records)
	if err != nil || strings.Count(string(body), answer) != 1 || strings.Contains(string(body), `"note":"subagents_cancelled"`) {
		t.Fatalf("collected result was lost, duplicated or cancelled: %s, %v", body, err)
	}
}

func TestForegroundResultsJoinRepeatedGuidance(t *testing.T) {
	const (
		firstGuidance  = "Continue while the first check runs."
		secondGuidance = "Verify and summarize the first check."
		firstAnswer    = "First child verified result"
		secondAnswer   = "Second child verified result"
	)
	parentRequests, childRequests := make(chan fantasy.Call, 3), make(chan fantasy.Call, 2)
	finishSecondStep, finishThirdStep := make(chan struct{}), make(chan struct{})
	finishFirstChild, finishSecondChild := make(chan struct{}), make(chan struct{})
	observe := func(requests chan<- fantasy.Call, release <-chan struct{}, reply script) script {
		return func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			requests <- request
			if release != nil {
				select {
				case <-ctx.Done():
					return
				case <-release:
				}
			}
			reply(ctx, request, yield)
		}
	}
	parent := &scriptedModel{scripts: []script{
		response(call("first", "spawn_agent", `{"task":"Run the first check"}`), finish()),
		observe(parentRequests, finishSecondStep, response(call("second", "spawn_agent", `{"task":"Run the second check"}`), finish())),
		observe(parentRequests, finishThirdStep, response(text("First check summarized"), finish())),
		observe(parentRequests, nil, response(text("Both checks summarized"), finish())),
	}}
	child := &scriptedModel{scripts: []script{
		observe(childRequests, finishFirstChild, response(text(firstAnswer), finish())),
		observe(childRequests, finishSecondChild, response(text(secondAnswer), finish())),
	}}
	f := newFixture(t, parent)
	config := f.session.Config
	config.Mode = protocol.WorkModeFlagship
	input := protocol.DefaultConversationStart()
	input.Config, input.Execution, input.Message.Text = &config, protocol.ConversationExecutionModel, "Run two checks and summarize the results"
	turn := f.command("start_session", &input).Data.(protocol.QueuedTurn)
	f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		if binding.Session == turn.Session {
			return parent, nil
		}
		return child, nil
	}
	f.start()
	await := func(requests <-chan fantasy.Call, step string) fantasy.Call {
		t.Helper()
		select {
		case request := <-requests:
			return request
		case <-f.ctx.Done():
			t.Fatalf("%s did not start: %v", step, f.ctx.Err())
			return fantasy.Call{}
		}
	}
	guide := func(text string) protocol.TurnID {
		return f.command("guide_turn", &protocol.CommandGuideTurnPayload{
			Recipient: turn.ID, ExpectedRevision: turn.Revision, Message: protocol.ConversationInput{Text: text},
		}).Data.(protocol.QueuedTurn).ID
	}

	await(childRequests, "first child")
	turns := f.childTurns()
	if len(turns) != 1 {
		t.Fatalf("started with %d children, want one", len(turns))
	}
	first := guide(firstGuidance)
	if request := promptText(await(parentRequests, "parent after first guidance")); !strings.Contains(request, firstGuidance) || strings.Contains(request, firstAnswer) {
		t.Fatalf("first guided request should carry guidance but not the pending result: %s", request)
	}

	// Settle the first child while the parent is still in the guided model step.
	second := guide(secondGuidance)
	close(finishFirstChild)
	f.waitRun(turns[0], protocol.ConversationStatusCompleted)
	close(finishSecondStep)
	if request := promptText(await(parentRequests, "parent after second guidance")); !strings.Contains(request, secondGuidance) || strings.Count(request, firstAnswer) != 1 {
		t.Fatalf("second guided request should carry guidance and the completed result exactly once: %s", request)
	}
	// The new spawn must have been released before this guided request returns.
	await(childRequests, "second child during the guided step")
	close(finishSecondChild)
	close(finishThirdStep)
	if request := promptText(await(parentRequests, "parent after the second child finishes")); strings.Count(request, firstAnswer) != 1 || strings.Count(request, secondAnswer) != 1 {
		t.Fatalf("next request should carry both results without redelivery: %s", request)
	}
	f.waitRun(turn.ID, protocol.ConversationStatusCompleted)
	for _, id := range []protocol.TurnID{first, second} {
		if run := f.waitRun(id, protocol.ConversationStatusCompleted); run.Joined == nil || *run.Joined != turn.ID {
			t.Fatalf("guidance %s did not join the original turn", id)
		}
	}
	if count := parent.count(); count != 4 {
		t.Fatalf("parent requests = %d, want four", count)
	}
}

func TestForegroundChildrenOverlapIndependentParentWork(t *testing.T) {
	const childAnswer = "Result from the overlapping child"
	childStarted := make(chan struct{}, 2)
	finishChildren := make(chan struct{})
	localDone, parentReady := make(chan struct{}), make(chan struct{})
	integration := make(chan fantasy.Call, 1)
	local := objectTool("local_check", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		close(localDone)
		return ToolResult{Value: map[string]bool{"checked": true}}
	})
	parent := &scriptedModel{scripts: []script{
		response(
			call("first", "spawn_agent", `{"task":"Check the first independent area","overlap":true}`),
			call("second", "spawn_agent", `{"task":"Check the second independent area","overlap":true}`),
			finish(),
		),
		response(call("local", "local_check", `{}`), finish()),
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			close(parentReady)
			response(text("Completed the local check"), finish())(ctx, request, yield)
		},
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			integration <- request
			response(text("Integrated the local check and both child results"), finish())(ctx, request, yield)
		},
	}}
	check := func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		childStarted <- struct{}{}
		select {
		case <-ctx.Done():
		case <-finishChildren:
			response(text(childAnswer), finish())(ctx, request, yield)
		}
	}
	children := &scriptedModel{scripts: []script{check, check}}
	f := newFixture(t, parent, local)
	config := f.session.Config
	config.Mode = protocol.WorkModeFlagship
	input := protocol.DefaultConversationStart()
	input.Config, input.Execution, input.Message.Text = &config, protocol.ConversationExecutionModel, "Run two delegated checks and one local check"
	turn := f.command("start_session", &input).Data.(protocol.QueuedTurn)
	f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		if binding.Session == turn.Session {
			return parent, nil
		}
		return children, nil
	}
	f.start()
	wait := func(done <-chan struct{}, what string) {
		t.Helper()
		select {
		case <-done:
		case <-f.ctx.Done():
			t.Fatalf("%s did not run while both children were pending", what)
		}
	}
	wait(childStarted, "the first child")
	wait(childStarted, "the second child")
	wait(localDone, "the parent's local tool")
	wait(parentReady, "the parent's next model step")
	close(finishChildren)
	f.waitRun(turn.ID, protocol.ConversationStatusCompleted)
	select {
	case request := <-integration:
		if got := strings.Count(promptText(request), childAnswer); got != 2 {
			t.Fatalf("integration received %d children, want two", got)
		}
	default:
		t.Fatal("the parent finished without integrating its child results")
	}
	if count := parent.count(); count != 4 {
		t.Fatalf("parent made %d requests, want four", count)
	}
}

func TestWaitAgentReturnsPendingBeforeForegroundAdmissionIsReleased(t *testing.T) {
	f, parent := delegationFixture(t, &scriptedModel{})
	spawn := saveDelegationCall(t, f, parent, "foreground", false)
	if result := f.r.spawnChild(parent, spawn, spawn.Arguments); result.Failure != nil {
		t.Fatal(result.Failure)
	}
	f.r.workers.Wait()
	args, err := json.Marshal(map[string]any{"session": parent.foreground[0].session})
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan ToolResult, 1)
	go func() {
		done <- f.r.waitChild(parent, canonical.ToolRequest{Arguments: args})
	}()
	select {
	case result := <-done:
		value, ok := result.Value.(map[string]any)
		if !ok || result.Failure != nil || value["pending"] != true || value["status"] != protocol.ConversationStatusQueued {
			t.Fatalf("wait returned %+v for an unreleased child", result)
		}
	case <-f.ctx.Done():
		t.Fatal("wait blocked the batch that needed to release its foreground child")
	}
}
