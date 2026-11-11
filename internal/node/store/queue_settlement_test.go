package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// queueFixtureCommand runs one controller command as the Node routes it.
func queueFixtureCommand(ctx context.Context, db *sql.Conn, storage *Store, node protocol.NodeID, kind string, data any, now time.Time) (protocol.Output, error) {
	request := protocol.NewRequest(node, protocol.Command{Kind: kind, Data: data})
	body, err := encode(request)
	if err != nil {
		return protocol.Output{}, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
		return protocol.Output{}, err
	}
	// A refused command leaves nothing behind, as its own transaction would.
	if _, err = db.ExecContext(ctx, "SAVEPOINT fixture_command"); err != nil {
		return protocol.Output{}, err
	}
	output, _, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, node, request, now)
	if err != nil {
		if _, rollback := db.ExecContext(ctx, "ROLLBACK TO fixture_command"); rollback != nil {
			return output, rollback
		}
	}
	if _, release := db.ExecContext(ctx, "RELEASE fixture_command"); release != nil {
		return output, release
	}
	return output, err
}

// queueFixtureInput records the running turn's own input, as its first save does.
func queueFixtureInput(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, now time.Time) error {
	record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "Fixture input"))), now)
	record.Provenance.Author = "user"
	_, _, err := AppendCanonical(ctx, db, session, record)
	return err
}

