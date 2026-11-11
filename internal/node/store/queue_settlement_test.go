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
	output, _, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, node, request, now)
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
					if _, err = held(ctx, db, session.ID, guided, now); err != nil {
						return err
					}
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
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
					if _, err = RecoverRuns(ctx, db, now); err != nil {
						return err
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
