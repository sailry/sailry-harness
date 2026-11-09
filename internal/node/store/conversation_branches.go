package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func insertBranchSession(ctx context.Context, db *sql.Conn, previous protocol.Session, worktree protocol.WorktreeID) (protocol.Session, error) {
	value := previous
	value.ID = protocol.NewSessionID()
	value.Revision = 1
	value.Archived = false
	value.Worktree = worktree
	value.Fork = nil
	var err error
	value.CreationOrder, err = NextCounter(ctx, db, CounterSessions)
	if err != nil {
		return value, err
	}
	var project any
	if value.Project != nil {
		project = value.Project.String()
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO sessions(rowid,id,project,worktree,revision) VALUES(?1,?2,?3,?4,1)", value.CreationOrder, value.ID.String(), project, worktree.String()); err != nil {
		return value, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO session_revisions(session,revision,worktree,config,profile,roles) SELECT ?1,1,?2,config,profile,roles FROM session_revisions WHERE session=?3 AND revision=?4", value.ID.String(), worktree.String(), previous.ID.String(), previous.Revision); err != nil {
		return value, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) SELECT ?1,key,body FROM agent_state WHERE owner=?2 AND key=?3", value.ID.String(), previous.ID.String(), sessionTitleKey); err != nil {
		return value, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO session_media(session,body) SELECT ?1,body FROM session_media WHERE session=?2", value.ID.String(), previous.ID.String())
	return value, err
}
func forkConversation(ctx context.Context, db *sql.Conn, input protocol.CommandForkConversationPayload) (protocol.Session, []protocol.Event, error) {
	previous, err := checkedSession(ctx, db, input.Session, input.ExpectedRevision)
	if err != nil {
		return protocol.Session{}, nil, err
	}
	boundary, err := visibleAgentRun(ctx, db, input.Session, input.Through)
	if err != nil {
		return protocol.Session{}, nil, err
	}
	if activeRunStatus(boundary.Status) {
		return protocol.Session{}, nil, fault(protocol.ErrorCodeBusy, "fork target has not finished")
	}
	var active bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=h.turn WHERE h.session=?1 AND t.rowid<=?2 AND json_extract(a.body,'$.status') IN ('running','stopping'))", input.Session.String(), boundary.Sequence).Scan(&active); err != nil {
		return protocol.Session{}, nil, err
	}
	if active {
		return protocol.Session{}, nil, fault(protocol.ErrorCodeBusy, "fork history contains an active turn")
	}
	value, err := insertBranchSession(ctx, db, previous, previous.Worktree)
	if err != nil {
		return value, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO conversation_turns(session,turn) SELECT ?1,h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=h.turn WHERE h.session=?2 AND t.rowid<=?3 AND json_extract(a.body,'$.status')!='queued'", value.ID.String(), input.Session.String(), boundary.Sequence); err != nil {
		return value, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO session_forks(session,source,through_turn) VALUES(?1,?2,?3)", value.ID.String(), input.Session.String(), input.Through.String()); err != nil {
		return value, nil, err
	}
	if err = rebuildConversationMembership(ctx, db, value.ID); err != nil {
		return value, nil, err
	}
	if err = rebuildCanonicalState(ctx, db, value.ID); err != nil {
		return value, nil, err
	}
	value.Fork = &protocol.ConversationFork{Session: input.Session, Through: input.Through}
	value.Activity, err = readActivity(ctx, db, value.ID)
	return value, []protocol.Event{{Kind: "session_changed", Data: &value}}, err
}
func rebuildConversationMembership(ctx context.Context, db *sql.Conn, session protocol.SessionID) error {
	if _, err := db.ExecContext(ctx, "DELETE FROM conversation_events WHERE session=?1", session.String()); err != nil {
		return err
	}
	_, err := db.ExecContext(ctx, "INSERT INTO conversation_events(session,sequence) SELECT ?1,e.sequence FROM agent_events e JOIN conversation_turns h ON h.turn=e.turn WHERE h.session=?1", session.String())
	return err
}
func conversationHead(ctx context.Context, db *sql.Conn, session protocol.SessionID) (*protocol.TurnID, error) {
	var text string
	err := db.QueryRowContext(ctx, "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1", session.String()).Scan(&text)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	id, err := protocol.ParseTurnID(text)
	return &id, err
}
func conversationBusy(ctx context.Context, db *sql.Conn, session protocol.SessionID) (bool, error) {
	var busy bool
	err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM conversation_turns h JOIN agent_runs a ON a.turn=h.turn WHERE h.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))", session.String()).Scan(&busy)
	return busy, err
}
func rewindConversation(ctx context.Context, db *sql.Conn, input protocol.CommandRewindConversationPayload, backup bool) (protocol.ConversationRewind, []protocol.Event, error) {
	value := protocol.DefaultConversationRewind()
	value.Session = input.Session
	value.Through = input.Through
	current, err := ReadSession(ctx, db, input.Session)
	if err != nil {
		return value, nil, err
	}
	if current.Delegation != nil {
		return value, nil, fault(protocol.ErrorCodePermissionDenied, "child conversation is read-only")
	}
	revision, err := historyRevision(ctx, db, input.Session)
	if err != nil {
		return value, nil, err
	}
	if err = checkRevision(revision, input.ExpectedRevision); err != nil {
		return value, nil, err
	}
	busy, err := conversationBusy(ctx, db, input.Session)
	if err != nil {
		return value, nil, err
	}
	if busy {
		return value, nil, fault(protocol.ErrorCodeBusy, "conversation has active or queued turns")
	}
	head, err := conversationHead(ctx, db, input.Session)
	if err != nil {
		return value, nil, err
	}
	if head == nil || *head != input.ExpectedHead {
		return value, nil, fault(protocol.ErrorCodeRevisionConflict, "conversation head changed")
	}
	if input.Through != nil && *input.Through == *head {
		return value, nil, fault(protocol.ErrorCodeConflict, "conversation is already at this turn")
	}
	boundary := uint64(0)
	if input.Through != nil {
		run, e := visibleAgentRun(ctx, db, input.Session, *input.Through)
		if e != nil {
			return value, nil, e
		}
		boundary = run.Sequence
	}
	if backup {
		saved, _, e := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: input.Session, Through: *head, ExpectedRevision: current.Revision})
		if e != nil {
			return value, nil, e
		}
		value.Backup = &saved
	}
	if _, err = db.ExecContext(ctx, "DELETE FROM conversation_turns WHERE session=?1 AND turn IN (SELECT id FROM turns WHERE rowid>?2)", input.Session.String(), boundary); err != nil {
		return value, nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE sessions SET history_revision=history_revision+1 WHERE id=?1", input.Session.String()); err != nil {
		return value, nil, err
	}
	if err = rebuildConversationMembership(ctx, db, input.Session); err != nil {
		return value, nil, err
	}
	if err = rebuildCanonicalState(ctx, db, input.Session); err != nil {
		return value, nil, err
	}
	value.Revision = revision + 1
	current, err = ReadSession(ctx, db, input.Session)
	if err != nil {
		return value, nil, err
	}
	return value, []protocol.Event{{Kind: "session_rewound", Data: protocol.EventSessionRewoundPayload{Session: &current, Backup: sessionPointer(value.Backup)}}}, nil
}
func replaceConversationTurn(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	input := request.Command.Data.(*protocol.CommandReplaceTurnPayload)
	current, err := checkedSession(ctx, db, input.Session, input.ExpectedRevision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err = validateAgentInput(input.Message); err != nil {
		return protocol.Output{}, nil, err
	}
	target, err := visibleAgentRun(ctx, db, input.Session, input.Turn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if target.Kind != protocol.ConversationRunKindTask {
		return protocol.Output{}, nil, invalid("only task turns can be edited")
	}
	var previous sql.NullString
	if err = db.QueryRowContext(ctx, "SELECT (SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn WHERE h.session=?1 AND t.rowid<?2 ORDER BY t.rowid DESC LIMIT 1)", input.Session.String(), target.Sequence).Scan(&previous); err != nil {
		return protocol.Output{}, nil, err
	}
	var through *protocol.TurnID
	if previous.Valid {
		id, e := protocol.ParseTurnID(previous.String)
		if e != nil {
			return protocol.Output{}, nil, e
		}
		through = &id
	}
	history, _, err := rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{Session: input.Session, Through: through, ExpectedHead: input.ExpectedHead, ExpectedRevision: input.ExpectedHistoryRevision}, true)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	turn, err := admitAgentTurn(ctx, db, node, caller, request, current, protocol.ConversationRunKindTask, true)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	current, err = ReadSession(ctx, db, input.Session)
	return protocol.Output{Kind: "turn_replaced", Data: protocol.OutputTurnReplacedPayload{Turn: turn, History: &history}}, []protocol.Event{{Kind: "session_rewound", Data: protocol.EventSessionRewoundPayload{Session: &current, Backup: sessionPointer(history.Backup)}}}, err
}
func forkConversationAt(ctx context.Context, db *sql.Conn, input protocol.CommandForkConversationAtPayload) (protocol.Output, []protocol.Event, error) {
	current, err := checkedSession(ctx, db, input.Session, input.ExpectedRevision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if current.Project == nil {
		return protocol.Output{}, nil, invalid("connection conversations cannot move to a project")
	}
	var target bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM worktrees WHERE id=?1 AND project=?2 AND id NOT IN(SELECT worktree FROM removed_worktrees))", input.Worktree.String(), current.Project.String()).Scan(&target); err != nil {
		return protocol.Output{}, nil, err
	}
	if !target {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "worktree does not belong to the selected project")
	}
	busy, err := conversationBusy(ctx, db, input.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if busy {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "conversation has active or queued turns")
	}
	head, err := conversationHead(ctx, db, input.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if head == nil {
		value, e := insertBranchSession(ctx, db, current, input.Worktree)
		return protocol.Output{Kind: "session", Data: value}, []protocol.Event{{Kind: "session_changed", Data: &value}}, e
	}
	value, _, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: input.Session, Through: *head, ExpectedRevision: input.ExpectedRevision})
	if err != nil {
		return protocol.Output{}, nil, err
	}
	value.Worktree = input.Worktree
	return reviseSession(ctx, db, value)
}

func sessionPointer(value *protocol.Session) **protocol.Session {
	if value == nil {
		return nil
	}
	return &value
}
