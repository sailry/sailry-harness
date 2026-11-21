package store

import (
	"database/sql"
	"errors"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDetachedGuidanceCaptureProjection(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"released", "claimed before refresh", "completed before refresh", "readdressed before refresh"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			storage, node, session, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if err := queueFixtureInput(ctx, db, session.ID, parent, now); err != nil {
					return err
				}
				command := func(kind string, data any) (protocol.Output, error) {
					return queueFixtureCommand(ctx, db, storage, node, kind, data, now)
				}
				output, err := command("guide_turn", &protocol.CommandGuideTurnPayload{
					Recipient: parent, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Guidance input"},
				})
				if err != nil {
					return err
				}
				guided := output.Data.(protocol.QueuedTurn).ID
				initial, err := ReadConversationSnapshot(ctx, db, session.ID, 20, nil)
				if err != nil {
					return err
				}
				initial.Node = node
				projection := client.NewConversationProjection(node, session.ID, 1)
				if applied, err := projection.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: initial}); err != nil || applied != client.Applied {
					return fmt.Errorf("initial guidance projection: %v, %v", applied, err)
				}
				before, item, err := queueFixtureItem(ctx, db, session.ID, guided)
				if err != nil || item == nil || item.Guidance == nil {
					return fmt.Errorf("guidance was not pending: %+v, %v", item, err)
				}
				config := session.Config
				config.Permission = protocol.PermissionProject
				if _, err = command("set_session_config", &protocol.CommandSetSessionConfigPayload{Session: session.ID, ExpectedRevision: session.Revision, Config: config}); err != nil {
					return err
				}
				if _, err = command("stop_turn", &protocol.CommandStopTurnPayload{Turn: parent}); err != nil {
					return err
				}
				if _, _, err = FinishTurn(ctx, db, parent, protocol.ConversationStatusCancelled, nil, now); err != nil {
					return err
				}
				after, released, err := queueFixtureItem(ctx, db, session.ID, guided)
				if err != nil || released == nil {
					return fmt.Errorf("released guidance was lost: %v", err)
				}
				if after.Revision <= before.Revision || released.Revision <= item.Revision || released.ConfigRevision != session.Revision+1 || released.Guidance != nil {
					return fmt.Errorf("recapture was not versioned: before %+v, after %+v", before, after)
				}
				if _, err = command("send_queued_turn", &protocol.CommandSendQueuedTurnPayload{Turn: guided, ExpectedRevision: item.Revision}); err == nil {
					return errors.New("pre-recapture item revision was accepted")
				} else if refusal, ok := errors.AsType[*protocol.Fault](err); !ok || refusal.Code != protocol.ErrorCodeRevisionConflict {
					return fmt.Errorf("stale item revision returned another failure: %v", err)
				}
				claim := func(turn protocol.TurnID, revision uint64) error {
					if _, err := command("send_queued_turn", &protocol.CommandSendQueuedTurnPayload{Turn: turn, ExpectedRevision: revision}); err != nil {
						return err
					}
					invocation, _, err := ClaimNext(ctx, db, now)
					if err != nil || invocation == nil || invocation.Run.Turn != turn {
						return fmt.Errorf("expected turn did not start: %+v, %v", invocation, err)
					}
					return nil
				}
				switch scenario {
				case "claimed before refresh", "completed before refresh":
					if err = claim(guided, released.Revision); err != nil {
						return err
					}
					if scenario == "completed before refresh" {
						if err = queueFixtureInput(ctx, db, session.ID, guided, now); err != nil {
							return err
						}
						if _, _, err = FinishTurn(ctx, db, guided, protocol.ConversationStatusCompleted, nil, now); err != nil {
							return err
						}
					}
				case "readdressed before refresh":
					output, err := command("submit_turn", &protocol.CommandSubmitTurnPayload{
						Session: session.ID, ExpectedRevision: session.Revision + 1, Message: protocol.ConversationInput{Text: "Replacement task"},
					})
					if err != nil {
						return err
					}
					target := output.Data.(protocol.QueuedTurn).ID
					_, pending, err := queueFixtureItem(ctx, db, session.ID, target)
					if err != nil || pending == nil {
						return fmt.Errorf("replacement task was not pending: %v", err)
					}
					if err = claim(target, pending.Revision); err != nil {
						return err
					}
					if _, err = command("guide_queued_turn", &protocol.CommandGuideQueuedTurnPayload{Turn: guided, Recipient: target, ExpectedRevision: released.Revision}); err != nil {
						return err
					}
				}
				activity, err := ReadConversationActivity(ctx, db, session.ID, 20)
				if err != nil {
					return err
				}
				publish := func(kind string, data any) error {
					frame := protocol.ConversationFrame{Node: node, Session: session.ID, Sequence: projection.Snapshot().Sequence + 1, Change: protocol.ConversationChange{Kind: kind, Data: data}}
					applied, err := projection.Apply(1, protocol.Update{Kind: "conversation_frame", Data: frame})
					if err != nil || applied != client.Applied {
						return fmt.Errorf("publish %s after recapture: %v, %v", kind, applied, err)
					}
					return nil
				}
				for _, run := range activity.Runs {
					if err = publish("run", run); err != nil {
						return err
					}
				}
				if err = publish("queue", activity.Queue); err != nil {
					return err
				}
				if !reflect.DeepEqual(projection.Snapshot().Page.Queue, activity.Queue) {
					return errors.New("projection retained an obsolete queue capture")
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}
