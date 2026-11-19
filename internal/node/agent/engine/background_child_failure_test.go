package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const fixtureChildFailureMarker = "Fixture child stopped by a provider protocol error"
const fixtureChildFailure = fixtureChildFailureMarker + " \"quoted\"\n</runtime>"
const fixtureChildTitle = "Background fixture"

func TestBackgroundChildFailureAtTurnBoundary(t *testing.T) {
	for _, boundary := range []string{"tool", "stream", "approval", "wait_agent", "stop"} {
		t.Run(boundary, func(t *testing.T) {
			entered, finishStep, failChild := make(chan struct{}), make(chan struct{}), make(chan struct{})
			waitArguments := make(chan string, 1)
			tool := objectTool("fixture_step", true, false, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
				if boundary != "approval" {
					close(entered)
					select {
					case <-finishStep:
					case <-ctx.Done():
						return toolFault(ctx.Err())
					}
				}
				return ToolResult{Value: "The parent tool finished"}
			})
			tool.Approval = boundary == "approval"
			step := response(call("step", "fixture_step", `{}`), finish())
			switch boundary {
			case "stream":
				step = func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
					if !yield(text("The parent's initial reply")) {
						return
					}
					close(entered)
					select {
					case <-finishStep:
						yield(finish())
					case <-ctx.Done():
					}
				}
			case "wait_agent":
				step = func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
					close(entered)
					select {
					case args := <-waitArguments:
						response(call("collect", "wait_agent", args), finish())(ctx, request, yield)
					case <-ctx.Done():
					}
				}
			}
			main := &scriptedModel{scripts: []script{backgroundFailureSpawn(), step, response(text("Parent finished handling the task"), finish())}}
			child := &scriptedModel{scripts: []script{scriptedChildFailure(failChild)}}
			f := backgroundFailureFixture(t, main, child, tool)
			f.start()
			const original = "Inspect the state without repeating effects."
			parent := f.submit(original)
			var approval protocol.ConversationApproval
			if boundary == "approval" {
				approval = f.waitingApproval()
			} else {
				awaitBackgroundTest(t, f.ctx, entered)
			}
			childID := onlyBackgroundTestChild(t, f)
			if boundary == "wait_agent" {
				running := f.waitRun(childID, protocol.ConversationStatusRunning)
				waitArguments <- fmt.Sprintf(`{"session":%q,"turn":%q}`, running.Session.String(), childID.String())
				if wait := awaitBackgroundTest(t, f.ctx, f.r.clock.(*manualClock).waits); wait.duration != 150*time.Second {
					t.Fatalf("wait_agent did not actually wait: %v", wait.duration)
				}
			}
			close(failChild)
			failed := f.waitRun(childID, protocol.ConversationStatusFailed)
			if boundary != "wait_agent" && (len(backgroundFailureReports(f.state())) != 0 || main.count() != 2) {
				t.Fatal("background failure interrupted the parent's active step")
			}
			switch boundary {
			case "stop":
				f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: parent})
				f.waitRun(parent, protocol.ConversationStatusCancelled)
				if len(backgroundFailureReports(f.state())) != 0 || main.count() != 2 || !backgroundFailureHistory(t, f).Page.Queue.Paused {
					t.Fatal("Stop lost to failure delivery")
				}
			case "wait_agent":
				f.waitRun(parent, protocol.ConversationStatusCompleted)
				if main.count() != 3 || len(backgroundFailureReports(f.state())) != 0 || !strings.Contains(promptText(main.calls[2]), fixtureChildFailureMarker) {
					t.Fatal("the explicit collection was lost or automatically repeated")
				}
				assertBackgroundFailureState(t, f, childID, protocol.ConversationBackgroundStateConsumed, nil)
			default:
				if boundary == "approval" {
					f.command("resolve_approval", &protocol.CommandResolveApprovalPayload{Session: f.session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}})
				} else {
					close(finishStep)
				}
				f.waitRun(parent, protocol.ConversationStatusCompleted)
				requireBackgroundFailureReport(t, f, parent, failed)
				if main.count() != 3 {
					t.Fatal("the same parent did not receive the failure exactly once at its next request")
				}
				assertFailureModelEvidence(t, main.calls[2])
			}
			assertBackgroundTestInput(t, f.state(), parent, original)
			if child.count() != 1 || onlyBackgroundTestChild(t, f) != childID || f.readQuery("SELECT count(*) FROM agent_runs WHERE json_extract(body,'$.notice')=1") != 0 {
				t.Fatal("the busy parent retried the child or spawned a notice turn")
			}
		})
	}
}

