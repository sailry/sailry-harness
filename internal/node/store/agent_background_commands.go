package store

import (
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type continuationFence struct {
	Package  protocol.PluginReference    `json:"package"`
	Key      string                      `json:"key"`
	Revision uint64                      `json:"revision"`
	Scope    protocol.PluginStorageScope `json:"scope"`
}

func ExecuteBackgroundCommand(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request, now time.Time) (protocol.Output, []protocol.Event, error) {
	if request.Command.Kind == "deliver_background" {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "background delivery is internal Node input")
	}
	if request.Command.Kind != "continue_turn" {
		return protocol.Output{}, nil, invalid("invalid background command")
	}
	if request.Plugin == nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "continuation requires plugin provenance")
	}
	info, err := ReadPlugin(ctx, db, request.Plugin.Package.Name)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if info == nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "continuation package is unavailable")
	}
	if err := CheckPluginRequest(ctx, db, request, *info); err != nil {
		return protocol.Output{}, nil, err
	}
	return admitContinuation(ctx, db, node, caller, request)
}

func validContinuation(ctx context.Context, db *sql.Conn, fence continuationFence, session protocol.SessionID) (bool, error) {
	info, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE name=?1 AND installed=1", fence.Package.Name)
	if err != nil {
		return false, err
	}
	if info == nil || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" || info.Summary.Name != fence.Package.Name || info.Summary.Digest != fence.Package.Digest || info.Summary.SettingsRevision != fence.Package.SettingsRevision {
		return false, nil
	}
	var current bool
	switch fence.Scope {
	case protocol.PluginStorageScopeNode:
		err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_values WHERE name=?1 AND key=?2 AND revision=?3 AND value IS NOT NULL)", fence.Package.Name, fence.Key, fence.Revision).Scan(&current)
	case protocol.PluginStorageScopeConversation:
		err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_conversation_values v JOIN plugin_conversation_records r ON r.sequence=v.record WHERE v.name=?1 AND v.key=?2 AND v.session=?3 AND v.revision=?4 AND v.restored=0 AND r.value IS NOT NULL)", fence.Package.Name, fence.Key, session.String(), fence.Revision).Scan(&current)
	default:
		return false, invalid("invalid continuation scope")
	}
	return current, err
}

func admitContinuation(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	input, ok := request.Command.Data.(*protocol.CommandContinueTurnPayload)
	if !ok || request.Plugin == nil {
		return protocol.Output{}, nil, invalid("invalid continuation request")
	}
	fence := continuationFence{request.Plugin.Package, input.Key, input.ExpectedRevision, input.Scope}
	valid, err := validContinuation(ctx, db, fence, input.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if !valid {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeRevisionConflict, "continuation data changed")
	}
	session, err := ReadSession(ctx, db, input.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if session.Config.Mode == protocol.WorkModePlan || session.Delegation != nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "session does not accept automatic input")
	}
	blocked, err := backgroundBlocked(ctx, db, session.ID)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if blocked {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "session has pending input")
	}
	if input.After != nil {
		run, e := visibleAgentRun(ctx, db, session.ID, *input.After)
		if e != nil {
			return protocol.Output{}, nil, e
		}
		var latest string
		if e = db.QueryRowContext(ctx, "SELECT t.id FROM turns t JOIN agent_runs a ON a.turn=t.id WHERE t.session=?1 ORDER BY t.rowid DESC LIMIT 1", session.ID.String()).Scan(&latest); e != nil && !errors.Is(e, sql.ErrNoRows) {
			return protocol.Output{}, nil, e
		}
		if run.Origin != nil || run.Status != protocol.ConversationStatusCompleted || latest != input.After.String() {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "continuation source is no longer current")
		}
	}
	if err = validateAgentInput(input.Message); err != nil {
		return protocol.Output{}, nil, err
	}
	if err = validateAgentReferences(ctx, db, session, input.Message, nil); err != nil {
		return protocol.Output{}, nil, err
	}
	turn, err := admitAgentTurn(ctx, db, node, caller, request, session, protocol.ConversationRunKindTask, true)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err = bindAgentAttachments(ctx, db, turn.ID, session.Worktree, input.Message.Attachments); err != nil {
		return protocol.Output{}, nil, err
	}
	// The input is captured now: a continuation admitted inside a plugin
	// transaction shares that request, whose stored body is only a digest.
	message, err := encode(input.Message)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET input=?2 WHERE turn=?1", turn.ID.String(), message); err != nil {
		return protocol.Output{}, nil, err
	}
	body, err := encode(fence)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_continuations(turn,body) VALUES(?1,?2)", turn.ID.String(), body); err != nil {
		return protocol.Output{}, nil, err
	}
	run, err := ReadAgentRun(ctx, db, turn.ID)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	info, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE name=?1", fence.Package.Name)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	run.Continuation = &protocol.ConversationContinuation{Package: fence.Package}
	if info != nil && info.Extension != nil {
		run.Continuation.Marker = info.Extension.ContinuationMarker
	}
	if err = putAgentRun(ctx, db, run); err != nil {
		return protocol.Output{}, nil, err
	}
	events, err := activityEvent(ctx, db, session.ID)
	return protocol.Output{Kind: "queued_turn", Data: turn}, events, err
}

