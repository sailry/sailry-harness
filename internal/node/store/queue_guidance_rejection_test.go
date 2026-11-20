package store

import (
	"database/sql"
	"errors"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestQueuedGuidanceRejections(t *testing.T) {
	t.Parallel()
	storage, node, session, recipient, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := queueFixtureInput(ctx, db, session.ID, recipient, now); err != nil {
			return err
		}
		command := func(kind string, data any) (protocol.Output, error) {
			return queueFixtureCommand(ctx, db, storage, node, kind, data, now)
		}
		out, err := command("queue_turn", &protocol.CommandQueueTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Keep this queued"}})
		if err != nil {
			return err
		}
		turn := out.Data.(protocol.QueuedTurn).ID
		_, item, err := queueFixtureItem(ctx, db, session.ID, turn)
		if err != nil || item == nil {
			return fmt.Errorf("missing fixture queue item: %v", err)
		}
		rejected := func(target protocol.TurnID, revision uint64, want string, wantRace bool) error {
			before, err := ReadQueue(ctx, db, session.ID)
			if err != nil {
				return err
			}
			_, err = command("guide_queued_turn", &protocol.CommandGuideQueuedTurnPayload{Turn: turn, Recipient: target, ExpectedRevision: revision})
			failure, ok := errors.AsType[*protocol.Fault](err)
			if !ok {
				return fmt.Errorf("guidance returned %v, want %s", err, want)
			}
			if reason, race := QueueRejection("guide_queued_turn", failure); race != wantRace || reason != want {
				return fmt.Errorf("guidance returned %v (%s, race=%v), want %s, race=%v", err, reason, race, want, wantRace)
			}
			after, err := ReadQueue(ctx, db, session.ID)
			if err != nil || !reflect.DeepEqual(before, after) {
				return fmt.Errorf("%s changed the queue: %+v: %v", want, after, err)
			}
			return nil
		}
		if err = rejected(turn, item.Revision, "recipient_is_source", false); err != nil {
			return err
		}
		other, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &session.Config})
		if err != nil {
			return err
		}
		foreign, err := delegationFixtureParent(ctx, db, node, other.Data.(protocol.Session), now)
		if err != nil {
			return err
		}
		if _, err = command("stop_turn", &protocol.CommandStopTurnPayload{Turn: foreign}); err != nil {
			return err
		}
		if err = rejected(foreign, item.Revision, "guidance_scope_mismatch", false); err != nil {
			return err
		}
		if err = rejected(recipient, item.Revision+1, "queue_revision_changed", true); err != nil {
			return err
		}
		if _, err = command("stop_turn", &protocol.CommandStopTurnPayload{Turn: recipient}); err != nil {
			return err
		}
		if err = rejected(recipient, item.Revision, "recipient_stopping", true); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, recipient, protocol.ConversationStatusCancelled, nil, now); err != nil {
			return err
		}
		if err = rejected(recipient, item.Revision, "recipient_not_running", true); err != nil {
			return err
		}
		if _, err = command("remove_queued_turn", &protocol.CommandRemoveQueuedTurnPayload{Turn: turn, ExpectedRevision: item.Revision}); err != nil {
			return err
		}
		return rejected(recipient, item.Revision, "queue_item_not_pending", true)
	})
	if err != nil {
		t.Fatal(err)
	}
}