func TestBackgroundChildFailureAfterParentEnds(t *testing.T) {
	for _, scenario := range []string{"automatic", "legacy failure before queued input", "stopped child"} {
		t.Run(scenario, func(t *testing.T) {
			failChild := make(chan struct{})
			nextRequest := make(chan fantasy.Call, 1)
			main := &scriptedModel{scripts: []script{backgroundFailureSpawn(), response(text("The parent ended first"), finish()), func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
				nextRequest <- request
				response(text("Reviewed background work"), finish())(ctx, request, yield)
			}}}
			child := &scriptedModel{scripts: []script{scriptedChildFailure(failChild)}}
			f := backgroundFailureFixture(t, main, child)
			f.start()
			first := f.submit("Original task")
			f.waitRun(first, protocol.ConversationStatusCompleted)
			childID := onlyBackgroundTestChild(t, f)
			f.waitRun(childID, protocol.ConversationStatusRunning)
			reservation := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
			t.Cleanup(func() { _ = reservation.Close() })
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Page = backgroundFailureHistory(t, f).Page
			stream := reservation.Subscribe(snapshot)
			if _, err := stream.Next(f.ctx); err != nil {
				t.Fatal(err)
			}
			const input = "Continue the original task without changing this input."
			var receiver protocol.TurnID
			if scenario != "automatic" {
				receiver = f.command("queue_turn", &protocol.CommandQueueTurnPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Message: protocol.ConversationInput{Text: input}}).Data.(protocol.QueuedTurn).ID
			}
			if scenario == "legacy failure before queued input" {
				if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
					id := "agent:" + childID.String()
					var body []byte
					if err := db.QueryRowContext(f.ctx, "SELECT body FROM agent_background WHERE id=?1", id).Scan(&body); err != nil {
						return err
					}
					var evidence store.BackgroundRecord
					if err := json.Unmarshal(body, &evidence); err != nil {
						return err
					}
					evidence.FollowUp = false
					body, err := json.Marshal(evidence)
					if err != nil {
						return err
					}
					_, err = db.ExecContext(f.ctx, "UPDATE agent_background SET body=?2 WHERE id=?1", id, body)
					return err
				}); err != nil {
					t.Fatal(err)
				}
			}
			status := protocol.ConversationStatusFailed
			if scenario == "stopped child" {
				f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: childID})
				status = protocol.ConversationStatusCancelled
			} else {
				close(failChild)
			}
			failed := f.waitRun(childID, status)
			if scenario != "automatic" {
				f.command("start_queued_turn", &protocol.CommandStartQueuedTurnPayload{Turn: receiver})
			}
			request := awaitBackgroundTest(t, f.ctx, nextRequest)
			if scenario == "automatic" {
				for _, run := range backgroundFailureHistory(t, f).Page.Runs {
					if run.Notice {
						receiver = run.Turn
						if run.Input != nil {
							t.Fatal("public failure notice exposed input")
						}
					}
				}
				if receiver == (protocol.TurnID{}) {
					t.Fatal("idle delivery did not use the existing automatic notice queue")
				}
			}
			f.waitRun(receiver, protocol.ConversationStatusCompleted)
			if scenario == "stopped child" {
				if len(backgroundFailureReports(f.state())) != 0 || strings.Contains(promptText(request), fixtureChildFailureMarker) {
					t.Fatal("a stopped child was reported as failed")
				}
			} else {
				report := requireBackgroundFailureReport(t, f, receiver, failed)
				assertFailureModelEvidence(t, request)
				if scenario == "automatic" {
					assertBackgroundTestInput(t, f.state(), receiver, "")
					seen := false
				live:
					for {
						update, err := stream.Next(f.ctx)
						if err != nil {
							t.Fatal(err)
						}
						frame, ok := update.Data.(*protocol.ConversationFrame)
						if !ok {
							t.Fatalf("notice feed reset during delivery: %s", update.Kind)
						}
						switch value := frame.Change.Data.(type) {
						case *protocol.ConversationEntry:
							if value.Turn == receiver && (value.Author == "user" || value.ID == report.ID) {
								t.Fatal("failure appeared as a visible feed entry")
							}
						case *protocol.ConversationChangeHiddenPayload:
							if value.Turn == receiver && value.ID == report.ID {
								seen = true
							}
						case *protocol.ConversationRun:
							if value.Turn == receiver && value.Status == protocol.ConversationStatusCompleted {
								if !seen {
									t.Fatal("notice finished without publishing its hidden entry")
								}
								break live
							}
						}
					}
				} else {
					assertBackgroundTestInput(t, f.state(), receiver, input)
				}
			}
			if child.count() != 1 || onlyBackgroundTestChild(t, f) != childID || main.count() != 3 || len(backgroundFailureHistory(t, f).Page.Queue.Items) != 0 {
				t.Fatal("failure or cancellation replayed the child or left queued input")
			}
		})
	}
}