func checkAutomaticInput(ctx context.Context, db *sql.Conn, run protocol.ConversationRun) (*protocol.Fault, error) {
	fence, err := readBody[continuationFence](ctx, db, "SELECT body FROM agent_continuations WHERE turn=?1", run.Turn.String())
	if err != nil {
		return nil, err
	}
	if fence != nil {
		var pending bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1 AND p.turn!=?2 AND NOT EXISTS(SELECT 1 FROM agent_continuations c WHERE c.turn=p.turn))", run.Session.String(), run.Turn.String()).Scan(&pending); err != nil {
			return nil, err
		}
		if pending {
			return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "automatic input was superseded by user input"}, nil
		}
		session, err := ReadSession(ctx, db, run.Session)
		if err != nil {
			return nil, err
		}
		valid, err := validContinuation(ctx, db, *fence, run.Session)
		if err != nil {
			return nil, err
		}
		if session.Config.Mode == protocol.WorkModePlan || !valid {
			return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "automatic input is no longer available"}, nil
		}
	}
	if run.Notice {
		var pending, user bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_background WHERE session=?1 AND state='pending' AND delivery=?2),EXISTS(SELECT 1 FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN agent_runs a ON a.turn=p.turn WHERE t.session=?1 AND p.turn!=?2 AND coalesce(json_extract(a.body,'$.notice'),0)=0)", run.Session.String(), run.Turn.String()).Scan(&pending, &user); err != nil {
			return nil, err
		}
		if !pending || user {
			return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "background delivery is no longer available"}, nil
		}
	}
	return nil, nil
}

func cancelAutomaticInput(ctx context.Context, db *sql.Conn, run protocol.ConversationRun, failure *protocol.Fault, now time.Time) ([]protocol.Event, error) {
	run.Status, run.Error = protocol.ConversationStatusCancelled, failure
	finished := now.UnixMilli()
	run.FinishedMs = &finished
	if err := putAgentRun(ctx, db, run); err != nil {
		return nil, err
	}
	if _, err := db.ExecContext(ctx, "UPDATE agent_runs SET ready=0 WHERE turn=?1", run.Turn.String()); err != nil {
		return nil, err
	}
	if _, err := db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", run.Turn.String()); err != nil {
		return nil, err
	}
	var hold *protocol.ConversationBackgroundHold
	if run.Notice {
		hold = new(protocol.ConversationBackgroundHoldOther)
		if failure != nil && failure.Code == protocol.ErrorCodeOutcomeUnknown {
			hold = new(protocol.ConversationBackgroundHoldInterrupted)
		}
	}
	released, err := ReleaseBackground(ctx, db, run.Turn, hold)
	if err != nil {
		return nil, err
	}
	if err = bumpQueue(ctx, db, run.Session); err != nil {
		return nil, err
	}
	events, err := activityEvent(ctx, db, run.Session)
	return append(events, released...), err
}
