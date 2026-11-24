package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestExplicitCollectionDoesNotRedeliverForegroundResult(t *testing.T) {
	t.Parallel()
	const answer = "Unique completed child response"
	f, parent := delegationFixture(t, &scriptedModel{scripts: []script{response(text(answer), finish())}})
	spawn := saveDelegationCall(t, f, parent, "foreground", false)
	result := f.r.spawnChild(parent, spawn, spawn.Arguments)
	if result.Failure != nil {
		t.Fatal(result.Failure)
	}
	f.r.workers.Wait()
	child := parent.foreground[0]
	stopParent := child.stopParent
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
	if len(parent.foreground) != 0 {
		t.Fatal("parent retained the collected foreground child")
	}
	if stopParent() {
		t.Fatal("the collected child still had a parent cancellation callback")
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

func TestPluginStopCollectsOverlappingChildWithoutAnotherModelCall(t *testing.T) {
	t.Parallel()
	const answer = "Collected independent result"
	childStarted, stopInference := make(chan struct{}), make(chan struct{})
	release := make(chan struct{})
	parent := &scriptedModel{scripts: []script{
		response(call("spawn", "spawn_agent", `{"task":"Complete the independent check","overlap":true}`), finish()),
	}}
	child := &scriptedModel{scripts: []script{func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		close(childStarted)
		select {
		case <-ctx.Done():
			return
		case <-release:
			response(text(answer), finish())(ctx, request, yield)
		}
	}}}
	hook := objectTool("hook", true, true, func(context.Context, canonical.ToolRequest) ToolResult { return ToolResult{} })
	hook.Binding = &store.ApprovalBinding{Owner: protocol.PluginReference{Name: "hook-fixture"}}
	hook.BeforeModel = func(_ context.Context, records []canonical.Record, _ ToolResultReader) (bool, error) {
		for _, record := range records {
			if message := record.Message(); message != nil {
				for _, block := range message.Content {
					if result, ok := block.Data.(*canonical.ToolResponse); ok && result.ID == "spawn" {
						close(stopInference)
						return true, nil
					}
				}
			}
		}
		return false, nil
	}
	f := newFixture(t, parent, hook)
	config := f.session.Config
	config.Mode = protocol.WorkModeFlagship
	input := protocol.DefaultConversationStart()
	input.Config, input.Execution, input.Message.Text = &config, protocol.ConversationExecutionModel, "Run one independent check"
	turn := f.command("start_session", &input).Data.(protocol.QueuedTurn)
	f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		if binding.Session == turn.Session {
			return parent, nil
		}
		return child, nil
	}
	f.start()
	awaitBackgroundTest(t, f.ctx, childStarted)
	awaitBackgroundTest(t, f.ctx, stopInference)
	close(release)
	f.waitRun(f.childTurns()[0], protocol.ConversationStatusCompleted)
	f.waitRun(turn.ID, protocol.ConversationStatusCompleted)
	state, err := f.r.load(f.ctx, turn.Session)
	if err != nil {
		t.Fatal(err)
	}
	body, err := json.Marshal(state.Records)
	if err != nil || strings.Count(string(body), answer) != 1 || parent.count() != 1 {
		t.Fatalf("plugin stop lost the result or resumed inference: calls=%d, result count=%d, err=%v", parent.count(), strings.Count(string(body), answer), err)
	}
}

