package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	delegatedToolsKey  = "delegated_tools"
	retriedChildPrefix = "retry_child:"
)

// SaveDelegatedTools persists a child session's inherited parent tool boundary
// so a failed child turn can be retried safely without enlarging its permissions.
func SaveDelegatedTools(ctx context.Context, db *sql.Conn, session protocol.SessionID, body []byte) error {
	if len(body) == 0 {
		return nil
	}
	_, err := db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", session.String(), delegatedToolsKey, body)
	return err
}

// ReadDelegatedTools loads the persisted inherited tool boundary for session.
func ReadDelegatedTools(ctx context.Context, db *sql.Conn, session protocol.SessionID) ([]byte, error) {
	var body []byte
	err := db.QueryRowContext(ctx, "SELECT body FROM agent_state WHERE owner=?1 AND key=?2", session.String(), delegatedToolsKey).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	return body, err
}

// RetryChildTurn admits a recovery turn on a delegated child session whose
// latest turn ended failed or interrupted. It preserves the child's frozen
// permissions, role, worktree, and inherited tool boundary, unpauses the child
// queue, and schedules background delivery of the completed result to the parent.
func RetryChildTurn(ctx context.Context, db *sql.Conn, node protocol.NodeID, caller protocol.NodeID, request protocol.Request, targetTurn protocol.TurnID, now time.Time) (protocol.Output, []protocol.Event, error) {
	origin, err := readDelegation(ctx, db, targetTurn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if origin == nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeWrongTarget, "turn is not a delegated child")
	}
	targetRun, err := ReadAgentRun(ctx, db, targetTurn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if targetRun.Status != protocol.ConversationStatusFailed && targetRun.Status != protocol.ConversationStatusInterrupted {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "only failed or interrupted child turns can be retried")
	}
	var latest string
	if err = db.QueryRowContext(ctx, "SELECT id FROM turns WHERE session=?1 ORDER BY rowid DESC LIMIT 1", targetRun.Session.String()).Scan(&latest); err != nil {
		return protocol.Output{}, nil, err
	}
	if latest != targetTurn.String() {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "only the latest child turn can be retried")
	}
	var busy bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND (json_extract(a.body,'$.status') IN ('queued','running','stopping') OR json_extract(a.body,'$.status')='cancelled' AND json_extract(a.body,'$.finished_ms') IS NULL))", targetRun.Session.String()).Scan(&busy); err != nil {
		return protocol.Output{}, nil, err
	}
	if busy {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "child has active or pending work")
	}
	var ownsWorktree bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM worktrees w WHERE w.id=?1 AND NOT EXISTS(SELECT 1 FROM removed_worktrees r WHERE r.worktree=w.id))", targetRun.Worktree.String()).Scan(&ownsWorktree); err != nil {
		return protocol.Output{}, nil, err
	}
	if !ownsWorktree {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "child worktree is unavailable")
	}
	toolsBody, err := ReadDelegatedTools(ctx, db, targetRun.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if len(toolsBody) == 0 {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "parent tool capture is unavailable")
	}
	session, err := ReadSession(ctx, db, targetRun.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	capture, err := readTurnCapture(ctx, db, targetTurn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	parentRun, err := ReadAgentRun(ctx, db, origin.Turn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	var providerBody, hooks, inputBody, media, instructions []byte
	var compaction CompactionPolicy
	if err = db.QueryRowContext(ctx, "SELECT a.provider,a.hooks,a.compaction_percent,a.compaction_default,a.input,a.media,t.instructions FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE a.turn=?1", targetTurn.String()).Scan(&providerBody, &hooks, &compaction.Percent, &compaction.UseDefault, &inputBody, &media, &instructions); err != nil {
		return protocol.Output{}, nil, err
	}
	var provider protocol.ConversationProvider
	if err = json.Unmarshal(providerBody, &provider); err != nil {
		return protocol.Output{}, nil, err
	}
	if refreshed, refreshErr := captureTurnProvider(ctx, db, provider, capture.Config.Model); refreshErr == nil {
		provider = refreshed
		if encoded, encErr := encode(provider); encErr == nil {
			providerBody = encoded
		}
	}
	var hasUserInput bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_events WHERE session=?1 AND turn=?2 AND json_extract(body,'$.kind')='message' AND json_extract(body,'$.role')='user')", targetRun.Session.String(), targetTurn.String()).Scan(&hasUserInput); err != nil {
		return protocol.Output{}, nil, err
	}
	message := protocol.DefaultConversationInput()
	if hasUserInput {
		message.Text = "Continue the delegated task from where the previous attempt failed."
	} else if len(inputBody) > 0 {
		if err = json.Unmarshal(inputBody, &message); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	if strings.TrimSpace(message.Text) == "" && len(message.References) == 0 && len(message.Attachments) == 0 {
		message.Text = "Continue the delegated task from where the previous attempt failed."
	}

	turn := capture
	turn.ID, turn.Request = protocol.NewTurnID(), request.ID
	seq, err := NextCounter(ctx, db, CounterTurns)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	pluginsBody, err := encode(turn.Plugins)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	rolesBody, err := encode(turn.Roles)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO turns(id,session,revision,request,caller,kind,plugins,roles,instructions,rowid,user) SELECT ?1,?2,?3,?4,?5,'task',?6,?7,?8,?9,user FROM turns WHERE id=?10", turn.ID.String(), session.ID.String(), turn.Revision, request.ID.String(), caller[:], pluginsBody, rolesBody, instructions, seq, targetTurn.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	if err = inheritTurnOrigin(ctx, db, turn.ID, targetTurn); err != nil {
		return protocol.Output{}, nil, err
	}
	retriedOrigin := *origin
	retriedOrigin.Overlap = false
	originBody, err := encode(retriedOrigin)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_delegations(turn,parent_turn,entry,part,body) VALUES(?1,?2,?3,?4,?5)", turn.ID.String(), origin.Turn.String(), origin.Entry, origin.Index, originBody); err != nil {
		return protocol.Output{}, nil, err
	}
	if err = BindTurnAttachments(ctx, db, turn.ID, message.Attachments); err != nil {
		return protocol.Output{}, nil, err
	}
	run := protocol.DefaultConversationRun()
	run.Turn, run.Session, run.Worktree, run.Kind, run.Sequence, run.Revision, run.Status = turn.ID, session.ID, session.Worktree, protocol.ConversationRunKindTask, seq, turn.Revision, protocol.ConversationStatusQueued
	runBody, err := encode(run)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	encodedInput, err := encode(message)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_runs(turn,ready,body,provider,hooks,compaction_percent,input,media,compaction_default) VALUES(?1,1,?2,?3,?4,?5,?6,?7,?8)", turn.ID.String(), runBody, providerBody, hooks, compaction.Percent, encodedInput, media, compaction.UseDefault); err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)", session.ID.String(), turn.ID.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	if err = bumpQueue(ctx, db, session.ID); err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_queues SET paused=0 WHERE session=?1", session.ID.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,'1') ON CONFLICT(owner,key) DO UPDATE SET body='1'", session.ID.String(), retriedChildPrefix+turn.ID.String()); err != nil {
		return protocol.Output{}, nil, err
	}

	title := "Retry subagent"
	if storedTitle, titleErr := readBody[string](ctx, db, "SELECT body FROM agent_state WHERE owner=?1 AND key=?2", session.ID.String(), sessionTitleKey); titleErr == nil && storedTitle != nil && strings.TrimSpace(*storedTitle) != "" {
		title = *storedTitle
	} else if priorBg, bgErr := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "agent:"+targetTurn.String()); bgErr == nil && priorBg != nil && strings.TrimSpace(priorBg.Task.Title) != "" {
		title = priorBg.Task.Title
	}
	events, err := RegisterChildBackground(ctx, db, parentRun, run, title, now)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	activity, err := readActivity(ctx, db, session.ID)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	events = append(events, protocol.Event{Kind: "turn_queued", Data: protocol.EventTurnQueuedPayload{Activity: &activity, Turn: turn}})
	return protocol.Output{Kind: "queued_turn", Data: turn}, events, nil
}

