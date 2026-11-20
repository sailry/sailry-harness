package node

import (
	"database/sql"
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestArchiveRejectsUnfinishedTasks(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	ctx := t.Context()
	idle, busy := sessionFixture(t, node), sessionFixture(t, node)
	encode := func(value any) []byte {
		t.Helper()
		body, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		return body
	}
	run := protocol.DefaultConversationRun()
	run.Turn, run.Session, run.Worktree, run.Revision = protocol.NewTurnID(), busy.ID, busy.Worktree, busy.Revision
	run.Kind, run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusQueued
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: busy.ID, ExpectedRevision: busy.Revision, Message: protocol.ConversationInput{Text: "Held input"},
	}})
	caller := node.ID()
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		sequence, err := store.NextCounter(ctx, db, store.CounterTurns)
		if err != nil {
			return err
		}
		run.Sequence = uint64(sequence)
		if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", caller[:], request.ID.String(), encode(request)); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO turns(rowid,id,kind,plugins,roles,session,revision,request,caller) VALUES(?1,?2,'task',X'5B5D',?3,?4,?5,?6,?7)", sequence, run.Turn.String(), encode(busy.Roles), busy.ID.String(), busy.Revision, request.ID.String(), caller[:]); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)", busy.ID.String(), run.Turn.String()); err != nil {
			return err
		}
		// The synthetic durable turn is never claimed by the executor.
		_, err = db.ExecContext(ctx, "INSERT INTO agent_runs(turn,ready,body,hooks) VALUES(?1,0,?2,?3)", run.Turn.String(), encode(run), encode(protocol.DefaultHooksSettings()))
		return err
	}); err != nil {
		t.Fatal(err)
	}
	setRun := func(status protocol.ConversationStatus, waiting *protocol.ConversationRunWaiting) {
		t.Helper()
		run.Status, run.Waiting = status, waiting
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, "UPDATE agent_runs SET body=?2 WHERE turn=?1", run.Turn.String(), encode(run))
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	assertBlocked := func(t *testing.T) {
		t.Helper()
		before := snapshotFixture(t, node)
		for _, command := range []protocol.Command{
			{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: busy.ID, ExpectedRevision: busy.Revision, Archived: true}},
			{Kind: "archive_sessions", Data: protocol.CommandArchiveSessionsPayload{Expected: []protocol.SessionRevision{
				{Session: idle.ID, Revision: idle.Revision}, {Session: busy.ID, Revision: busy.Revision},
			}}},
		} {
			response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), command))
			if response.Fault == nil || response.Fault.Code != protocol.ErrorCodeBusy {
				t.Fatalf("%s accepted unfinished work: %+v", command.Kind, response)
			}
			after := snapshotFixture(t, node)
			if after.Cursor != before.Cursor {
				t.Fatal("rejected archive published an event")
			}
			for _, session := range after.Sessions {
				if session.Archived || session.Revision != 1 {
					t.Fatalf("rejected archive partially changed session: %+v", session)
				}
			}
		}
	}
	for _, scenario := range []struct {
		name    string
		status  protocol.ConversationStatus
		waiting *protocol.ConversationRunWaiting
	}{
		{"held queue", protocol.ConversationStatusQueued, nil},
		{"running", protocol.ConversationStatusRunning, nil},
		{"stopping", protocol.ConversationStatusStopping, nil},
		{"waiting for controller", protocol.ConversationStatusRunning, &protocol.ConversationRunWaiting{Kind: "controller_login"}},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			setRun(scenario.status, scenario.waiting)
			assertBlocked(t)
		})
	}
	setRun(protocol.ConversationStatusCompleted, nil)
	t.Run("background work after the turn", func(t *testing.T) {
		record := store.BackgroundRecord{Task: protocol.ConversationBackgroundTask{
			ID: "command:" + protocol.NewRequestID().String(), Session: busy.ID, Turn: run.Turn,
			Kind:  protocol.ConversationBackgroundKind{Kind: "command", Data: protocol.ConversationBackgroundKindCommandPayload{ID: protocol.NewRequestID()}},
			State: protocol.ConversationBackgroundStateRunning,
		}, FollowUp: true}
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, "INSERT INTO agent_background(id,session,turn,kind,source,state,sequence,body) VALUES(?1,?2,?3,'command','fixture','running',1,?4)", record.Task.ID, busy.ID.String(), run.Turn.String(), encode(record))
			return err
		}); err != nil {
			t.Fatal(err)
		}
		assertBlocked(t)
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, "DELETE FROM agent_background WHERE id=?1", record.Task.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	})
	archived := commandFixture(t, node, protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{
		Session: busy.ID, ExpectedRevision: busy.Revision, Archived: true,
	}}).Data.(protocol.Session)
	if !archived.Archived || archived.Revision != busy.Revision+1 {
		t.Fatalf("settled session was not archived: %+v", archived)
	}
	setRun(protocol.ConversationStatusStopping, nil)
	restored := commandFixture(t, node, protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{
		Session: busy.ID, ExpectedRevision: archived.Revision, Archived: false,
	}}).Data.(protocol.Session)
	if restored.Archived {
		t.Fatal("unfinished legacy work prevented restoring its archived session")
	}
}
