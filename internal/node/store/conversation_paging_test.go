package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHistoryPacksShortInterleavedTurnsOnOlderPages(t *testing.T) {
	storage, node, session, long, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, _, err := FinishTurn(ctx, db, long, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		short, err := admitCredentialClaim(ctx, db, storage, node, node, session, now)
		if err != nil {
			return err
		}
		var entries []protocol.ConversationEntry
		for _, turn := range []protocol.TurnID{long, short.ID, long, long} {
			record := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "reply")), now)
			entry, _, err := AppendCanonical(ctx, db, session.ID, record)
			if err != nil {
				return err
			}
			entries = append(entries, entry)
		}
		turns := []protocol.TurnID{long, short.ID}
		head, more, err := historyEntries(ctx, db, session.ID, turns, nil, 2, false)
		if err != nil {
			return err
		}
		if !more || len(head) != 2 || head[0].ID != entries[2].ID || head[1].ID != entries[3].ID {
			return fmt.Errorf("live head lost the newest events: %v", head)
		}
		page, more, err := historyEntries(ctx, db, session.ID, turns, nil, 2, true)
		if err != nil {
			return err
		}
		if !more || len(page) != 2 || page[0].ID != entries[1].ID || page[1].ID != entries[3].ID {
			return fmt.Errorf("older page did not pack the complete short turn and newest long-turn entry: %v", page)
		}
		rest, more, err := historyEntries(ctx, db, session.ID, turns[:1], &page[1].Sequence, 2, true)
		if err != nil {
			return err
		}
		if more || len(rest) != 2 || rest[0].ID != entries[0].ID || rest[1].ID != entries[2].ID {
			return fmt.Errorf("gap in long turn: %v", rest)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