func queueFixtureItem(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (protocol.ConversationQueue, *protocol.ConversationPending, error) {
	queue, err := ReadQueue(ctx, db, session)
	if err != nil {
		return queue, nil, err
	}
	for i := range queue.Items {
		if queue.Items[i].Turn == turn {
			return queue, &queue.Items[i], nil
		}
	}
	return queue, nil, nil
}

func TestQueueSettlement(t *testing.T) {
	t.Parallel()
	guide := func(ctx context.Context, db *sql.Conn, storage *Store, node protocol.NodeID, session protocol.Session, recipient protocol.TurnID, now time.Time) (protocol.TurnID, error) {
		output, err := queueFixtureCommand(ctx, db, storage, node, "guide_turn", &protocol.CommandGuideTurnPayload{Recipient: recipient, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Guidance input"}}, now)
		if err != nil {
			return protocol.TurnID{}, err
		}
		guided := output.Data.(protocol.QueuedTurn).ID
		_, item, err := queueFixtureItem(ctx, db, session.ID, guided)
		if err != nil {
			return guided, err
		}
		if item == nil || item.Guidance == nil || *item.Guidance != recipient {
			return guided, errors.New("guidance was not addressed to the running turn")
		}
		if next, _, err := ClaimNext(ctx, db, now); err != nil || next != nil {
			return guided, fmt.Errorf("addressed guidance started beside its recipient: %v", err)
		}
		return guided, nil
	}
	claimed := func(ctx context.Context, db *sql.Conn, turn protocol.TurnID, text string, now time.Time) error {
		next, _, err := ClaimNext(ctx, db, now)
		if err != nil {
			return err
		}
		if next == nil || next.Run.Turn != turn || next.Message.Text != text {
			return errors.New("the expected queued input did not start")
		}
		if again, _, err := ClaimNext(ctx, db, now); err != nil || again != nil {
			return fmt.Errorf("queued input started twice: %v", err)
		}
		return nil
	}
	held := func(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, now time.Time) (protocol.ConversationPending, error) {
		queue, item, err := queueFixtureItem(ctx, db, session, turn)
		if err != nil {
			return protocol.ConversationPending{}, err
		}
		if item == nil || item.Guidance != nil || !queue.Paused {
			return protocol.ConversationPending{}, errors.New("released guidance is not an ordinary item of a paused queue")
		}
		if next, _, err := ClaimNext(ctx, db, now); err != nil || next != nil {
			return *item, fmt.Errorf("a paused queue started input: %v", err)
		}
		return *item, nil
	}
	for _, scenario := range []string{"guidance after stop waits for send now", "guidance after stop waits for resume", "guidance after completion starts", "guidance after failure starts", "guidance after restart waits for resume", "send now replaces the running turn", "failure keeps queued input running", "failure before input holds the queue", "stop ends cancelled without attention", "compaction raises no attention", "restart cancels a queued notice turn without pausing", "restart interrupts a queued child"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if scenario != "failure before input holds the queue" {
					if err := queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
						return err
					}
				}
				switch scenario {
				case "guidance after stop waits for send now", "guidance after stop waits for resume":
					guided, err := guide(ctx, db, storage, node, session, turn, now)
					if err != nil {
						return err
					}
					if _, err = queueFixtureCommand(ctx, db, storage, node, "stop_turn", &protocol.CommandStopTurnPayload{Turn: turn}, now); err != nil {
						return err
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusInterrupted, nil, now); err != nil {
						return err
					}
					item, err := held(ctx, db, session.ID, guided, now)
					if err != nil {
						return err
					}
					if scenario == "guidance after stop waits for send now" {
						_, err = queueFixtureCommand(ctx, db, storage, node, "send_queued_turn", &protocol.CommandSendQueuedTurnPayload{Turn: guided, ExpectedRevision: item.Revision}, now)
					} else {
						queue, e := ReadQueue(ctx, db, session.ID)
						if e != nil {
							return e
						}
						_, err = queueFixtureCommand(ctx, db, storage, node, "set_queue_paused", &protocol.CommandSetQueuePausedPayload{Session: session.ID, ExpectedRevision: queue.Revision, Paused: false}, now)
					}
					if err != nil {
						return err
					}
					return claimed(ctx, db, guided, "Guidance input", now)
				case "guidance after completion starts", "guidance after failure starts":
					guided, err := guide(ctx, db, storage, node, session, turn, now)
					if err != nil {
						return err
					}
					status, failure := protocol.ConversationStatusCompleted, (*protocol.Fault)(nil)
					if scenario == "guidance after failure starts" {
						status, failure = protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "provider failed"}
					}
					if _, _, err = FinishTurn(ctx, db, turn, status, failure, now); err != nil {
						return err
					}
					queue, item, err := queueFixtureItem(ctx, db, session.ID, guided)
					if err != nil {
						return err
					}
					if item == nil || item.Guidance != nil || queue.Paused {
						return errors.New("unjoined guidance did not return to a running queue")
					}
					return claimed(ctx, db, guided, "Guidance input", now)
				case "guidance after restart waits for resume":
					guided, err := guide(ctx, db, storage, node, session, turn, now)
					if err != nil {
						return err
					}
					if _, err = RecoverRuns(ctx, db, now); err != nil {
						return err
					}
					released, err := held(ctx, db, session.ID, guided, now)
					if err != nil {
						return err
					}
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
					}
					// A second recovery finds nothing left to release.
					if _, err = RecoverRuns(ctx, db, now); err != nil {
						return err
					}
					again, item, err := queueFixtureItem(ctx, db, session.ID, guided)
					if err != nil {
						return err
					}
					if again.Revision != queue.Revision || len(again.Items) != 1 || item == nil || item.Revision != released.Revision {
						return errors.New("recovery released the guidance more than once")
					}
					if _, err = queueFixtureCommand(ctx, db, storage, node, "set_queue_paused", &protocol.CommandSetQueuePausedPayload{Session: session.ID, ExpectedRevision: queue.Revision, Paused: false}, now); err != nil {
						return err
					}
					return claimed(ctx, db, guided, "Guidance input", now)
				case "send now replaces the running turn":
					output, err := queueFixtureCommand(ctx, db, storage, node, "queue_turn", &protocol.CommandQueueTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Chosen input"}}, now)
					if err != nil {
						return err
					}
					chosen := output.Data.(protocol.QueuedTurn).ID
					_, item, err := queueFixtureItem(ctx, db, session.ID, chosen)
					if err != nil || item == nil {
						return fmt.Errorf("queued input is missing: %v", err)
					}
					if _, err = queueFixtureCommand(ctx, db, storage, node, "send_queued_turn", &protocol.CommandSendQueuedTurnPayload{Turn: chosen, ExpectedRevision: item.Revision}, now); err != nil {
						return err
					}
					stopping, err := ReadAgentRun(ctx, db, turn)
					if err != nil || stopping.Status != protocol.ConversationStatusStopping {
						return fmt.Errorf("send now did not stop the running turn: %v", err)
					}
					if next, _, err := ClaimNext(ctx, db, now); err != nil || next != nil {
						return fmt.Errorf("chosen input started beside the stopping turn: %v", err)
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusInterrupted, nil, now); err != nil {
						return err
					}
					return claimed(ctx, db, chosen, "Chosen input", now)
				case "failure keeps queued input running":
					output, err := queueFixtureCommand(ctx, db, storage, node, "submit_turn", &protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Next input"}}, now)
					if err != nil {
						return err
					}
					if _, err = backgroundFixtureRecord(ctx, db, session, turn, "after-failure", true, now); err != nil {
						return err
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "provider failed"}, now); err != nil {
						return err
					}
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if queue.Paused {
						return errors.New("a failed turn paused the queue")
					}
					if activity, err := readActivity(ctx, db, session.ID); err != nil || !activity.Attention.Unread {
						return fmt.Errorf("a failed task turn raised no attention: %v", err)
					}
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil {
						return err
					}
					// The waiting result joins the user's next turn rather than a notice turn.
					if next == nil || next.Run.Turn != output.Data.(protocol.QueuedTurn).ID || next.Run.Notice {
						return errors.New("queued input did not start after a failed turn")
					}
					return nil
				case "stop ends cancelled without attention":
					if _, err := queueFixtureCommand(ctx, db, storage, node, "stop_turn", &protocol.CommandStopTurnPayload{Turn: turn}, now); err != nil {
						return err
					}
					// A stop wins over whatever the turn reported at its end.
					run, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "request was aborted"}, now)
					if err != nil {
						return err
					}
					activity, err := readActivity(ctx, db, session.ID)
					if err != nil {
						return err
					}
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if run.Status != protocol.ConversationStatusCancelled || run.Error != nil || activity.Attention.Unread || !queue.Paused {
						return fmt.Errorf("stopped turn settled as %s, error %v, unread %v, paused %v", run.Status, run.Error, activity.Attention.Unread, queue.Paused)
					}
					return nil
				case "compaction raises no attention":
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if _, err := db.ExecContext(ctx, "UPDATE session_attention SET unread=0 WHERE session=?1", session.ID.String()); err != nil {
						return err
					}
					output, err := queueFixtureCommand(ctx, db, storage, node, "compact_context", &protocol.CommandCompactContextPayload{Session: session.ID, ExpectedRevision: session.Revision}, now)
					if err != nil {
						return err
					}
					compaction := output.Data.(protocol.QueuedTurn).ID
					if next, _, err := ClaimNext(ctx, db, now); err != nil || next == nil || next.Run.Turn != compaction {
						return fmt.Errorf("compaction was not claimed: %v", err)
					}
					if _, _, err = FinishTurn(ctx, db, compaction, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if activity, err := readActivity(ctx, db, session.ID); err != nil || activity.Attention.Unread {
						return fmt.Errorf("a compaction run raised attention: %v", err)
					}
					return nil
				case "restart cancels a queued notice turn without pausing":
					if _, err := backgroundFixtureRecord(ctx, db, session, turn, "before-restart", true, now); err != nil {
						return err
					}
					// Finishing the turn admits the notice turn for the waiting result.
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if len(queue.Items) != 1 || !queue.Items[0].Notice {
						return errors.New("the waiting result admitted no notice turn")
					}
					before, err := readActivity(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if _, err = RecoverCommands(ctx, db, now); err != nil {
						return err
					}
					if _, err = RecoverRuns(ctx, db, now); err != nil {
						return err
					}
					notice, err := ReadAgentRun(ctx, db, queue.Items[0].Turn)
					if err != nil {
						return err
					}
					queue, err = ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
					}
					after, err := readActivity(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if notice.Status != protocol.ConversationStatusCancelled || len(queue.Items) != 0 || queue.Paused || after.Attention.Revision != before.Attention.Revision {
						return fmt.Errorf("recovered notice turn is %s, items %d, paused %v, attention %d to %d", notice.Status, len(queue.Items), queue.Paused, before.Attention.Revision, after.Attention.Revision)
					}
					return nil
				case "restart interrupts a queued child":
					call, err := delegationFixtureCall(ctx, db, turn, "spawn_agent", map[string]any{"task": "Orphaned child"}, now)
					if err != nil {
						return err
					}
					orphan, _, err := DelegateAgent(ctx, db, node, turn, call, nil, true, now)
					if err != nil {
						return err
					}
					if call, err = delegationFixtureCall(ctx, db, turn, "spawn_agent", map[string]any{"task": "Stopped child"}, now); err != nil {
						return err
					}
					stopped, _, err := DelegateAgent(ctx, db, node, turn, call, nil, true, now)
					if err != nil {
						return err
					}
					if _, err = stopAgentTurn(ctx, db, stopped.Run.Turn, now); err != nil {
						return err
					}
					if _, err = RecoverRuns(ctx, db, now); err != nil {
						return err
					}
					// A child the user stopped before the restart was not lost work.
					if run, err := ReadAgentRun(ctx, db, stopped.Run.Turn); err != nil || run.Status != protocol.ConversationStatusCancelled || run.FinishedMs == nil || run.Error != nil {
						return fmt.Errorf("stopped queued child recovered as %s: %v", run.Status, err)
					}
					run, err := ReadAgentRun(ctx, db, orphan.Run.Turn)
					if err != nil {
						return err
					}
					if run.Status != protocol.ConversationStatusInterrupted || run.FinishedMs == nil || run.Error == nil || run.Error.Code != protocol.ErrorCodeOutcomeUnknown {
						return fmt.Errorf("queued child survived the restart as %s", run.Status)
					}
					// The orphan no longer holds the conversation's first child slot.
					parent, err := delegationFixtureParent(ctx, db, node, session, now)
					if err != nil {
						return err
					}
					if call, err = delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": "Later child"}, now); err != nil {
						return err
					}
					later, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
					if err != nil {
						return err
					}
					if next, _, err := ClaimChild(ctx, db, later.Run.Turn, now); err != nil || next == nil {
						return fmt.Errorf("a later child stayed behind the orphan: %v", err)
					}
					return nil
				default:
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "provider is unavailable"}, now); err != nil {
						return err
					}
					queue, item, err := queueFixtureItem(ctx, db, session.ID, turn)
					if err != nil {
						return err
					}
					if item == nil || item.Ready || !queue.Paused {
						return errors.New("unrecorded input was not retained in a paused queue")
					}
					return nil
				}
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestBackgroundDelivery(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"claim pass delivers once the queue resumes", "plan mode delivers", "automatic chain stops at its limit", "cancelled foreground child is consumed"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			mode := protocol.WorkModeCode
			if name == "plan mode delivers" {
				mode = protocol.WorkModePlan
			}
			storage, node, session, turn, now := agentHistoryFixture(t, mode)
			ctx := context.Background()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if _, err := db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", turn.String()); err != nil {
					return err
				}
				deliver := func(key string) (*AgentInvocation, error) {
					if _, err := backgroundFixtureRecord(ctx, db, session, turn, key, true, now); err != nil {
						return nil, err
					}
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil || next == nil {
						return nil, err
					}
					if !next.Run.Notice || len(next.Run.Notices) != 1 || next.Run.Notices[0].ID != key {
						return nil, fmt.Errorf("claim was not the notice turn for %s", key)
					}
					_, _, err = FinishTurn(ctx, db, next.Run.Turn, protocol.ConversationStatusCompleted, nil, now)
					return next, err
				}
				switch name {
				case "claim pass delivers once the queue resumes":
					if err := setQueuePaused(ctx, db, session.ID, true); err != nil {
						return err
					}
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if next, err := deliver("waiting"); err != nil || next != nil {
						return fmt.Errorf("paused queue started a notice turn: %v", err)
					}
					// Resuming schedules nothing itself; the claim pass finds the result.
					if err := setQueuePaused(ctx, db, session.ID, false); err != nil {
						return err
					}
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil {
						return err
					}
					if next == nil || !next.Run.Notice || len(next.Run.Notices) != 1 || next.Run.Notices[0].ID != "waiting" {
						return errors.New("result stayed undelivered after its blocker cleared")
					}
				case "plan mode delivers":
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if next, err := deliver("planned"); err != nil || next == nil {
						return fmt.Errorf("Plan mode withheld the result: %v", err)
					}
				case "cancelled foreground child is consumed":
					states := map[bool]protocol.ConversationBackgroundState{}
					var unreported protocol.TurnID
					for _, reported := range []bool{true, false} {
						call, err := delegationFixtureCall(ctx, db, turn, "spawn_agent", map[string]any{"task": "Foreground child"}, now)
						if err != nil {
							return err
						}
						child, _, err := DelegateAgent(ctx, db, node, turn, call, nil, true, now)
						if err != nil {
							return err
						}
						if next, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || next == nil {
							return fmt.Errorf("child did not start: %v", err)
						}
						if reported {
							record := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Subagent was cancelled before it finished.")), now)
							record.Provenance.Author = "user"
							record.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, Notes: &canonical.OperationNotes{CancelledSubagents: []string{child.Session.ID.String()}}}
							if _, _, err = AppendCanonical(ctx, db, session.ID, record); err != nil {
								return err
							}
						}
						if _, _, err = FinishTurn(ctx, db, child.Run.Turn, protocol.ConversationStatusCancelled, nil, now); err != nil {
							return err
						}
						task, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "agent:"+child.Run.Turn.String())
						if err != nil {
							return err
						}
						if task == nil || task.Task.Outcome == nil || *task.Task.Outcome != protocol.ConversationBackgroundOutcomeCancelled {
							return errors.New("cancelled child left no cancelled record")
						}
						states[reported] = task.Task.State
						if !reported {
							unreported = child.Run.Turn
						}
					}
					if states[true] != protocol.ConversationBackgroundStateConsumed || states[false] != protocol.ConversationBackgroundStatePending {
						return fmt.Errorf("reported child is %s, unreported child is %s", states[true], states[false])
					}
					// The other order: the child's end is stored before its owner
					// records the cancellation, and the owner's settlement consumes it.
					if _, err := ConsumeStoppedChildren(ctx, db, turn, []protocol.TurnID{unreported}); err != nil {
						return err
					}
					task, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "agent:"+unreported.String())
					if err != nil {
						return err
					}
					if task == nil || task.Task.State != protocol.ConversationBackgroundStateConsumed {
						return errors.New("child that ended before the cancellation note stayed pending")
					}
				default:
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					for index := 0; index < automaticLimit; index++ {
						if next, err := deliver(fmt.Sprintf("chain-%d", index)); err != nil || next == nil {
							return fmt.Errorf("notice turn %d did not start: %v", index, err)
						}
					}
					if next, err := deliver("held"); err != nil || next != nil {
						return fmt.Errorf("chain passed its limit: %v", err)
					}
					held, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='held'")
					if err != nil {
						return err
					}
					if held == nil || !held.Task.Suppressed || held.Task.State != protocol.ConversationBackgroundStatePending || held.Task.Delivery != nil || held.Task.Hold == nil || *held.Task.Hold != protocol.ConversationBackgroundHoldLimit {
						return errors.New("held result does not wait for user input")
					}
					current, err := ReadSession(ctx, db, session.ID)
					if err != nil {
						return err
					}
					user, err := delegationFixtureParent(ctx, db, node, current, now)
					if err != nil {
						return err
					}
					// The fixture records no input, which is what removes the row.
					if _, err = db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", user.String()); err != nil {
						return err
					}
					run, _, _, err := BindBackground(ctx, db, user)
					if err != nil {
						return err
					}
					// The fixture never confirms a notice turn's input, so the
					// earlier results are released to this turn as well.
					if len(run.Notices) == 0 || run.Notices[len(run.Notices)-1].ID != "held" {
						return errors.New("user turn did not receive the held result")
					}
					if _, _, err = FinishTurn(ctx, db, user, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if _, err = backgroundFixtureRecord(ctx, db, session, turn, "after-user", true, now); err != nil {
						return err
					}
					if next, _, err := ClaimNext(ctx, db, now); err != nil || next == nil || !next.Run.Notice {
						return fmt.Errorf("user input did not reset the chain: %v", err)
					}
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

// TestGuidanceQueue covers the queue and guidance transitions listed in
// docs/conversation.md that TestQueueSettlement does not.
func TestGuidanceQueue(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"pending guidance is taken together", "other settings stay ordinary", "outlived guidance starts one turn", "queue commands"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if err := queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
					return err
				}
				command := func(kind string, data any) (protocol.Output, error) {
					return queueFixtureCommand(ctx, db, storage, node, kind, data, now)
				}
				send := func(kind, text string, attachments ...protocol.AttachmentID) (protocol.TurnID, error) {
					current, err := ReadSession(ctx, db, session.ID)
					if err != nil {
						return protocol.TurnID{}, err
					}
					message := protocol.ConversationInput{Text: text, Attachments: attachments}
					var data any = &protocol.CommandGuideTurnPayload{Recipient: turn, ExpectedRevision: current.Revision, Message: message}
					if kind == "queue_turn" {
						data = &protocol.CommandQueueTurnPayload{Session: session.ID, ExpectedRevision: current.Revision, Message: message}
					}
					output, err := command(kind, data)
					if err != nil {
						return protocol.TurnID{}, err
					}
					return output.Data.(protocol.QueuedTurn).ID, nil
				}
				// order checks the queue's items and which of them wait for a recipient.
				order := func(want []protocol.TurnID, recipients ...*protocol.TurnID) error {
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if len(queue.Items) != len(want) {
						return fmt.Errorf("queue holds %d items, want %d", len(queue.Items), len(want))
					}
					for i, item := range queue.Items {
						if item.Turn != want[i] {
							return fmt.Errorf("queue position %d holds another item", i)
						}
						if i < len(recipients) && (item.Guidance == nil) != (recipients[i] == nil) || i < len(recipients) && item.Guidance != nil && *item.Guidance != *recipients[i] {
							return fmt.Errorf("queue position %d waits for the wrong recipient", i)
						}
					}
					return nil
				}
				texts := func(values []AgentGuidance) []string {
					out := []string{}
					for _, value := range values {
						out = append(out, value.Message.Text)
					}
					return out
				}
				revision := func(id protocol.TurnID) uint64 {
					_, item, _ := queueFixtureItem(ctx, db, session.ID, id)
					if item == nil {
						return 0
					}
					return item.Revision
				}
				switch scenario {
				case "pending guidance is taken together":
					waiting, err := send("queue_turn", "Ordinary")
					if err != nil {
						return err
					}
					first, err := send("guide_turn", "First")
					if err != nil {
						return err
					}
					attachment := protocol.DefaultAttachment()
					attachment.ID = protocol.NewAttachmentID()
					attachment.Spec.Worktree = session.Worktree
					body, err := encode(attachment)
					if err != nil {
						return err
					}
					if _, err = db.ExecContext(ctx, "INSERT INTO attachments(id,worktree,caller,body) VALUES(?1,?2,?3,?4)", attachment.ID.String(), session.Worktree.String(), node[:], body); err != nil {
						return err
					}
					second, err := send("guide_turn", "Second", attachment.ID)
					if err != nil {
						return err
					}
					// An attachment that is not stored yet cannot be sent.
					if _, err = send("guide_turn", "Uploading", protocol.NewAttachmentID()); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
						return fmt.Errorf("guidance with a missing attachment was admitted: %v", err)
					}
					third, err := send("guide_turn", "Third")
					if err != nil {
						return err
					}
					if err = order([]protocol.TurnID{first, second, third, waiting}, &turn, &turn, &turn, nil); err != nil {
						return err
					}
					batch, _, err := TakeGuidance(ctx, db, turn)
					if err != nil {
						return err
					}
					if got := texts(batch); fmt.Sprint(got) != "[First Second Third]" {
						return fmt.Errorf("batch = %v", got)
					}
					if len(batch[1].Message.Attachments) != 1 || batch[1].Message.Attachments[0] != attachment.ID {
						return errors.New("guidance lost its attachment")
					}
					bound, err := ListTurnAttachments(ctx, db, second)
					if err != nil || len(bound) != 1 {
						return fmt.Errorf("attachment is not bound to its own message: %v", err)
					}
					for _, id := range []protocol.TurnID{first, second, third} {
						run, err := ReadAgentRun(ctx, db, id)
						if err != nil {
							return err
						}
						if run.Status != protocol.ConversationStatusCompleted || run.Joined == nil || *run.Joined != turn {
							return errors.New("joined guidance is not recorded with its turn")
						}
					}
					if err = order([]protocol.TurnID{waiting}, nil); err != nil {
						return err
					}
					// Guidance sent after the batch was taken waits for the next one.
					if _, err = send("guide_turn", "Fourth"); err != nil {
						return err
					}
					if batch, _, err = TakeGuidance(ctx, db, turn); err != nil || fmt.Sprint(texts(batch)) != "[Fourth]" {
						return fmt.Errorf("later guidance = %v: %v", texts(batch), err)
					}
					if batch, _, err = TakeGuidance(ctx, db, turn); err != nil || len(batch) != 0 {
						return fmt.Errorf("guidance was delivered twice: %v", err)
					}
				case "other settings stay ordinary":
					first, err := send("guide_turn", "Same settings")
					if err != nil {
						return err
					}
					current, err := ReadSession(ctx, db, session.ID)
					if err != nil {
						return err
					}
					config := current.Config
					changed := protocol.PermissionProject
					if config.Permission == changed {
						changed = protocol.PermissionAsk
					}
					config.Permission = changed
					if _, err = command("set_session_config", &protocol.CommandSetSessionConfigPayload{Session: session.ID, ExpectedRevision: current.Revision, Config: config}); err != nil {
						return err
					}
					other, err := send("guide_turn", "Other settings")
					if err != nil {
						return err
					}
					if err = order([]protocol.TurnID{first, other}, &turn, nil); err != nil {
						return err
					}
					if _, item, _ := queueFixtureItem(ctx, db, session.ID, other); !item.Ready {
						return errors.New("input with other settings is not ready to start")
					}
					batch, _, err := TakeGuidance(ctx, db, turn)
					if err != nil || fmt.Sprint(texts(batch)) != "[Same settings]" {
						return fmt.Errorf("batch = %v: %v", texts(batch), err)
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil {
						return err
					}
					if next == nil || next.Run.Turn != other || next.Turn.Config.Permission != changed {
						return errors.New("input with other settings did not start with its own settings")
					}
				case "outlived guidance starts one turn":
					waiting, err := send("queue_turn", "Ordinary")
					if err != nil {
						return err
					}
					if _, err = command("start_queued_turn", &protocol.CommandStartQueuedTurnPayload{Turn: waiting}); err != nil {
						return err
					}
					ids := []protocol.TurnID{}
					for _, text := range []string{"First", "Second", "Third"} {
						id, err := send("guide_turn", text)
						if err != nil {
							return err
						}
						ids = append(ids, id)
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "fixture failure"}, now); err != nil {
						return err
					}
					if err = order(append(ids, waiting), nil, nil, nil, nil); err != nil {
						return err
					}
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil {
						return err
					}
					if next == nil || next.Run.Turn != ids[0] || next.Message.Text != "First" {
						return errors.New("the first outlived input did not start")
					}
					// The rest of the batch waits for that turn; the ordinary item does not.
					if err = order([]protocol.TurnID{ids[1], ids[2], waiting}, &ids[0], &ids[0], nil); err != nil {
						return err
					}
					if again, _, err := ClaimNext(ctx, db, now); err != nil || again != nil {
						return fmt.Errorf("batched input started beside its turn: %v", err)
					}
					batch, _, err := TakeGuidance(ctx, db, ids[0])
					if err != nil || fmt.Sprint(texts(batch)) != "[Second Third]" {
						return fmt.Errorf("successor batch = %v: %v", texts(batch), err)
					}
					for _, id := range ids[1:] {
						run, err := ReadAgentRun(ctx, db, id)
						if err != nil {
							return err
						}
						if run.Joined == nil || *run.Joined != ids[0] || run.Guidance == nil || *run.Guidance != turn {
							return errors.New("batched input lost its origin or its turn")
						}
					}
					if err = queueFixtureInput(ctx, db, session.ID, ids[0], now); err != nil {
						return err
					}
					if _, _, err = FinishTurn(ctx, db, ids[0], protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if next, _, err = ClaimNext(ctx, db, now); err != nil || next == nil || next.Run.Turn != waiting {
						return fmt.Errorf("the ordinary item did not keep its own turn: %v", err)
					}
				default:
					held, err := send("queue_turn", "Held")
					if err != nil {
						return err
					}
					guided, err := send("guide_turn", "Guided")
					if err != nil {
						return err
					}
					// Two clients: the second edit carries a stale item revision.
					stale := revision(guided)
					if _, err = command("edit_queued_turn", &protocol.CommandEditQueuedTurnPayload{Turn: guided, ExpectedRevision: stale, Message: protocol.ConversationInput{Text: "Edited"}}); err != nil {
						return err
					}
					if _, err = command("edit_queued_turn", &protocol.CommandEditQueuedTurnPayload{Turn: guided, ExpectedRevision: stale, Message: protocol.ConversationInput{Text: "Lost update"}}); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
						return fmt.Errorf("stale edit = %v", err)
					}
					// An edited item no longer waits for the turn; it is guided again explicitly.
					if err = order([]protocol.TurnID{guided, held}, nil, nil); err != nil {
						return err
					}
					if _, err = command("guide_queued_turn", &protocol.CommandGuideQueuedTurnPayload{Turn: guided, Recipient: turn, ExpectedRevision: revision(guided)}); err != nil {
						return err
					}
					if _, err = command("guide_queued_turn", &protocol.CommandGuideQueuedTurnPayload{Turn: held, Recipient: turn, ExpectedRevision: revision(held)}); err != nil {
						return err
					}
					if err = order([]protocol.TurnID{guided, held}, &turn, &turn); err != nil {
						return err
					}
					if _, err = command("withdraw_guidance", &protocol.CommandWithdrawGuidancePayload{Turn: held, ExpectedRevision: revision(held)}); err != nil {
						return err
					}
					if _, item, _ := queueFixtureItem(ctx, db, session.ID, held); item == nil || item.Guidance != nil || item.Ready {
						return errors.New("withdrawn guidance is not a held item")
					}
					if _, err = command("withdraw_guidance", &protocol.CommandWithdrawGuidancePayload{Turn: held, ExpectedRevision: revision(held)}); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
						return fmt.Errorf("second withdrawal = %v", err)
					}
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if _, err = command("move_queued_turn", &protocol.CommandMoveQueuedTurnPayload{Session: session.ID, ExpectedRevision: queue.Revision, Turn: held, Before: &guided}); err != nil {
						return err
					}
					if _, err = command("move_queued_turn", &protocol.CommandMoveQueuedTurnPayload{Session: session.ID, ExpectedRevision: queue.Revision, Turn: guided, Before: &held}); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
						return fmt.Errorf("stale move = %v", err)
					}
					if err = order([]protocol.TurnID{held, guided}, nil, &turn); err != nil {
						return err
					}
					// Pausing returns waiting guidance to the queue and holds everything.
					if queue, err = ReadQueue(ctx, db, session.ID); err != nil {
						return err
					}
					if _, err = command("set_queue_paused", &protocol.CommandSetQueuePausedPayload{Session: session.ID, ExpectedRevision: queue.Revision, Paused: true}); err != nil {
						return err
					}
					if batch, _, err := TakeGuidance(ctx, db, turn); err != nil || len(batch) != 0 {
						return fmt.Errorf("a paused queue delivered guidance: %v", err)
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if next, _, err := ClaimNext(ctx, db, now); err != nil || next != nil {
						return fmt.Errorf("a paused queue started input: %v", err)
					}
					// A session with waiting input cannot be deleted; archiving keeps the queue.
					current, err := ReadSession(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if _, _, err = removeSession(ctx, db, session.ID, current.Revision); !providerFixtureFault(err, protocol.ErrorCodeBusy) {
						return fmt.Errorf("session with queued input was deleted: %v", err)
					}
					if _, _, err = archiveSession(ctx, db, protocol.CommandSetSessionArchivedPayload{Session: session.ID, ExpectedRevision: current.Revision, Archived: true}); err != nil {
						return err
					}
					if err = order([]protocol.TurnID{held, guided}, nil, nil); err != nil {
						return err
					}
					if _, err = command("remove_queued_turn", &protocol.CommandRemoveQueuedTurnPayload{Turn: held, ExpectedRevision: revision(held)}); err != nil {
						return err
					}
					if queue, err = ReadQueue(ctx, db, session.ID); err != nil {
						return err
					}
					if _, err = command("set_queue_paused", &protocol.CommandSetQueuePausedPayload{Session: session.ID, ExpectedRevision: queue.Revision, Paused: false}); err != nil {
						return err
					}
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil {
						return err
					}
					if next == nil || next.Run.Turn != guided || next.Message.Text != "Edited" {
						return errors.New("the resumed queue did not start the edited item")
					}
					// A compaction item is not guidance and has no editable input.
					if current, err = ReadSession(ctx, db, session.ID); err != nil {
						return err
					}
					output, err := command("compact_context", &protocol.CommandCompactContextPayload{Session: session.ID, ExpectedRevision: current.Revision})
					if err != nil {
						return err
					}
					compaction := output.Data.(protocol.QueuedTurn).ID
					if _, err = command("guide_queued_turn", &protocol.CommandGuideQueuedTurnPayload{Turn: compaction, Recipient: guided, ExpectedRevision: revision(compaction)}); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
						return fmt.Errorf("guided compaction = %v", err)
					}
					if again, _, err := ClaimNext(ctx, db, now); err != nil || again != nil {
						return fmt.Errorf("compaction started beside the running turn: %v", err)
					}
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