func TestForegroundChildFailureStaysInForeground(t *testing.T) {
	release := make(chan struct{})
	f, parent := delegationFixture(t, &scriptedModel{scripts: []script{scriptedChildFailure(release)}})
	spawn := saveDelegationCall(t, f, parent, "foreground-failure", false)
	if result := f.r.spawnChild(parent, spawn, spawn.Arguments); result.Failure != nil {
		t.Fatal(result.Failure)
	}
	f.r.workers.Wait()
	child := parent.foreground[0]
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := store.ReleaseChildren(f.ctx, db, parent.inv.Run.Turn, []protocol.TurnID{child.turn})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	child.released = true
	f.start()
	close(release)
	failed := f.waitRun(child.turn, protocol.ConversationStatusFailed)
	if delivered, err := f.r.deliverBackground(parent, false); err != nil || delivered {
		t.Fatalf("background delivery took a foreground child: %t, %v", delivered, err)
	}
	if delivered, err := f.r.foregroundStep(parent, true); err != nil || !delivered {
		t.Fatalf("foreground result was not delivered: %t, %v", delivered, err)
	}
	if delivered, err := f.r.deliverBackground(parent, false); err != nil || delivered {
		t.Fatalf("foreground result was repeated as a background failure: %t, %v", delivered, err)
	}
	if failed.Error == nil {
		t.Fatal("foreground failure has no stored reason")
	}
	result := mustHaveCallback(t, f.state().Records, child.session, child.turn, protocol.ConversationBackgroundOutcomeFailed)
	if result.Notice.Summary == nil || *result.Notice.Summary != failed.Error.Message || result.ErrorCode == nil || *result.ErrorCode != failed.Error.Code {
		t.Fatalf("foreground callback lost the fault: %+v", result)
	}
	assertBackgroundFailureState(t, f, child.turn, protocol.ConversationBackgroundStateConsumed, nil)
}

