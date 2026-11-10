package store

import (
	"context"
	"database/sql"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func SupportsAgentCommand(kind string) bool {
	switch kind {
	case "create_session_at", "set_session_config", "submit_turn", "queue_turn", "guide_turn", "stop_turn", "start_queued_turn", "compact_context", "read_conversation", "read_conversation_runs", "read_conversation_run", "read_turn", "read_turn_capture", "read_tool_detail", "read_tool_output", "list_conversation_assets", "read_usage", "search_conversation", "fork_conversation", "fork_conversation_at", "rewind_conversation", "replace_turn", "resolve_approval", "resolve_question", "read_queued_turn", "edit_queued_turn", "remove_queued_turn", "send_queued_turn", "guide_queued_turn", "withdraw_guidance", "move_queued_turn", "set_queue_paused", "read_compaction_settings", "save_compaction_settings":
		return true
	}
	return false
}
func ExecuteAgentCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, profile string, caller protocol.NodeID, request protocol.Request, now time.Time) (protocol.Output, []protocol.Event, error) {
	switch request.Command.Kind {
	case "create_session_at":
		v := request.Command.Data.(*protocol.CommandCreateSessionAtPayload)
		if v.Target != node {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeWrongTarget, "session target belongs to another Node")
		}
		if v.Config != nil && v.ProviderRevision > 0 {
			saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", v.Config.Provider.String())
			if err != nil {
				return protocol.Output{}, nil, err
			}
			if saved == nil || saved.Provider == nil {
				return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "provider is unavailable")
			}
			if err = checkRevision(saved.Provider.Revision, v.ProviderRevision); err != nil {
				return protocol.Output{}, nil, err
			}
		}
		return createSession(ctx, db, node, profile, protocol.CommandCreateSessionPayload{Project: v.Project, Worktree: v.Worktree, Config: v.Config})
	case "set_session_config":
		v := request.Command.Data.(*protocol.CommandSetSessionConfigPayload)
		session, err := checkedSession(ctx, db, v.Session, v.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = validateConfig(ctx, db, node, v.Config); err != nil {
			return protocol.Output{}, nil, err
		}
		if !sameAssistant(session.Config.Resource, v.Config.Resource) || !sameAssistant(session.Config.Assistant, v.Config.Assistant) {
			return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "conversation resource and assistant scope are immutable")
		}
		session.Config = v.Config
		return reviseSession(ctx, db, session)
	case "submit_turn", "queue_turn":
		var id protocol.SessionID
		var revision uint64
		if request.Command.Kind == "submit_turn" {
			v := request.Command.Data.(*protocol.CommandSubmitTurnPayload)
			id, revision = v.Session, v.ExpectedRevision
		} else {
			v := request.Command.Data.(*protocol.CommandQueueTurnPayload)
			id, revision = v.Session, v.ExpectedRevision
		}
		session, err := checkedSession(ctx, db, id, revision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		message, err := agentInput(request.Command)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = validateAgentInput(message); err != nil {
			return protocol.Output{}, nil, err
		}
		turn, err := admitAgentTurn(ctx, db, node, caller, request, session, protocol.ConversationRunKindTask, request.Command.Kind == "submit_turn")
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = bindAgentAttachments(ctx, db, turn.ID, session.Worktree, message.Attachments); err != nil {
			return protocol.Output{}, nil, err
		}
		activity, err := readActivity(ctx, db, id)
		return protocol.Output{Kind: "queued_turn", Data: turn}, []protocol.Event{{Kind: "turn_queued", Data: protocol.EventTurnQueuedPayload{Activity: &activity, Turn: turn}}}, err
	case "compact_context":
		v := request.Command.Data.(*protocol.CommandCompactContextPayload)
		session, err := checkedSession(ctx, db, v.Session, v.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err = agentInput(request.Command); err != nil {
			return protocol.Output{}, nil, err
		}
		turn, err := admitAgentTurn(ctx, db, node, caller, request, session, protocol.ConversationRunKindCompaction, true)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		activity, err := readActivity(ctx, db, v.Session)
		return protocol.Output{Kind: "queued_turn", Data: turn}, []protocol.Event{{Kind: "turn_queued", Data: protocol.EventTurnQueuedPayload{Activity: &activity, Turn: turn}}}, err
	case "guide_turn":
		v := request.Command.Data.(*protocol.CommandGuideTurnPayload)
		target, err := activeAgentRun(ctx, db, v.Recipient)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		session, err := checkedSession(ctx, db, target.Session, v.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = validateAgentInput(v.Message); err != nil {
			return protocol.Output{}, nil, err
		}
		turn, err := admitAgentTurn(ctx, db, node, caller, request, session, protocol.ConversationRunKindTask, true)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = tagGuidance(ctx, db, turn.ID, target.Turn); err != nil {
			return protocol.Output{}, nil, err
		}
		activity, err := readActivity(ctx, db, target.Session)
		return protocol.Output{Kind: "queued_turn", Data: turn}, []protocol.Event{{Kind: "turn_queued", Data: protocol.EventTurnQueuedPayload{Activity: &activity, Turn: turn}}}, err
	case "stop_turn":
		run, err := stopAgentTurn(ctx, db, request.Command.Data.(*protocol.CommandStopTurnPayload).Turn, now)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		events, err := activityEvent(ctx, db, run.Session)
		return protocol.Output{Kind: "run", Data: run}, events, err
	case "start_queued_turn":
		run, err := startQueued(ctx, db, request.Command.Data.(*protocol.CommandStartQueuedTurnPayload).Turn)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		events, err := activityEvent(ctx, db, run.Session)
		return protocol.Output{Kind: "run", Data: run}, events, err
	case "fork_conversation":
		value, events, err := forkConversation(ctx, db, *request.Command.Data.(*protocol.CommandForkConversationPayload))
		return protocol.Output{Kind: "session", Data: value}, events, err
	case "fork_conversation_at":
		return forkConversationAt(ctx, db, *request.Command.Data.(*protocol.CommandForkConversationAtPayload))
	case "rewind_conversation":
		value, events, err := rewindConversation(ctx, db, *request.Command.Data.(*protocol.CommandRewindConversationPayload), true)
		return protocol.Output{Kind: "rewound", Data: value}, events, err
	case "replace_turn":
		return replaceConversationTurn(ctx, db, node, caller, request)
	case "resolve_approval":
		return resolveApproval(ctx, db, *request.Command.Data.(*protocol.CommandResolveApprovalPayload))
	case "resolve_question":
		return resolveQuestion(ctx, db, node, caller, request)
	case "save_compaction_settings":
		return saveCompaction(ctx, db, *request.Command.Data.(*protocol.ConversationCompactionSettings))
	case "edit_queued_turn", "remove_queued_turn", "send_queued_turn", "guide_queued_turn", "withdraw_guidance", "move_queued_turn", "set_queue_paused":
		return executeQueueCommand(ctx, db, caller, request, now)
	default:
		output, err := ReadAgentCommand(ctx, db, node, request.Command)
		return output, nil, err
	}
}
func sameAssistant(a, b any) bool {
	x, _ := encode(a)
	y, _ := encode(b)
	return string(x) == string(y)
}
func bindAgentAttachments(ctx context.Context, db *sql.Conn, turn protocol.TurnID, worktree protocol.WorktreeID, ids []protocol.AttachmentID) error {
	if _, err := db.ExecContext(ctx, "DELETE FROM turn_attachments WHERE turn=?1", turn.String()); err != nil {
		return err
	}
	for i, id := range ids {
		var bound bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM attachments WHERE id=?1 AND worktree=?2)", id.String(), worktree.String()).Scan(&bound); err != nil {
			return err
		}
		if !bound {
			return fault(protocol.ErrorCodeNotFound, "attachment is unavailable on this worktree")
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO turn_attachments(turn,attachment,position) VALUES(?1,?2,?3)", turn.String(), id.String(), i); err != nil {
			return err
		}
	}
	return nil
}
func executeQueueCommand(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, now time.Time) (protocol.Output, []protocol.Event, error) {
	var session protocol.SessionID
	changed := []protocol.ConversationRun{}
	switch v := request.Command.Data.(type) {
	case *protocol.CommandEditQueuedTurnPayload:
		run, err := checkQueueItem(ctx, db, v.Turn, v.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = validateAgentInput(v.Message); err != nil {
			return protocol.Output{}, nil, err
		}
		var captured bool
		if err = db.QueryRowContext(ctx, "SELECT input IS NOT NULL FROM agent_runs WHERE turn=?1", v.Turn.String()).Scan(&captured); err != nil {
			return protocol.Output{}, nil, err
		}
		if captured {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "admitted input is immutable; submit an edited message")
		}
		if run.Notice || run.Kind != protocol.ConversationRunKindTask {
			return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "queued maintenance input is read-only")
		}
		if err = bindAgentAttachments(ctx, db, v.Turn, run.Worktree, v.Message.Attachments); err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET revision=revision+1,request=?2,caller=?3,recipient=NULL WHERE turn=?1", v.Turn.String(), request.ID.String(), caller[:]); err != nil {
			return protocol.Output{}, nil, err
		}
		session = run.Session
		changed = append(changed, run)
		if err = bumpQueue(ctx, db, session); err != nil {
			return protocol.Output{}, nil, err
		}
	case *protocol.CommandRemoveQueuedTurnPayload:
		run, err := checkQueueItem(ctx, db, v.Turn, v.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		run, err = stopAgentTurn(ctx, db, v.Turn, now)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		session = run.Session
		changed = append(changed, run)
	case *protocol.CommandSendQueuedTurnPayload:
		run, err := checkQueueItem(ctx, db, v.Turn, v.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		session = run.Session
		queue, err := ReadQueue(ctx, db, session)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		var before *protocol.TurnID
		if len(queue.Items) > 0 {
			id := queue.Items[0].Turn
			before = &id
		}
		if err = reorderQueue(ctx, db, session, v.Turn, before); err != nil {
			return protocol.Output{}, nil, err
		}
		active, err := readBodies[protocol.ConversationRun](ctx, db, "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('running','stopping')", session.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		for _, r := range active {
			stopped, e := stopAgentTurn(ctx, db, r.Turn, now)
			if e != nil {
				return protocol.Output{}, nil, e
			}
			changed = append(changed, stopped)
		}
		run, err = startQueued(ctx, db, v.Turn)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		changed = append(changed, run)
	case *protocol.CommandGuideQueuedTurnPayload:
		run, err := checkQueueItem(ctx, db, v.Turn, v.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = tagGuidance(ctx, db, v.Turn, v.Recipient); err != nil {
			return protocol.Output{}, nil, err
		}
		session = run.Session
		run, err = ReadAgentRun(ctx, db, v.Turn)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		changed = append(changed, run)
	case *protocol.CommandWithdrawGuidancePayload:
		run, err := checkQueueItem(ctx, db, v.Turn, v.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		result, err := db.ExecContext(ctx, "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE turn=?1 AND recipient IS NOT NULL", v.Turn.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		n, err := result.RowsAffected()
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if n == 0 {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "queued input has no pending guidance")
		}
		if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET ready=0 WHERE turn=?1", v.Turn.String()); err != nil {
			return protocol.Output{}, nil, err
		}
		session = run.Session
		if err = bumpQueue(ctx, db, session); err != nil {
			return protocol.Output{}, nil, err
		}
		changed = append(changed, run)
	case *protocol.CommandMoveQueuedTurnPayload:
		session = v.Session
		queue, err := ReadQueue(ctx, db, session)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = checkRevision(queue.Revision, v.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		if err = reorderQueue(ctx, db, session, v.Turn, v.Before); err != nil {
			return protocol.Output{}, nil, err
		}
	case *protocol.CommandSetQueuePausedPayload:
		session = v.Session
		queue, err := ReadQueue(ctx, db, session)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = checkRevision(queue.Revision, v.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		if err = setQueuePaused(ctx, db, session, v.Paused); err != nil {
			return protocol.Output{}, nil, err
		}
		if v.Paused {
			if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE turn IN (SELECT id FROM turns WHERE session=?1) AND recipient IS NOT NULL", session.String()); err != nil {
				return protocol.Output{}, nil, err
			}
		}
	default:
		return protocol.Output{}, nil, invalid("command does not operate on a queue")
	}
	current, err := ReadSession(ctx, db, session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if current.Delegation != nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "child queued input is read-only")
	}
	queue, err := ReadQueue(ctx, db, session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	events, err := activityEvent(ctx, db, session)
	return protocol.Output{Kind: "queue", Data: protocol.ConversationQueueUpdate{Session: session, Queue: queue, Runs: changed}}, events, err
}
func ReadAgentCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, command protocol.Command) (protocol.Output, error) {
	return ReadAgentCommandForCaller(ctx, db, node, node, command)
}
func ReadAgentCommandForCaller(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, command protocol.Command) (protocol.Output, error) {
	switch v := command.Data.(type) {
	case *protocol.CommandListConversationAssetsPayload:
		value, err := ListConversationAssets(ctx, db, v.Session, v.Query)
		return protocol.Output{Kind: "conversation_assets", Data: value}, err
	case *protocol.CommandReadConversationPayload:
		value, err := ReadConversation(ctx, db, v.Session, v.Before, v.Limit)
		return protocol.Output{Kind: "conversation", Data: value}, err
	case *protocol.CommandReadTurnPayload:
		value, err := ReadConversationTurn(ctx, db, v.Session, v.Turn, v.ExpectedRevision, v.Before, v.Limit)
		return protocol.Output{Kind: "turn_history", Data: value}, err
	case *protocol.CommandReadTurnCapturePayload:
		if _, err := visibleAgentRun(ctx, db, v.Session, v.Turn); err != nil {
			return protocol.Output{}, err
		}
		value, err := readTurnCapture(ctx, db, v.Turn)
		return protocol.Output{Kind: "queued_turn", Data: value}, err
	case *protocol.CommandReadQueuedTurnPayload:
		value, err := readQueuedMessage(ctx, db, v.Turn)
		return protocol.Output{Kind: "queued_message", Data: value}, err
	case *protocol.CommandReadConversationRunsPayload:
		value, err := readConversationRuns(ctx, db, *v)
		return protocol.Output{Kind: "conversation_runs", Data: value}, err
	case *protocol.CommandReadConversationRunPayload:
		revision, err := historyRevision(ctx, db, v.Session)
		if err != nil {
			return protocol.Output{}, err
		}
		if v.ExpectedRevision != nil {
			if err = checkRevision(revision, *v.ExpectedRevision); err != nil {
				return protocol.Output{}, err
			}
		}
		value, err := visibleAgentRun(ctx, db, v.Session, v.Turn)
		return protocol.Output{Kind: "conversation_run", Data: summarizeRun(value)}, err
	case *protocol.CommandReadToolDetailPayload:
		value, err := readAgentToolDetail(ctx, db, *v)
		return protocol.Output{Kind: "tool_detail", Data: value}, err
	case *protocol.CommandReadToolOutputPayload:
		var owner []byte
		if err := db.QueryRowContext(ctx, "SELECT caller FROM turns WHERE id=?1", v.Turn.String()).Scan(&owner); err != nil {
			return protocol.Output{}, err
		}
		if string(owner) != string(caller[:]) {
			return protocol.Output{}, fault(protocol.ErrorCodePermissionDenied, "tool output read belongs to another invocation")
		}
		value, err := readAgentToolOutput(ctx, db, *v)
		return protocol.Output{Kind: "tool_output", Data: value}, err
	case *protocol.CommandSearchConversationPayload:
		value, err := searchAgentConversation(ctx, db, v.Session, v.Query)
		return protocol.Output{Kind: "conversation_matches", Data: value}, err
	case *protocol.UsageQuery:
		value, err := readAgentUsage(ctx, db, node, *v)
		return protocol.Output{Kind: "usage", Data: value}, err
	}
	if command.Kind == "read_compaction_settings" {
		value, err := ReadCompaction(ctx, db)
		return protocol.Output{Kind: "compaction_settings", Data: value}, err
	}
	return protocol.Output{}, fault(protocol.ErrorCodeNotConfigured, "agent command is unavailable")
}
func summarizeRun(run protocol.ConversationRun) protocol.ConversationRunsSummary {
	value := protocol.DefaultConversationRunsSummary()
	value.Turn = run.Turn
	value.Sequence = run.Sequence
	value.Status = run.Status
	value.Origin = run.Origin
	if run.Error != nil {
		value.ErrorCode = &run.Error.Code
	}
	return value
}
func readConversationRuns(ctx context.Context, db *sql.Conn, input protocol.CommandReadConversationRunsPayload) (protocol.ConversationRunsPage, error) {
	value := protocol.DefaultConversationRunsPage()
	value.Session = input.Session
	if input.Limit == 0 {
		return value, invalid("run limit must be positive")
	}
	var err error
	if value.Revision, err = historyRevision(ctx, db, input.Session); err != nil {
		return value, err
	}
	boundary := uint64(1<<63 - 1)
	if input.Before != nil {
		run, e := visibleAgentRun(ctx, db, input.Session, *input.Before)
		if e != nil {
			return value, e
		}
		boundary = run.Sequence
	}
	runs, err := readBodies[protocol.ConversationRun](ctx, db, "SELECT a.body FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=h.turn WHERE h.session=?1 AND t.rowid<?2 ORDER BY t.rowid DESC LIMIT ?3", input.Session.String(), boundary, min(input.Limit, 100)+1)
	if err != nil {
		return value, err
	}
	if len(runs) > int(min(input.Limit, 100)) {
		runs = runs[:min(input.Limit, 100)]
		id := runs[len(runs)-1].Turn
		value.NextBefore = &id
	}
	for _, run := range runs {
		if run.Session != input.Session {
			origin := run.Session
			run.Origin = &origin
		}
		value.Runs = append(value.Runs, summarizeRun(run))
	}
	queue, err := ReadQueue(ctx, db, input.Session)
	value.QueueRevision = queue.Revision
	return value, err
}