func TestPluginStopGuidanceDetachesOverlappingChild(t *testing.T) {
	t.Parallel()
	childStarted, stopInference := make(chan struct{}), make(chan struct{})
	release := make(chan struct{})
	parent := &scriptedModel{scripts: []script{
		response(call("spawn", "spawn_agent", `{"task":"Complete the independent check","overlap":true}`), finish()),
	}}
	child := &scriptedModel{scripts: []script{func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		close(childStarted)
		select {
		case <-ctx.Done():
		case <-release:
			response(text("Detached result"), finish())(ctx, request, yield)
		}
	}}}
	var stopped sync.Once
	hook := objectTool("hook", true, true, func(context.Context, canonical.ToolRequest) ToolResult { return ToolResult{} })
	hook.Binding = &store.ApprovalBinding{Owner: protocol.PluginReference{Name: "hook-fixture"}}
	hook.BeforeModel = func(_ context.Context, records []canonical.Record, _ ToolResultReader) (bool, error) {
		for _, record := range records {
			if message := record.Message(); message != nil {
				for _, block := range message.Content {
					if result, ok := block.Data.(*canonical.ToolResponse); ok && result.ID == "spawn" {
						stopped.Do(func() { close(stopInference) })
						return true, nil
					}
				}
			}
		}
		return false, nil
	}
	f := newFixture(t, parent, hook)
	config := f.session.Config
	config.Mode = protocol.WorkModeFlagship
	input := protocol.DefaultConversationStart()
	input.Config, input.Execution, input.Message.Text = &config, protocol.ConversationExecutionModel, "Run one independent check"
	turn := f.command("start_session", &input).Data.(protocol.QueuedTurn)
	f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		if binding.Session == turn.Session {
			return parent, nil
		}
		return child, nil
	}
	f.start()
	awaitBackgroundTest(t, f.ctx, childStarted)
	awaitBackgroundTest(t, f.ctx, stopInference)
	var run protocol.ConversationRun
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		run, err = store.ReadAgentRun(f.ctx, db, turn.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	f.command("guide_turn", &protocol.CommandGuideTurnPayload{Recipient: turn.ID, ExpectedRevision: run.Revision, Message: protocol.ConversationInput{Text: "Change of plan"}})
	// The guidance ends the wait: the turn finishes while its child runs on.
	f.waitRun(turn.ID, protocol.ConversationStatusCompleted)
	childTurn := f.childTurns()[0]
	if f.readQuery("SELECT count(*) FROM agent_runs WHERE turn='"+childTurn.String()+"' AND json_extract(body,'$.status')='running'") != 1 {
		t.Fatal("the detached child did not keep running")
	}
	if f.readQuery("SELECT count(*) FROM agent_background WHERE kind='agent' AND json_extract(body,'$.follow_up')=1") != 1 {
		t.Fatal("the detached child does not follow up like a background child")
	}
	close(release)
	f.waitRun(childTurn, protocol.ConversationStatusCompleted)
}

func TestForegroundResultsJoinRepeatedGuidance(t *testing.T) {
	t.Parallel()
	const (
		firstGuidance  = "Continue while the first check runs."
		secondGuidance = "Verify and summarize the first check."
		firstAnswer    = "First child verified result"
		secondAnswer   = "Second child verified result"
	)
	parentRequests, childRequests := make(chan fantasy.Call, 4), make(chan fantasy.Call, 2)
	finishSecondStep, finishThirdStep, finishFourthStep := make(chan struct{}), make(chan struct{}), make(chan struct{})
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
		response(call("first", "spawn_agent", `{"task":"Run the first check","overlap":false}`), finish()),
		observe(parentRequests, finishSecondStep, response(call("second", "spawn_agent", `{"task":"Run the second check"}`), finish())),
		observe(parentRequests, finishThirdStep, response(text("Interrupted before second guidance"), finish())),
		observe(parentRequests, finishFourthStep, response(text("First check summarized"), finish())),
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

	// Let the first guided step spawn the second child, settle the first
	// child so the parent resumes, and steer the active stream with the second guidance.
	close(finishSecondStep)
	await(childRequests, "second child during the guided step")
	close(finishFirstChild)
	f.waitRun(turns[0], protocol.ConversationStatusCompleted)
	await(parentRequests, "parent after first child finishes")
	second := guide(secondGuidance)
	if request := promptText(await(parentRequests, "parent after second guidance")); !strings.Contains(request, secondGuidance) || strings.Count(request, firstAnswer) != 1 {
		t.Fatalf("second guided request should carry guidance and the completed result exactly once: %s", request)
	}
	close(finishSecondChild)
	close(finishFourthStep)
	if request := promptText(await(parentRequests, "parent after the second child finishes")); strings.Count(request, firstAnswer) != 1 || strings.Count(request, secondAnswer) != 1 {
		t.Fatalf("next request should carry both results without redelivery: %s", request)
	}
	f.waitRun(turn.ID, protocol.ConversationStatusCompleted)
	for _, id := range []protocol.TurnID{first, second} {
		if run := f.waitRun(id, protocol.ConversationStatusCompleted); run.Joined == nil || *run.Joined != turn.ID {
			t.Fatalf("guidance %s did not join the original turn", id)
		}
	}
	if count := parent.count(); count != 5 {
		t.Fatalf("parent requests = %d, want five", count)
	}
}