func TestBackgroundChildFailureDeliveryRollsBackWithCanonicalAppend(t *testing.T) {
	f, parent := delegationFixture(t)
	const uncertain = "The original child effect could not be confirmed"
	f.r.authorizeProvider = func(context.Context, *store.AgentInvocation) error {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: uncertain}
	}
	spawn := saveDelegationCall(t, f, parent, "unknown-failure", true)
	if result := f.r.spawnChild(parent, spawn, spawn.Arguments); result.Failure != nil {
		t.Fatal(result.Failure)
	}
	f.start()
	child := onlyBackgroundTestChild(t, f)
	f.waitRun(child, protocol.ConversationStatusFailed)
	write := func(query string) {
		t.Helper()
		if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(f.ctx, query)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	write(`CREATE TRIGGER refuse_failure_message BEFORE INSERT ON agent_events WHEN json_extract(NEW.body,'$.provenance.author')='system'
		BEGIN SELECT RAISE(ABORT,'injected canonical append failure'); END`)
	if delivered, err := f.r.deliverBackground(parent, false); err == nil || delivered {
		t.Fatalf("failed append confirmed a delivery: %t, %v", delivered, err)
	}
	assertBackgroundFailureState(t, f, child, protocol.ConversationBackgroundStatePending, nil)
	if len(backgroundFailureReports(f.state())) != 0 {
		t.Fatal("refused append left a runtime message")
	}
	write(`DROP TRIGGER refuse_failure_message`)
	if delivered, err := f.r.deliverBackground(parent, false); err != nil || !delivered {
		t.Fatalf("the failure was not available after rollback: %t, %v", delivered, err)
	}
	if delivered, err := f.r.deliverBackground(parent, false); err != nil || delivered {
		t.Fatalf("attempt to redeliver an unknown outcome: %t, %v", delivered, err)
	}
	assertBackgroundFailureState(t, f, child, protocol.ConversationBackgroundStateDelivered, &parent.inv.Run.Turn)
	reports := backgroundFailureReports(f.state())
	if len(reports) != 1 || onlyBackgroundTestChild(t, f) != child {
		t.Fatal("unknown outcome was lost, duplicated or replayed")
	}
	if result := reports[0].Background().Results[0]; result.Notice.Summary == nil || *result.Notice.Summary != uncertain || result.ErrorCode == nil || *result.ErrorCode != protocol.ErrorCodeOutcomeUnknown {
		t.Fatal("the runtime message lost the stored unknown outcome")
	}
}

func backgroundFailureSpawn() script {
	return response(call("spawn", "spawn_agent", `{"task":"Check background facts","title":"`+fixtureChildTitle+`","background":true}`), finish())
}

func scriptedChildFailure(release <-chan struct{}) script {
	return func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
		select {
		case <-release:
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.InvalidResponse, Message: fixtureChildFailure}})
		case <-ctx.Done():
		}
	}
}

func backgroundFailureFixture(t *testing.T, main, child *scriptedModel, tools ...Tool) *engineFixture {
	t.Helper()
	f := newFixture(t, main, tools...)
	f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		if binding.Session == f.session.ID {
			return main, nil
		}
		return child, nil
	}
	return f
}

func awaitBackgroundTest[T any](t *testing.T, ctx context.Context, ch <-chan T) T {
	t.Helper()
	select {
	case value := <-ch:
		return value
	case <-ctx.Done():
		t.Fatal(ctx.Err())
		var zero T
		return zero
	}
}

func onlyBackgroundTestChild(t *testing.T, f *engineFixture) protocol.TurnID {
	t.Helper()
	turns := f.childTurns()
	if len(turns) != 1 {
		t.Fatalf("found %d child runs, want one", len(turns))
	}
	return turns[0]
}

func backgroundFailureReports(state store.AgentState) []canonical.Record {
	var reports []canonical.Record
	for _, record := range state.Records {
		if batch := record.Background(); batch != nil {
			for _, result := range batch.Results {
				if result.Notice.Kind.Kind == "agent" && result.Notice.Outcome == protocol.ConversationBackgroundOutcomeFailed {
					reports = append(reports, record)
					break
				}
			}
		}
	}
	return reports
}

func requireBackgroundFailureReport(t *testing.T, f *engineFixture, receiver protocol.TurnID, child protocol.ConversationRun) canonical.Record {
	t.Helper()
	reports := backgroundFailureReports(f.state())
	if len(reports) != 1 {
		t.Fatalf("found %d failure messages, want one", len(reports))
	}
	record := reports[0]
	if record.Turn != receiver || record.Provenance.Author != "system" || record.Metadata.Runtime == nil || record.Metadata.Runtime.UserVisible || !record.Metadata.Runtime.AgentVisible {
		t.Fatalf("wrong failure audience or receiver: %+v", record)
	}
	item := mustHaveCallback(t, reports, child.Session, child.Turn, protocol.ConversationBackgroundOutcomeFailed)
	if child.Error == nil || item.Notice.Title != fixtureChildTitle || item.Notice.Summary == nil || *item.Notice.Summary != child.Error.Message || item.ErrorCode == nil || *item.ErrorCode != child.Error.Code {
		t.Fatalf("notification differs from the failed child: %+v (run: %+v)", item, child)
	}
	assertBackgroundFailureState(t, f, child.Turn, protocol.ConversationBackgroundStateDelivered, &receiver)
	for _, entry := range backgroundFailureHistory(t, f).Page.Entries {
		if entry.ID == record.ID {
			t.Fatal("runtime failure message was exposed in reloaded history")
		}
	}
	return record
}

