package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func NextChildGuidance(ctx context.Context, db *sql.Conn, recipient protocol.TurnID) (*protocol.ConversationRun, []protocol.Event, error) {
	run, err := ReadAgentRun(ctx, db, recipient)
	if err != nil {
		return nil, nil, err
	}
	origin, err := readDelegation(ctx, db, recipient)
	if err != nil {
		return nil, nil, err
	}
	if origin == nil || run.Status != protocol.ConversationStatusCompleted {
		return nil, nil, nil
	}
	var body []byte
	err = db.QueryRowContext(ctx, "SELECT a.body FROM agent_runs a JOIN agent_pending p ON p.turn=a.turn JOIN turns t ON t.id=a.turn JOIN agent_queues q ON q.session=t.session WHERE p.recipient=?1 AND t.session=?2 AND a.ready=1 AND q.paused=0 AND json_extract(a.body,'$.status')='queued' AND NOT EXISTS(SELECT 1 FROM agent_runs active JOIN turns other ON other.id=active.turn WHERE other.session=t.session AND json_extract(active.body,'$.status') IN ('running','stopping')) ORDER BY p.position,t.rowid LIMIT 1", recipient.String(), run.Session.String()).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil, nil
	}
	if err != nil {
		return nil, nil, err
	}
	var next protocol.ConversationRun
	if err = json.Unmarshal(body, &next); err != nil {
		return nil, nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET recipient=?2,revision=revision+1 WHERE recipient=?1 AND turn<>?2", recipient.String(), next.Turn.String()); err != nil {
		return nil, nil, err
	}
	if err = bumpQueue(ctx, db, run.Session); err != nil {
		return nil, nil, err
	}
	events, err := activityEvent(ctx, db, run.Session)
	return &next, events, err
}

func retainChildGuidance(ctx context.Context, db *sql.Conn, run protocol.ConversationRun) error {
	result, err := db.ExecContext(ctx, "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE (recipient=?1 OR turn=?1) AND recipient IS NOT NULL", run.Turn.String())
	if err != nil {
		return err
	}
	changed, err := result.RowsAffected()
	if err != nil || changed == 0 {
		return err
	}
	return bumpQueue(ctx, db, run.Session)
}

// guideDelegatedTurn admits controller guidance with the recipient's capture.
// Child settings are immutable even if its main session has changed meanwhile.
func guideDelegatedTurn(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, target protocol.ConversationRun, input protocol.CommandGuideTurnPayload, now time.Time) (protocol.Output, []protocol.Event, error) {
	if target.Kind != protocol.ConversationRunKindTask {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "guidance requires an active task recipient")
	}
	if err := checkRevision(target.Revision, input.ExpectedRevision); err != nil {
		return protocol.Output{}, nil, err
	}
	if err := validateAgentInput(input.Message); err != nil {
		return protocol.Output{}, nil, err
	}
	origin, err := readDelegation(ctx, db, target.Turn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if origin == nil {
		return protocol.Output{}, nil, invalid("guidance recipient is not a child")
	}
	var active bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status')='running')", origin.Session.String()).Scan(&active); err != nil {
		return protocol.Output{}, nil, err
	}
	if !active {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "parent session is no longer running")
	}
	capture, err := readTurnCapture(ctx, db, target.Turn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	child, err := ReadSession(ctx, db, capture.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	child.Config, child.Roles = capture.Config, capture.Roles
	if err = validateAgentReferences(ctx, db, child, input.Message, capture.Plugins); err != nil {
		return protocol.Output{}, nil, err
	}
	turn := capture
	turn.ID, turn.Request = protocol.NewTurnID(), request.ID
	sequence, err := NextCounter(ctx, db, CounterTurns)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	plugins, err := encode(turn.Plugins)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	roles, err := encode(turn.Roles)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO turns(id,kind,session,revision,request,caller,plugins,roles,instructions,rowid) SELECT ?1,'task',session,revision,?2,?3,?4,?5,instructions,?6 FROM turns WHERE id=?7", turn.ID.String(), request.ID.String(), caller[:], plugins, roles, sequence, target.Turn.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	association, err := encode(origin)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_delegations(turn,parent_turn,entry,part,body) VALUES(?1,?2,?3,?4,?5)", turn.ID.String(), origin.Turn.String(), origin.Entry, origin.Index, association); err != nil {
		return protocol.Output{}, nil, err
	}
	if err = bindAgentAttachments(ctx, db, turn.ID, target.Worktree, input.Message.Attachments); err != nil {
		return protocol.Output{}, nil, err
	}
	run := protocol.DefaultConversationRun()
	run.Turn, run.Session, run.Worktree, run.Kind, run.Sequence, run.Revision, run.Status = turn.ID, target.Session, target.Worktree, protocol.ConversationRunKindTask, sequence, target.Revision, protocol.ConversationStatusQueued
	body, err := encode(run)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	message, err := encode(input.Message)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_runs(turn,ready,body,provider,hooks,compaction_percent,input) SELECT ?1,1,?2,provider,hooks,compaction_percent,?3 FROM agent_runs WHERE turn=?4", turn.ID.String(), body, message, target.Turn.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)", target.Session.String(), turn.ID.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_pending(turn,position,revision,request,caller) SELECT ?1,coalesce(max(p.position),0)+1,1,?2,?3 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?4", turn.ID.String(), request.ID.String(), caller[:], target.Session.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	if err = bumpQueue(ctx, db, target.Session); err != nil {
		return protocol.Output{}, nil, err
	}
	if err = tagGuidance(ctx, db, turn.ID, target.Turn); err != nil {
		return protocol.Output{}, nil, err
	}
	events, err := activityEvent(ctx, db, target.Session)
	return protocol.Output{Kind: "queued_turn", Data: turn}, events, err
}