func TestCollaborateForegroundChildrenOverlapIndependentParentWorkByDefault(t *testing.T) {
	t.Parallel()
	const (
		slowTask   = "Check the slow independent area"
		fastTask   = "Check the fast independent area"
		slowAnswer = "Result from the slow child"
		fastAnswer = "Result from the fast child"
	)
	childStarted := make(chan struct{}, 2)
	finishSlow, finishFast := make(chan struct{}), make(chan struct{})
	localDone, parentReady := make(chan struct{}), make(chan struct{})
	integration := make(chan fantasy.Call, 2)
	local := objectTool("local_check", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		close(localDone)
		return ToolResult{Value: map[string]bool{"checked": true}}
	})
	parent := &scriptedModel{scripts: []script{
		response(
			call("first", "spawn_agent", `{"task":"`+slowTask+`"}`),
			call("second", "spawn_agent", `{"task":"`+fastTask+`"}`),
			finish(),
		),
		response(call("local", "local_check", `{}`), finish()),
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			close(parentReady)
			response(text("Completed the local check"), finish())(ctx, request, yield)
		},
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			integration <- request
			response(text("Integrated the fast check"), finish())(ctx, request, yield)
		},
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			integration <- request
			response(text("Integrated the local check and both child results"), finish())(ctx, request, yield)
		},
	}}
	check := func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		release, answer := finishSlow, slowAnswer
		if strings.Contains(promptText(request), fastTask) {
			release, answer = finishFast, fastAnswer
		}
		childStarted <- struct{}{}
		select {
		case <-ctx.Done():
		case <-release:
			response(text(answer), finish())(ctx, request, yield)
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
	close(finishFast)
	select {
	case request := <-integration:
		if prompt := promptText(request); strings.Count(prompt, fastAnswer) != 1 || strings.Contains(prompt, slowAnswer) {
			t.Fatal("the parent did not receive the fast result independently")
		}
	case <-f.ctx.Done():
		t.Fatal("the parent waited for the slow child before processing the fast result")
	}
	close(finishSlow)
	f.waitRun(turn.ID, protocol.ConversationStatusCompleted)
	select {
	case request := <-integration:
		if prompt := promptText(request); strings.Count(prompt, fastAnswer) != 1 || strings.Count(prompt, slowAnswer) != 1 {
			t.Fatal("final integration did not receive both results exactly once")
		}
	default:
		t.Fatal("the parent finished without integrating its child results")
	}
	if count := parent.count(); count != 5 {
		t.Fatalf("parent made %d requests, want five", count)
	}
}

