package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProjectShellInventoryOrder(t *testing.T) {
	t.Parallel()
	storage, node, session, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		terminal := protocol.TerminalInfo{ID: protocol.NewTerminalID(), Worktree: &session.Worktree, Status: protocol.TerminalStatus{Kind: "running"}, Revision: 1}
		changes, err := SaveTerminal(ctx, db, terminal, true)
		if err != nil {
			return err
		}
		if len(changes) != 1 {
			return fmt.Errorf("starting a terminal produced %d inventory changes", len(changes))
		}
		created := changes[0].Data.(protocol.TerminalInfo)
		later, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &session.Config})
		if err != nil {
			return err
		}
		if created.CreationOrder <= session.CreationOrder || created.CreationOrder >= later.Data.(protocol.Session).CreationOrder || created.UpdatedMs <= 0 {
			return fmt.Errorf("terminal not ordered between earlier and later sessions: %+v", created)
		}
		// Give the stored shell a known earlier timestamp so an activity change
		// can be checked without waiting for the wall clock to advance.
		created.UpdatedMs = 1
		body, err := encode(created)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "UPDATE terminals SET body=?1 WHERE id=?2", body, created.ID.String()); err != nil {
			return err
		}
		changes, err = SaveTerminal(ctx, db, terminal, true)
		if err != nil {
			return err
		}
		if len(changes) != 0 {
			return fmt.Errorf("identical terminal advanced its activity time: %+v", changes)
		}
		terminal.Revision++ // Taking over the same shell does not run a command.
		terminal.Owner = &node
		changes, err = SaveTerminal(ctx, db, terminal, true)
		if err != nil {
			return err
		}
		if len(changes) != 1 || changes[0].Data.(protocol.TerminalInfo).UpdatedMs != created.UpdatedMs {
			return fmt.Errorf("terminal ownership change moved the shell in the sidebar: %+v", changes)
		}
		terminal.Title = new("build")
		changes, err = SaveTerminal(ctx, db, terminal, true)
		if err != nil {
			return err
		}
		if len(changes) != 1 {
			return fmt.Errorf("terminal title change produced %d inventory changes", len(changes))
		}
		updated := changes[0].Data.(protocol.TerminalInfo)
		if updated.UpdatedMs <= created.UpdatedMs || updated.CreationOrder != created.CreationOrder {
			return fmt.Errorf("terminal update changed creation order or lost activity: %+v", updated)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
