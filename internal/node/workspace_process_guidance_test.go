package node

import (
	"context"
	"database/sql"
	"slices"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestGuideQueuedTurnInterruptsForegroundCommand(t *testing.T) {
	for _, outcome := range []string{"cancelled", "exited", "unknown"} {
		t.Run(outcome, func(t *testing.T) {
			started, cancelled := make(chan struct{}), make(chan struct{})
			backgroundStarted, backgroundStopped := make(chan struct{}), make(chan struct{})
			var launches atomic.Int32
			f := newStopFixture(t, func(ctx context.Context, launch process.Launch, output func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
				launches.Add(1)
				result := protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}
				switch launch.Command {
				case "background work":
					close(backgroundStarted)
					<-ctx.Done()
					close(backgroundStopped)
					return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
				case "in progress":
				default:
					return result, nil
				}
				close(started)
				<-ctx.Done()
				close(cancelled)
				output(protocol.ProcessOutputOutputStreamStdout, []byte("output before cancellation"))
				switch outcome {
				case "cancelled":
					result.Outcome = protocol.ProcessOutcome{Kind: "cancelled"}
				case "unknown":
					result.Outcome = protocol.ProcessOutcome{Kind: "unknown", Data: "original uncertain result"}
				}
				return result, nil
			})
			f.script.reset(func(step int, _ map[string]string) [][3]string {
				if step == 1 {
					return [][3]string{
						{"prior", "run_command", `{"command":"already done"}`},
						{"background", "run_command", `{"command":"background work","background":true}`},
						{"active", "run_command", `{"command":"in progress"}`},
						{"later", "run_command", `{"command":"unneeded"}`},
					}
				}
				return nil
			})
			f.submit("Original task")
			f.wait("the background command", backgroundStarted)
			f.wait("the foreground command", started)
			runs, _, _ := f.read()
			recipient := runs[0].Turn
			ordinary := queueGuidanceFixture(f, "Leave this in the queue")
			chosen := queueGuidanceFixture(f, "Use this direction instead")
			dispatch := protocol.NewRequest(f.node.ID(), protocol.Command{Kind: "guide_queued_turn", Data: protocol.CommandGuideQueuedTurnPayload{
				Turn: chosen.Turn, Recipient: recipient, ExpectedRevision: chosen.Revision,
			}})
			for range 2 {
				if result := dispatchFixture(t, f.node, f.node.ID(), dispatch); result.Fault != nil {
					t.Fatal(result.Fault)
				}
			}
			f.wait("foreground cancellation", cancelled)
			f.until("the guided task to finish", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
				return slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
					return run.Turn == recipient && run.Status == protocol.ConversationStatusCompleted
				}) && slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
					return run.Turn == chosen.Turn && run.Joined != nil && *run.Joined == recipient
				})
			})
			f.script.mu.Lock()
			prompts := slices.Clone(f.script.prompts)
			f.script.mu.Unlock()
			if len(prompts) != 2 || !strings.Contains(prompts[1], chosen.Preview) || strings.Contains(prompts[1], ordinary.Preview) {
				t.Fatalf("selected guidance did not reach the next model call: %v", prompts)
			}
			_, background, queue := f.read()
			if queue.Paused || len(queue.Items) != 1 || queue.Items[0].Turn != ordinary.Turn || launches.Load() != 3 {
				t.Fatalf("unexpected background/queue/launches: %+v / %+v / %d", background, queue, launches.Load())
			}
			if commands := f.node.processes.List(f.session.ID); !slices.ContainsFunc(commands, func(command protocol.ProcessInfo) bool {
				return command.Command == "background work" && command.Status.Kind == "running"
			}) {
				t.Fatalf("the registered background command did not remain active: %+v", commands)
			}
			select {
			case <-backgroundStopped:
				t.Fatal("steering cancelled an existing background command")
			default:
			}

			results := guidanceToolResults(f)
			if len(results) != 4 || results["prior"] == nil || results["prior"].Status != canonical.ToolSuccess || results["later"] == nil || results["later"].Status != canonical.ToolErrorStatus || !strings.Contains(string(results["later"].Result.Value), string(protocol.ErrorCodeCancelled)) {
				t.Fatalf("prior success or skipped call lost: %+v", results)
			}
			active := results["active"]
			if active == nil || !strings.Contains(string(active.Result.Value), "output before cancellation") {
				t.Fatalf("foreground receipt lost: %+v", active)
			}
			if outcome == "exited" && active.Status != canonical.ToolSuccess || outcome != "exited" && (active.Status != canonical.ToolErrorStatus || !strings.Contains(string(active.Result.Value), `"kind":"unknown"`)) {
				t.Fatalf("foreground outcome was replaced: %s", active.Result.Value)
			}
			if outcome == "unknown" && !strings.Contains(string(active.Result.Value), "original uncertain result") {
				t.Fatalf("original unknown outcome was lost: %s", active.Result.Value)
			}
		})
	}
}