// ClaimRetriedChild claims the next queued controller-retried child turn while
// respecting the parent conversation's ChildConcurrency cap.
func ClaimRetriedChild(ctx context.Context, db *sql.Conn, now time.Time) (*AgentInvocation, []protocol.Event, error) {
	rows, err := db.QueryContext(ctx, "SELECT owner, key FROM agent_state WHERE key LIKE ?1 ORDER BY rowid", retriedChildPrefix+"%")
	if err != nil {
		return nil, nil, err
	}
	type candidate struct{ owner, key string }
	var pending []candidate
	for rows.Next() {
		var c candidate
		if err = rows.Scan(&c.owner, &c.key); err != nil {
			rows.Close()
			return nil, nil, err
		}
		pending = append(pending, c)
	}
	if err = rows.Err(); err != nil {
		rows.Close()
		return nil, nil, err
	}
	rows.Close()

	for _, c := range pending {
		rawTurn := strings.TrimPrefix(c.key, retriedChildPrefix)
		turnID, parseErr := protocol.ParseTurnID(rawTurn)
		if parseErr != nil {
			_, _ = db.ExecContext(ctx, "DELETE FROM agent_state WHERE owner=?1 AND key=?2", c.owner, c.key)
			continue
		}
		run, err := ReadAgentRun(ctx, db, turnID)
		if err != nil || run.Status != protocol.ConversationStatusQueued {
			_, _ = db.ExecContext(ctx, "DELETE FROM agent_state WHERE owner=?1 AND key=?2", c.owner, c.key)
			continue
		}
		inv, events, err := ClaimChild(ctx, db, turnID, now)
		if err != nil {
			return nil, nil, err
		}
		if inv != nil {
			if _, err = db.ExecContext(ctx, "DELETE FROM agent_state WHERE owner=?1 AND key=?2", c.owner, c.key); err != nil {
				return nil, nil, err
			}
			return inv, events, nil
		}
	}
	return nil, nil, nil
}
