package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestBackgroundNoticeRetainsMergedTurnOrigin(t *testing.T) {
	storage, node, session, first, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		origin, err := OriginalTurnUser(ctx, db, first)
		if err != nil {
			return err
		}
		if err = queueFixtureInput(ctx, db, session.ID, first, now); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, first, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if _, err = backgroundFixtureRecord(ctx, db, session, first, "merged-completion", true, now); err != nil {
			return err
		}
		target, err := CreateUser(ctx, db, "Merged owner")
		if err != nil {
			return err
		}
		if _, err = MergeIdentityUsers(ctx, db, origin.ID, target.ID); err != nil {
			return err
		}
		turn, _, err := admitBackground(ctx, db, node, session.ID, now)
		if err != nil {
			return err
		}
		if turn == nil {
			return fmt.Errorf("merged background notice was not admitted")
		}
		stored, err := OriginalTurnUser(ctx, db, turn.ID)
		if err != nil {
			return err
		}
		actor, err := TurnUser(ctx, db, turn.ID)
		if err != nil {
			return err
		}
		if stored.ID != origin.ID || actor.ID != target.ID {
			return fmt.Errorf("background origin=%s actor=%s", stored.ID, actor.ID)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestNoticeRunHistoryDoesNotExposeInput(t *testing.T) {
	storage, node, session, first, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		// User input remains visible before its first canonical record arrives.
		history, err := ReadConversation(ctx, db, session.ID, nil, 20)
		if err != nil {
			return err
		}
		if len(history.Page.Runs) != 1 || history.Page.Runs[0].Input == nil || history.Page.Runs[0].Input.Message.Text != "Fixture input" {
			return fmt.Errorf("ordinary pending input was lost: %+v", history.Page.Runs)
		}
		if err = queueFixtureInput(ctx, db, session.ID, first, now); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, first, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if _, err = backgroundFixtureRecord(ctx, db, session, first, "completed-background-task", true, now); err != nil {
			return err
		}
		admitted, _, err := admitBackground(ctx, db, node, session.ID, now)
		if err != nil {
			return err
		}
		if admitted == nil {
			return fmt.Errorf("background notice was not admitted")
		}
		check := func(status protocol.ConversationStatus) error {
			history, err := ReadConversation(ctx, db, session.ID, nil, 20)
			if err != nil {
				return err
			}
			var found bool
			for _, run := range history.Page.Runs {
				if run.Turn == admitted.ID {
					found = true
					if !run.Notice || run.Session != session.ID || run.Status != status || run.Input != nil {
						return fmt.Errorf("invalid public notice run: %+v", run)
					}
				}
			}
			if !found {
				return fmt.Errorf("notice run is missing from conversation history")
			}
			single, err := ReadConversationTurn(ctx, db, session.ID, admitted.ID, history.Page.Revision, nil, 20)
			if err != nil {
				return err
			}
			if !single.Run.Notice || single.Run.Input != nil || single.Run.Status != status {
				return fmt.Errorf("turn history exposed notice input: %+v", single.Run)
			}
			return nil
		}
		if err = check(protocol.ConversationStatusQueued); err != nil {
			return err
		}
		inv, _, err := ClaimNext(ctx, db, now)
		if err != nil {
			return err
		}
		if inv == nil || inv.Run.Turn != admitted.ID || !inv.Run.Notice {
			return fmt.Errorf("background notice was not claimed")
		}
		if err = check(protocol.ConversationStatusRunning); err != nil {
			return err
		}
		var retained bool
		if err = db.QueryRowContext(ctx, "SELECT input IS NOT NULL AND NOT EXISTS(SELECT 1 FROM agent_events WHERE turn=?1) FROM agent_runs WHERE turn=?1", admitted.ID.String()).Scan(&retained); err != nil {
			return err
		}
		if !retained {
			return fmt.Errorf("fixture no longer covers the interval before notice input becomes canonical")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