func TestWaitAgentReturnsPendingBeforeForegroundAdmissionIsReleased(t *testing.T) {
	t.Parallel()
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

func TestParentModelFailureDetachesOverlappingChild(t *testing.T) {
	t.Parallel()
	const childAnswer = "Result from the overlapping child"
	childStarted, finishChild := make(chan struct{}), make(chan struct{})
	integration := make(chan fantasy.Call, 1)
	step := func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		prompt := promptText(request)
		switch {
		case strings.Contains(prompt, childAnswer):
			integration <- request
			response(text("Integrated the child result"), finish())(ctx, request, yield)
		case !strings.Contains(prompt, `"admitted"`):
			response(call("child", "spawn_agent", `{"task":"Check the independent area"}`), finish())(ctx, request, yield)
		default:
			<-childStarted
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.InvalidResponse, Message: "fixture failure"}})
		}
	}
	parent := &scriptedModel{scripts: []script{step, step, step, step, step, step}}
	var started sync.Once
	children := &scriptedModel{scripts: []script{func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		started.Do(func() { close(childStarted) })
		select {
		case <-ctx.Done():
		case <-finishChild:
			response(text(childAnswer), finish())(ctx, request, yield)
		}
	}}}
	f := newFixture(t, parent)
	config := f.session.Config
	config.Mode = protocol.WorkModeFlagship
	input := protocol.DefaultConversationStart()
	input.Config, input.Execution, input.Message.Text = &config, protocol.ConversationExecutionModel, "Delegate one check"
	turn := f.command("start_session", &input).Data.(protocol.QueuedTurn)
	f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		if binding.Session == turn.Session {
			return parent, nil
		}
		return children, nil
	}
	f.start()
	f.waitRun(turn.ID, protocol.ConversationStatusFailed)
	admitted := f.childTurns()
	if len(admitted) != 1 {
		t.Fatalf("admitted children=%d", len(admitted))
	}
	var child protocol.ConversationRun
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		child, err = store.ReadAgentRun(f.ctx, db, admitted[0])
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if child.Status != protocol.ConversationStatusRunning {
		t.Fatalf("the parent's model failure ended its overlapping child: %s", child.Status)
	}
	if f.readQuery("SELECT count(*) FROM agent_events WHERE json_extract(body,'$.metadata.runtime.notes.cancelled_subagents') IS NOT NULL") != 0 {
		t.Fatal("the failed parent reported its overlapping child as cancelled")
	}
	close(finishChild)
	f.waitRun(admitted[0], protocol.ConversationStatusCompleted)
	select {
	case request := <-integration:
		if strings.Count(promptText(request), childAnswer) != 1 {
			t.Fatal("the next turn did not receive the child result exactly once")
		}
	case <-f.ctx.Done():
		t.Fatal("the detached child result was not delivered to a later turn")
	}
	// Retrying the failed turn, as the desktop does, resends its input. The
	// retry sees the child's delivered result instead of delegating again.
	runs := fmt.Sprintf("SELECT count(*) FROM agent_runs WHERE json_extract(body,'$.session')='%s'", turn.Session)
	for f.readQuery(runs) < 2 || f.readQuery(runs+" AND json_extract(body,'$.status') IN ('queued','running')") != 0 {
		select {
		case <-f.ctx.Done():
			t.Fatal("the delivery turn did not settle")
		case <-time.After(5 * time.Millisecond):
		}
	}
	var current protocol.Session
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		current, err = store.ReadSession(f.ctx, db, turn.Session)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	retry := protocol.DefaultConversationInput()
	retry.Text = input.Message.Text
	next := f.command("submit_turn", &protocol.CommandSubmitTurnPayload{Session: turn.Session, ExpectedRevision: current.Revision, Message: retry}).Data.(protocol.QueuedTurn).ID
	f.waitRun(next, protocol.ConversationStatusCompleted)
	select {
	case request := <-integration:
		if !strings.Contains(promptText(request), childAnswer) {
			t.Fatal("the retried turn did not see the child result")
		}
	case <-f.ctx.Done():
		t.Fatal("the retried turn made no model request")
	}
	if admitted := f.childTurns(); len(admitted) != 1 {
		t.Fatalf("the retried turn delegated again: children=%d", len(admitted))
	}
}
