package store

import (
	"database/sql"
	"errors"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestChildQueueWithdrawalAndRemoval(t *testing.T) {
	t.Parallel()
	for _, action := range []string{"remove pending", "withdraw then remove", "already consumed"} {
		t.Run(action, func(t *testing.T) {
			t.Parallel()
			storage, node, _, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": "Child task"}, now)
				if err != nil {
					return err
				}
				child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
				if err != nil {
					return err
				}
				inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now)
				if err != nil || inv == nil {
					return fmt.Errorf("child was not claimed: %v", err)
				}
				if err = queueFixtureInput(ctx, db, child.Session.ID, child.Run.Turn, now); err != nil {
					return err
				}
				command := func(kind string, data any) (protocol.Output, error) {
					return queueFixtureCommand(ctx, db, storage, node, kind, data, now)
				}
				input := protocol.DefaultConversationInput()
				input.Text = "Pending child direction"
				output, err := command("guide_turn", &protocol.CommandGuideTurnPayload{Recipient: child.Run.Turn, ExpectedRevision: inv.Run.Revision, Message: input})
				if err != nil {
					return err
				}
				turn := output.Data.(protocol.QueuedTurn).ID
				queue, item, err := queueFixtureItem(ctx, db, child.Session.ID, turn)
				if err != nil || item == nil || item.Guidance == nil || *item.Guidance != child.Run.Turn {
					return fmt.Errorf("child guidance was not queued: %+v: %v", item, err)
				}
				revision := item.Revision
				for _, denied := range []protocol.Command{
					{Kind: "edit_queued_turn", Data: &protocol.CommandEditQueuedTurnPayload{Turn: turn, ExpectedRevision: revision, Message: protocol.ConversationInput{Text: "Changed child task"}}},
					{Kind: "send_queued_turn", Data: &protocol.CommandSendQueuedTurnPayload{Turn: turn, ExpectedRevision: revision}},
					{Kind: "guide_queued_turn", Data: &protocol.CommandGuideQueuedTurnPayload{Turn: turn, ExpectedRevision: revision, Recipient: child.Run.Turn}},
					{Kind: "move_queued_turn", Data: &protocol.CommandMoveQueuedTurnPayload{Session: child.Session.ID, Turn: turn, ExpectedRevision: queue.Revision}},
					{Kind: "set_queue_paused", Data: &protocol.CommandSetQueuePausedPayload{Session: child.Session.ID, ExpectedRevision: queue.Revision, Paused: true}},
				} {
					if _, err = command(denied.Kind, denied.Data); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
						return fmt.Errorf("child %s was not denied: %v", denied.Kind, err)
					}
					unchanged, err := ReadQueue(ctx, db, child.Session.ID)
					if err != nil || !reflect.DeepEqual(unchanged, queue) {
						return fmt.Errorf("denied %s changed the queue: %+v: %v", denied.Kind, unchanged, err)
					}
				}
				for _, stale := range []protocol.Command{
					{Kind: "withdraw_guidance", Data: &protocol.CommandWithdrawGuidancePayload{Turn: turn, ExpectedRevision: revision + 1}},
					{Kind: "remove_queued_turn", Data: &protocol.CommandRemoveQueuedTurnPayload{Turn: turn, ExpectedRevision: revision + 1}},
				} {
					if _, err = command(stale.Kind, stale.Data); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
						return fmt.Errorf("stale child %s was not rejected: %v", stale.Kind, err)
					}
				}
				if action == "already consumed" {
					joined, _, err := TakeGuidance(ctx, db, child.Run.Turn)
					if err != nil || len(joined) != 1 || joined[0].Turn.ID != turn {
						return fmt.Errorf("child guidance was not consumed: %+v: %v", joined, err)
					}
					for _, stale := range []protocol.Command{
						{Kind: "withdraw_guidance", Data: &protocol.CommandWithdrawGuidancePayload{Turn: turn, ExpectedRevision: revision}},
						{Kind: "remove_queued_turn", Data: &protocol.CommandRemoveQueuedTurnPayload{Turn: turn, ExpectedRevision: revision}},
					} {
						if _, err = command(stale.Kind, stale.Data); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
							return fmt.Errorf("consumed child %s was not rejected: %v", stale.Kind, err)
						}
					}
				} else {
					if action == "withdraw then remove" {
						output, err = command("withdraw_guidance", &protocol.CommandWithdrawGuidancePayload{Turn: turn, ExpectedRevision: revision})
						if err != nil {
							return err
						}
						update := output.Data.(protocol.ConversationQueueUpdate)
						if update.Session != child.Session.ID || update.Queue.Revision <= queue.Revision || len(update.Queue.Items) != 1 {
							return fmt.Errorf("withdrawal lost its queue update: %+v", update)
						}
						withdrawn := update.Queue.Items[0]
						if withdrawn.Turn != turn || withdrawn.Guidance != nil || withdrawn.Ready || withdrawn.Revision != revision+1 {
							return fmt.Errorf("withdrawal did not retain an unaddressed input: %+v", withdrawn)
						}
						if joined, _, err := TakeGuidance(ctx, db, child.Run.Turn); err != nil || len(joined) != 0 {
							return fmt.Errorf("withdrawn guidance was consumed: %+v: %v", joined, err)
						}
						full, err := ReadAgentCommand(ctx, db, node, protocol.Command{Kind: "read_queued_turn", Data: &protocol.CommandReadQueuedTurnPayload{Turn: turn}})
						if err != nil || !reflect.DeepEqual(full.Data.(protocol.ConversationQueuedMessage).Message, input) {
							return fmt.Errorf("withdrawal lost the original input: %v", err)
						}
						revision = withdrawn.Revision
					}
					output, err = command("remove_queued_turn", &protocol.CommandRemoveQueuedTurnPayload{Turn: turn, ExpectedRevision: revision})
					if err != nil {
						return err
					}
					update := output.Data.(protocol.ConversationQueueUpdate)
					if update.Session != child.Session.ID || len(update.Queue.Items) != 0 || len(update.Runs) != 1 || update.Runs[0].Turn != turn || update.Runs[0].Status != protocol.ConversationStatusCancelled {
						return fmt.Errorf("removal did not cancel only the pending input: %+v", update)
					}
				}
				for _, active := range []protocol.TurnID{parent, child.Run.Turn} {
					run, err := ReadAgentRun(ctx, db, active)
					if err != nil || run.Status != protocol.ConversationStatusRunning {
						return fmt.Errorf("queue command stopped an active owner: %+v: %v", run, err)
					}
				}
				if pending, err := PendingGuidance(ctx, db, child.Run.Turn); err != nil || pending {
					return errors.New("removed or consumed input stayed addressed to the child")
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