func TestQueueTurnAutoStartsAfterRunningTurn(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	f := newStopFixture(t, func(ctx context.Context, _ process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		close(started)
		select {
		case <-release:
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
		case <-ctx.Done():
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
		}
	})
	f.script.reset(func(step int, _ map[string]string) [][3]string {
		if step == 1 {
			return [][3]string{{"active", "run_command", `{"command":"first turn"}`}}
		}
		return nil
	})
	f.submit("Original task")
	f.wait("the foreground command", started)
	runs, _, _ := f.read()
	first := runs[0].Turn
	held := queueGuidanceFixture(f, "Manually held message")
	session := commandFixture(t, f.node, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: f.session.ID}}).Data.(protocol.Session)
	queued := commandFixture(t, f.node, protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, AutoStart: true, Message: protocol.ConversationInput{Text: "Automatically run after the previous turn"},
	}}).Data.(protocol.QueuedTurn)
	_, _, queue := f.read()
	if len(queue.Items) != 2 || queue.Items[0].Ready || queue.Items[1].Turn != queued.ID || !queue.Items[1].Ready {
		t.Fatalf("the follow-up and held message should have different readiness: %+v", queue)
	}
	close(release)
	f.until("the queued follow-up to start and complete without another command", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
			return run.Turn == first && run.Status == protocol.ConversationStatusCompleted
		}) && slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
			return run.Turn == queued.ID && run.Status == protocol.ConversationStatusCompleted && run.Joined == nil
		})
	})
	_, _, queue = f.read()
	if len(queue.Items) != 1 || queue.Items[0].Turn != held.Turn || queue.Items[0].Ready {
		t.Fatalf("automatic continuation changed the held message: %+v", queue)
	}
}

func TestGuideQueuedTurnInterruptsWithDifferentSettings(t *testing.T) {
	started, cancelled := make(chan struct{}), make(chan struct{})
	f := newStopFixture(t, func(ctx context.Context, _ process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		close(started)
		<-ctx.Done()
		close(cancelled)
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
	})
	f.script.reset(func(step int, _ map[string]string) [][3]string {
		if step == 1 {
			return [][3]string{{"active", "run_command", `{"command":"in progress"}`}}
		}
		return nil
	})
	f.submit("Original task")
	f.wait("the foreground command", started)
	runs, _, _ := f.read()
	recipient := runs[0].Turn
	ordinary := queueGuidanceFixture(f, "Leave this in the queue")
	session := commandFixture(t, f.node, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: f.session.ID}}).Data.(protocol.Session)
	session.Config.Permission = protocol.PermissionAsk
	commandFixture(t, f.node, protocol.Command{Kind: "set_session_config", Data: protocol.CommandSetSessionConfigPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Config: session.Config,
	}})
	chosen := queueGuidanceFixture(f, "Send this with the new permissions")
	commandFixture(t, f.node, protocol.Command{Kind: "guide_queued_turn", Data: protocol.CommandGuideQueuedTurnPayload{
		Turn: chosen.Turn, Recipient: recipient, ExpectedRevision: chosen.Revision,
	}})
	f.wait("foreground cancellation", cancelled)
	f.until("the selected input to complete as its own turn", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
			return run.Turn == recipient && run.Status == protocol.ConversationStatusCancelled
		}) && slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
			return run.Turn == chosen.Turn && run.Status == protocol.ConversationStatusCompleted && run.Joined == nil
		})
	})
	_, _, queue := f.read()
	if len(queue.Items) != 1 || queue.Items[0].Turn != ordinary.Turn || queue.Items[0].Ready {
		t.Fatalf("sending the selected input changed another queued item: %+v", queue)
	}
}

