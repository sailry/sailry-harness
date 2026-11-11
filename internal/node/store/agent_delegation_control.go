package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ChildState struct {
	Run       protocol.ConversationRun
	Response  *string
	Recipient *protocol.TurnID
	Paused    bool
}

func ReleaseChildren(ctx context.Context, db *sql.Conn, parent protocol.TurnID, children []protocol.TurnID) ([]protocol.Event, error) {
	for _, turn := range children {
		origin, err := readDelegation(ctx, db, turn)
		if err != nil {
			return nil, err
		}
		if origin == nil || origin.Turn != parent {
			return nil, fault(protocol.ErrorCodePermissionDenied, "child release belongs to another turn")
		}
		if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET ready=1 WHERE turn=?1 AND json_extract(body,'$.status')='queued'", turn.String()); err != nil {
			return nil, err
		}
	}
	return nil, nil
}
func ClaimChild(ctx context.Context, db *sql.Conn, turn protocol.TurnID, now time.Time) (*AgentInvocation, []protocol.Event, error) {
	origin, err := readDelegation(ctx, db, turn)
	if err != nil {
		return nil, nil, err
	}
	if origin == nil {
		return nil, nil, fault(protocol.ErrorCodeWrongTarget, "turn is not a delegated child")
	}
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, nil, err
	}
	if run.Status != protocol.ConversationStatusQueued {
		return nil, nil, nil
	}
	parent, err := ReadAgentRun(ctx, db, origin.Turn)
	if err != nil {
		return nil, nil, err
	}
	settings, err := ReadTeamSettings(ctx, db)
	if err != nil {
		return nil, nil, err
	}
	var active uint64
	if err = db.QueryRowContext(ctx, "SELECT count(*) FROM agent_runs a JOIN agent_delegations d ON d.turn=a.turn JOIN turns p ON p.id=d.parent_turn WHERE p.session=?1 AND json_extract(a.body,'$.status') IN ('running','stopping')", parent.Session.String()).Scan(&active); err != nil {
		return nil, nil, err
	}
	if active >= settings.ChildConcurrency {
		return nil, nil, nil
	}
	var next string
	err = db.QueryRowContext(ctx, "SELECT a.turn FROM agent_runs a JOIN agent_delegations d ON d.turn=a.turn JOIN turns t ON t.id=a.turn JOIN turns p ON p.id=d.parent_turn WHERE p.session=?1 AND a.ready=1 AND json_extract(a.body,'$.status')='queued' AND NOT EXISTS(SELECT 1 FROM agent_pending pending WHERE pending.turn=a.turn AND pending.recipient IS NULL) AND NOT EXISTS(SELECT 1 FROM agent_runs running JOIN turns current ON current.id=running.turn WHERE current.session=t.session AND json_extract(running.body,'$.status') IN ('running','stopping')) ORDER BY t.rowid LIMIT 1", parent.Session.String()).Scan(&next)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil, nil
	}
	if err != nil {
		return nil, nil, err
	}
	if next != turn.String() {
		return nil, nil, nil
	}
	inv, events, err := claimAgentInvocation(ctx, db, turn, now)
	if err != nil {
		return inv, events, err
	}
	changed, err := activityEvent(ctx, db, parent.Session)
	events = append(events, changed...)
	return inv, events, err
}
func FinishQueuedChild(ctx context.Context, db *sql.Conn, turn protocol.TurnID, status protocol.ConversationStatus, failure *protocol.Fault, now time.Time) ([]protocol.Event, error) {
	origin, err := readDelegation(ctx, db, turn)
	if err != nil {
		return nil, err
	}
	if origin == nil || status != protocol.ConversationStatusCancelled && status != protocol.ConversationStatusInterrupted && status != protocol.ConversationStatusFailed {
		return nil, invalid("invalid queued child completion")
	}
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, err
	}
	if run.FinishedMs != nil {
		return nil, nil
	}
	if run.StartedMs != nil || run.Status != protocol.ConversationStatusQueued && run.Status != protocol.ConversationStatusCancelled {
		return nil, fault(protocol.ErrorCodeConflict, "child execution is no longer queued")
	}
	_, events, err := FinishTurn(ctx, db, turn, status, failure, now)
	return events, err
}
func ownedChild(ctx context.Context, db *sql.Conn, parent protocol.TurnID, child protocol.SessionID, exact *protocol.TurnID) (protocol.ConversationRun, error) {
	parentRun, err := activeAgentRun(ctx, db, parent)
	if err != nil {
		return protocol.ConversationRun{}, err
	}
	parentOrigin, err := foundingDelegation(ctx, db, parentRun.Session)
	if err != nil {
		return protocol.ConversationRun{}, err
	}
	founding, err := foundingDelegation(ctx, db, child)
	if err != nil {
		return protocol.ConversationRun{}, err
	}
	if parentOrigin != nil || founding == nil || founding.Session != parentRun.Session {
		return protocol.ConversationRun{}, fault(protocol.ErrorCodePermissionDenied, "child belongs to another parent session")
	}
	var run protocol.ConversationRun
	if exact != nil {
		run, err = ReadAgentRun(ctx, db, *exact)
	} else {
		var body []byte
		err = db.QueryRowContext(ctx, "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.joined') IS NULL ORDER BY CASE json_extract(a.body,'$.status') WHEN 'running' THEN 0 WHEN 'stopping' THEN 0 WHEN 'queued' THEN 1 ELSE 2 END,t.rowid DESC LIMIT 1", child.String()).Scan(&body)
		if err == nil {
			err = json.Unmarshal(body, &run)
		}
	}
	if err != nil {
		return run, err
	}
	origin, err := readDelegation(ctx, db, run.Turn)
	if err != nil {
		return run, err
	}
	if run.Session != child || origin == nil || origin.Session != founding.Session || !sameAssistant(origin.Role, founding.Role) {
		return run, fault(protocol.ErrorCodePermissionDenied, "child execution belongs to another owner")
	}
	return run, nil
}
func ReadChildState(ctx context.Context, db *sql.Conn, parent protocol.TurnID, child protocol.SessionID, exact *protocol.TurnID) (ChildState, error) {
	state := ChildState{}
	var err error
	state.Run, err = ownedChild(ctx, db, parent, child, exact)
	if err != nil {
		return state, err
	}
	if state.Run.Status == protocol.ConversationStatusCompleted {
		state.Response, err = childResponse(ctx, db, state.Run.Turn)
		if err != nil {
			return state, err
		}
	}
	queue, err := ReadQueue(ctx, db, child)
	state.Paused = queue.Paused
	return state, err
}
func childResponse(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (*string, error) {
	records, err := readBodies[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND json_extract(body,'$.kind')='message' AND json_extract(body,'$.role')='assistant' ORDER BY sequence DESC", turn.String())
	if err != nil {
		return nil, err
	}
	for _, record := range records {
		message := record.Message()
		if message == nil || message.Role != canonical.Assistant {
			continue
		}
		var text strings.Builder
		valid := true
		for _, block := range message.Content {
			switch block.Kind {
			case "tool_request", "tool_response":
				valid = false
			case "text":
				if value, ok := block.Data.(*canonical.TextContent); ok {
					text.WriteString(value.Text)
				}
			}
		}
		if valid && text.Len() > 0 {
			value := text.String()
			return &value, nil
		}
	}
	return nil, nil
}
func PendingGuidance(ctx context.Context, db *sql.Conn, recipient protocol.TurnID) (bool, error) {
	var pending bool
	err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn WHERE p.recipient=?1 AND json_extract(a.body,'$.status')='queued')", recipient.String()).Scan(&pending)
	return pending, err
}

// GuideChild guides or resumes a child of the parent turn. identity is the
// recorded call, guide_agent or a package tool bound to agents.guide, and
// locates it in the conversation; arguments are the ones that execute, which
// such a package tool prepares.
func GuideChild(ctx context.Context, db *sql.Conn, node protocol.NodeID, parent protocol.TurnID, identity canonical.ToolIdentity, arguments json.RawMessage, now time.Time) (ChildState, *AgentInvocation, []protocol.Event, error) {
	var args struct {
		Session *protocol.SessionID `json:"session"`
		Message string              `json:"message"`
	}
	if operation, err := delegationOperation(ctx, db, parent, identity.Name); err != nil {
		return ChildState{}, nil, nil, err
	} else if operation != protocol.ToolOperationGuideAgent {
		return ChildState{}, nil, nil, invalid("invalid child guidance tool")
	}
	if err := protocol.DecodeObject(arguments, &args, []string{"session", "message"}, true); err != nil || args.Session == nil {
		return ChildState{}, nil, nil, invalid("invalid child guidance arguments")
	}
	state, err := ReadChildState(ctx, db, parent, *args.Session, nil)
	if err != nil {
		return state, nil, nil, err
	}
	if state.Run.Status == protocol.ConversationStatusStopping {
		return state, nil, nil, fault(protocol.ErrorCodeConflict, "child is stopping")
	}
	recipient := state.Run.Turn
	state.Recipient = &recipient
	if state.Run.Status == protocol.ConversationStatusRunning {
		inv, events, err := delegateAgent(ctx, db, node, parent, identity, arguments, nil, true, now, &state.Run)
		if err == nil {
			state.Run = inv.Run
		}
		return state, nil, events, err
	}
	inv, events, err := DelegatePrepared(ctx, db, node, parent, identity, arguments, nil, false, now)
	if err != nil {
		return state, nil, events, err
	}
	state.Run = inv.Run
	return state, &inv, events, nil
}
