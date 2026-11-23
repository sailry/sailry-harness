package store

import (
	"database/sql"
	"fmt"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHistoryPacksShortInterleavedTurnsOnOlderPages(t *testing.T) {
	t.Parallel()
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

func TestHistoryPagesKeepGuidanceWithItsRecipient(t *testing.T) {
	t.Parallel()
	storage, node, session, recipient, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		appendMessage := func(turn protocol.TurnID, role canonical.Role, text string) error {
			record := canonical.NewRecord(turn, "message", canonical.NewMessage(role, canonical.Text(0, text)), now)
			record.Provenance.Author = string(role)
			_, _, err := AppendCanonical(ctx, db, session.ID, record)
			return err
		}
		if err := appendMessage(recipient, canonical.User, "Task"); err != nil {
			return err
		}
		member, err := queueFixtureCommand(ctx, db, storage, node, "submit_turn", &protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "A separate task"},
		}, now)
		if err != nil {
			return err
		}
		independent := member.Data.(protocol.QueuedTurn).ID
		member, err = queueFixtureCommand(ctx, db, storage, node, "guide_turn", &protocol.CommandGuideTurnPayload{
			Recipient: recipient, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Joined guidance"},
		}, now)
		if err != nil {
			return err
		}
		guidance := member.Data.(protocol.QueuedTurn).ID
		joined, _, err := TakeGuidance(ctx, db, recipient)
		if err != nil || len(joined) != 1 || joined[0].Turn.ID != guidance {
			return fmt.Errorf("guidance was not consumed: %v", err)
		}
		if err = appendMessage(guidance, canonical.User, joined[0].Message.Text); err != nil {
			return err
		}
		if err = appendMessage(recipient, canonical.Assistant, "Recipient reply"); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, recipient, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		invocation, _, err := ClaimNext(ctx, db, now)
		if err != nil || invocation == nil || invocation.Run.Turn != independent {
			return fmt.Errorf("independent task was not claimed: %v", err)
		}
		if err = appendMessage(independent, canonical.User, invocation.Message.Text); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, independent, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}

		checkHead := func(limit uint16) error {
			head, err := ReadConversation(ctx, db, session.ID, nil, limit)
			if err != nil {
				return err
			}
			if len(head.Page.Runs) != 1 || head.Page.Runs[0].Turn != independent ||
				head.Page.NextBefore == nil || *head.Page.NextBefore != independent {
				return fmt.Errorf("head retained an orphaned member or the wrong cursor: %+v", head.Page)
			}
			return nil
		}
		if err = checkHead(1); err != nil {
			return err
		}
		page, err := ReadConversation(ctx, db, session.ID, &independent, 1)
		if err != nil {
			return err
		}
		if len(page.Page.Runs) != 2 || page.Page.NextBefore != nil ||
			page.Page.Runs[0].Turn != recipient || page.Page.Runs[1].Turn != guidance ||
			!slices.ContainsFunc(page.Page.Entries, func(entry protocol.ConversationEntry) bool { return entry.Turn == guidance }) {
			return fmt.Errorf("older page split a group across an interleaved boundary: %+v", page.Page)
		}
		legacy, err := ReadConversation(ctx, db, session.ID, &guidance, 2)
		if err != nil {
			return err
		}
		if len(legacy.Page.Runs) != 2 || legacy.Page.Runs[0].Turn != recipient ||
			legacy.Page.Runs[1].Turn != independent || legacy.Page.NextBefore != nil {
			return fmt.Errorf("legacy cursor skipped an older turn or repeated itself: %+v", legacy.Page)
		}
		for range 100 {
			if err = appendMessage(independent, canonical.Assistant, "Latest output"); err != nil {
				return err
			}
		}
		// The entry bound left the earlier group entirely outside the head.
		return checkHead(20)
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestHistoryPagesKeepGuidanceFromAnEmptyCancelledRecipient(t *testing.T) {
	t.Parallel()
	storage, node, session, recipient, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		output, err := queueFixtureCommand(ctx, db, storage, node, "guide_turn", &protocol.CommandGuideTurnPayload{
			Recipient: recipient, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Visible guidance"},
		}, now)
		if err != nil {
			return err
		}
		guidance := output.Data.(protocol.QueuedTurn).ID
		if joined, _, e := TakeGuidance(ctx, db, recipient); e != nil || len(joined) != 1 || joined[0].Turn.ID != guidance {
			return fmt.Errorf("guidance was not consumed: %v", e)
		}
		record := canonical.NewRecord(guidance, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Visible guidance")), now)
		record.Provenance.Author = "user"
		if _, _, err = AppendCanonical(ctx, db, session.ID, record); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, recipient, protocol.ConversationStatusCancelled, nil, now); err != nil {
			return err
		}
		// Older records can leave a cancelled recipient without captured input.
		if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET input=NULL WHERE turn=?1", recipient.String()); err != nil {
			return err
		}
		newer, err := admitCredentialClaim(ctx, db, storage, node, node, session, now)
		if err != nil {
			return err
		}
		if err = setQueuePaused(ctx, db, session.ID, false); err != nil {
			return err
		}
		if invocation, _, e := ClaimNext(ctx, db, now); e != nil || invocation == nil || invocation.Run.Turn != newer.ID {
			return fmt.Errorf("newer task was not claimed: %v", e)
		}

		head, err := ReadConversation(ctx, db, session.ID, nil, 20)
		if err != nil {
			return err
		}
		if len(head.Page.Runs) != 2 || head.Page.Runs[0].Turn != guidance || head.Page.Runs[1].Turn != newer.ID {
			return fmt.Errorf("head duplicated or reattached the standalone guidance: %+v", head.Page.Runs)
		}
		head, err = ReadConversation(ctx, db, session.ID, nil, 1)
		if err != nil {
			return err
		}
		if head.Page.NextBefore == nil || *head.Page.NextBefore != newer.ID ||
			slices.ContainsFunc(head.Page.Runs, func(run protocol.ConversationRun) bool { return run.Turn == guidance }) {
			return fmt.Errorf("head brought in guidance through the cancelled skeleton: %+v", head.Page)
		}
		older, err := ReadConversation(ctx, db, session.ID, head.Page.NextBefore, 1)
		if err != nil {
			return err
		}
		if len(older.Page.Runs) != 1 || older.Page.Runs[0].Turn != guidance || older.Page.NextBefore != nil ||
			len(older.Page.Entries) != 1 || older.Page.Entries[0].Turn != guidance {
			return fmt.Errorf("older page lost standalone guidance: %+v", older.Page)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