func TestGuideQueuedTurnAfterRecipientCompletes(t *testing.T) {
	f := newStopFixture(t, nil)
	f.script.reset(func(int, map[string]string) [][3]string { return nil })
	f.submit("Finish the first task")
	runs, _ := f.until("the first task to finish", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return len(runs) == 1 && runs[0].Status == protocol.ConversationStatusCompleted
	})
	ordinary := queueGuidanceFixture(f, "Keep the first queue item")
	chosen := queueGuidanceFixture(f, "Start the selected item")
	commandFixture(t, f.node, protocol.Command{Kind: "guide_queued_turn", Data: protocol.CommandGuideQueuedTurnPayload{
		Turn: chosen.Turn, Recipient: runs[0].Turn, ExpectedRevision: chosen.Revision,
	}})
	f.until("the selected item to run independently", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
			return run.Turn == chosen.Turn && run.Joined == nil && run.Status == protocol.ConversationStatusCompleted
		})
	})
	_, _, queue := f.read()
	if len(queue.Items) != 1 || queue.Items[0].Turn != ordinary.Turn || queue.Items[0].Ready {
		t.Fatalf("idle send changed another queued item: %+v", queue)
	}
}

func TestGuideQueuedTurnCannotInterruptReplacement(t *testing.T) {
	started, cancelled, release := make(chan struct{}), make(chan struct{}), make(chan struct{})
	f := newStopFixture(t, func(ctx context.Context, _ process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		close(started)
		select {
		case <-ctx.Done():
			close(cancelled)
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
		case <-release:
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
		}
	})
	f.script.reset(func(step int, _ map[string]string) [][3]string {
		if step == 2 {
			return [][3]string{{"replacement", "run_command", `{"command":"replacement work"}`}}
		}
		return nil
	})
	f.submit("Finish the first task")
	runs, _ := f.until("the first task to finish", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return len(runs) == 1 && runs[0].Status == protocol.ConversationStatusCompleted
	})
	recipient := runs[0].Turn
	f.submit("Start a different task")
	f.wait("the replacement command", started)
	item := queueGuidanceFixture(f, "Send this to the first task")
	result := dispatchFixture(t, f.node, f.node.ID(), protocol.NewRequest(f.node.ID(), protocol.Command{Kind: "guide_queued_turn", Data: protocol.CommandGuideQueuedTurnPayload{
		Turn: item.Turn, Recipient: recipient, ExpectedRevision: item.Revision,
	}}))
	if reason, _ := store.QueueRejection("guide_queued_turn", result.Fault); reason != "recipient_not_running" {
		t.Fatalf("stale send returned %+v", result)
	}
	close(release)
	f.until("the replacement task to finish", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
			return run.Turn != recipient && run.Turn != item.Turn && run.Status == protocol.ConversationStatusCompleted
		})
	})
	select {
	case <-cancelled:
		t.Fatal("stale guidance cancelled the replacement command")
	default:
	}
	_, _, queue := f.read()
	if len(queue.Items) != 1 || queue.Items[0].Turn != item.Turn || queue.Items[0].Ready {
		t.Fatalf("stale guidance changed the queue: %+v", queue)
	}
}

func queueGuidanceFixture(f stopFixture, text string) protocol.ConversationPending {
	f.t.Helper()
	current := commandFixture(f.t, f.node, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: f.session.ID}}).Data.(protocol.Session)
	queued := commandFixture(f.t, f.node, protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{
		Session: current.ID, ExpectedRevision: current.Revision, Message: protocol.ConversationInput{Text: text},
	}}).Data.(protocol.QueuedTurn)
	_, _, queue := f.read()
	for _, item := range queue.Items {
		if item.Turn == queued.ID {
			return item
		}
	}
	f.t.Fatalf("queued input %s is missing from %+v", queued.ID, queue)
	return protocol.ConversationPending{}
}

func guidanceToolResults(f stopFixture) map[string]*canonical.ToolResponse {
	f.t.Helper()
	results := map[string]*canonical.ToolResponse{}
	if err := f.node.profile.Store.Node.Read(f.t.Context(), func(db *sql.Conn) error {
		state, err := store.LoadAgentState(f.t.Context(), db, f.session.ID)
		if err != nil {
			return err
		}
		for _, record := range state.Records {
			if message := record.Message(); message != nil {
				for _, block := range message.Content {
					if result, ok := block.Data.(*canonical.ToolResponse); ok {
						results[result.ID] = result
					}
				}
			}
		}
		return nil
	}); err != nil {
		f.t.Fatal(err)
	}
	return results
}
