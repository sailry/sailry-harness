package store

import (
	"database/sql"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestReadConversationChildRequiresCurrentParentMembership(t *testing.T) {
	storage, node, parent, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		call, err := delegationFixtureCall(ctx, db, turn, "spawn_agent", map[string]any{"task": "Background child", "background": true}, now)
		if err != nil {
			return err
		}
		child, _, err := DelegateAgent(ctx, db, node, turn, call, nil, true, now)
		if err != nil {
			return err
		}
		selected, found, err := ReadConversationChild(ctx, db, parent.ID, child.Run.Turn)
		if err != nil {
			return err
		}
		if !found || selected.Run.Turn != child.Run.Turn || selected.Origin.Turn != turn {
			return errors.New("current parent lost its delegated run")
		}
		if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		fork, _, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: parent.ID, Through: turn, ExpectedRevision: parent.Revision})
		if err != nil {
			return err
		}
		revision, err := historyRevision(ctx, db, parent.ID)
		if err != nil {
			return err
		}
		if _, _, err = rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{
			Session: parent.ID, ExpectedRevision: revision, ExpectedHead: turn,
		}, false); err != nil {
			return err
		}
		if _, err = ReadAgentRun(ctx, db, child.Run.Turn); err != nil {
			return err
		}
		_, found, err = ReadConversationChild(ctx, db, parent.ID, child.Run.Turn)
		if err != nil {
			return err
		}
		if found {
			return errors.New("rewound child origin was still eligible for publication")
		}
		selected, found, err = ReadConversationChild(ctx, db, fork.ID, child.Run.Turn)
		if err != nil || !found || selected.Origin.Session != parent.ID || selected.Origin.Turn != turn {
			return errors.Join(err, errors.New("fork lost its own visible membership of the same origin"))
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