func mustHaveCallback(t *testing.T, records []canonical.Record, session protocol.SessionID, turn protocol.TurnID, outcome protocol.ConversationBackgroundOutcome) canonical.BackgroundResult {
	t.Helper()
	var matches []canonical.BackgroundResult
	for _, record := range records {
		if batch := record.Background(); batch != nil {
			for _, result := range batch.Results {
				var child protocol.ConversationBackgroundKindAgentPayload
				switch value := result.Notice.Kind.Data.(type) {
				case protocol.ConversationBackgroundKindAgentPayload:
					child = value
				case *protocol.ConversationBackgroundKindAgentPayload:
					child = *value
				}
				if child.Session == session && child.Turn == turn && result.Notice.Outcome == outcome {
					matches = append(matches, result)
				}
			}
		}
	}
	if len(matches) != 1 {
		t.Fatalf("found %d runtime results for %s, %s; want one", len(matches), turn, outcome)
	}
	return matches[0]
}

func assertFailureModelEvidence(t *testing.T, request fantasy.Call) {
	t.Helper()
	seen := 0
	for _, message := range request.Prompt {
		for _, part := range message.Content {
			text, ok := part.(fantasy.TextPart)
			if !ok || !strings.Contains(text.Text, fixtureChildFailureMarker) {
				continue
			}
			body, ok := strings.CutPrefix(text.Text, canonical.BackgroundLabel)
			var results []canonical.BackgroundResult
			if message.Role != fantasy.MessageRoleAssistant || !ok || json.Unmarshal([]byte(body), &results) != nil || strings.Contains(body, "</runtime>") {
				t.Fatalf("failure escaped the runtime evidence role: %v %q", message.Role, text.Text)
			}
			for _, result := range results {
				if result.Notice.Summary != nil && *result.Notice.Summary == fixtureChildFailure {
					seen++
				}
			}
		}
	}
	if seen != 1 || len(request.Prompt) == 0 || request.Prompt[len(request.Prompt)-1].Role == fantasy.MessageRoleAssistant {
		t.Fatalf("request carried %d exact fault records or would prefill assistant text", seen)
	}
}

func assertBackgroundFailureState(t *testing.T, f *engineFixture, child protocol.TurnID, state protocol.ConversationBackgroundState, delivery *protocol.TurnID) {
	t.Helper()
	for _, evidence := range backgroundFailureHistory(t, f).Page.Background {
		if evidence.ID == "agent:"+child.String() {
			if evidence.State != state || (delivery == nil) != (evidence.Delivery == nil) || delivery != nil && *delivery != *evidence.Delivery {
				t.Fatalf("unexpected failure evidence: %+v", evidence)
			}
			return
		}
	}
	t.Fatal("background failure evidence is missing")
}

func backgroundFailureHistory(t *testing.T, f *engineFixture) protocol.ConversationHistory {
	t.Helper()
	var history protocol.ConversationHistory
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		history, err = store.ReadConversation(f.ctx, db, f.session.ID, nil, 20)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return history
}

func assertBackgroundTestInput(t *testing.T, state store.AgentState, turn protocol.TurnID, want string) {
	t.Helper()
	users := 0
	for _, record := range state.Records {
		if record.Turn != turn || record.Provenance.Author != "user" {
			continue
		}
		users++
		message := record.Message()
		if message == nil || len(message.Content) != 1 {
			t.Fatal("failure data was attached to the user message")
		}
		if content, ok := message.Content[0].Data.(*canonical.TextContent); !ok || content.Text != want {
			t.Fatal("the original user message changed")
		}
	}
	if users != 0 && want == "" || users != 1 && want != "" {
		t.Fatalf("unexpected number of user input records: %d", users)
	}
}
