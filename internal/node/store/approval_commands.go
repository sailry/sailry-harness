package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ApprovalBinding struct {
	Owner     protocol.PluginReference `json:"owner"`
	Tool      string                   `json:"tool"`
	Operation *protocol.ToolOperation  `json:"operation"`
}

func (v *ApprovalBinding) UnmarshalJSON(data []byte) error {
	type plain ApprovalBinding
	var x plain
	if err := protocol.DecodeObject(data, &x, []string{"owner", "tool"}, true); err != nil {
		return err
	}
	*v = ApprovalBinding(x)
	return nil
}
func locateAgentCall(ctx context.Context, db *sql.Conn, turn protocol.TurnID, identity canonical.ToolIdentity) (string, uint64, *canonical.ToolRequest, error) {
	if identity.ID == nil || *identity.ID == "" {
		return "", 0, nil, invalid("tool call identity is missing")
	}
	token, err := encode(*identity.ID)
	if err != nil {
		return "", 0, nil, err
	}
	// Only the records that request this call are decoded; a stored body that
	// lacks the encoded call identifier is skipped before it is parsed.
	records, err := readBodies[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND (typeof(body)!='blob' OR instr(body,?3)>0) AND json_extract(body,'$.kind')='message' AND EXISTS(SELECT 1 FROM json_each(body,'$.content') b WHERE json_extract(b.value,'$.type')='tool_request' AND json_extract(b.value,'$.id')=?2) ORDER BY sequence", turn.String(), *identity.ID, token)
	if err != nil {
		return "", 0, nil, err
	}
	var entry string
	var index uint64
	var found *canonical.ToolRequest
	for _, r := range records {
		if m := r.Message(); m != nil {
			for _, b := range m.Content {
				if call, ok := b.Data.(*canonical.ToolRequest); ok && call.ID == *identity.ID && call.Name == identity.Name {
					var x, y any
					if json.Unmarshal(call.Arguments, &x) != nil || json.Unmarshal(identity.Arguments, &y) != nil {
						return "", 0, nil, invalid("invalid tool arguments")
					}
					a, _ := json.Marshal(x)
					z, _ := json.Marshal(y)
					if !bytes.Equal(a, z) {
						continue
					}
					if found != nil {
						return "", 0, nil, invalid("tool call identity is ambiguous")
					}
					entry, index, found = r.ID, b.Index, call
				}
			}
		}
	}
	if found == nil {
		return "", 0, nil, fault(protocol.ErrorCodeNotFound, "canonical tool request is unavailable")
	}
	return entry, index, found, nil
}
func activeAgentRun(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationRun, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err == nil && run.Status != protocol.ConversationStatusRunning {
		return run, fault(protocol.ErrorCodeConflict, "turn is no longer running")
	}
	return run, err
}
func ReadApproval(ctx context.Context, db *sql.Conn, id protocol.ApprovalID) (protocol.ConversationApproval, error) {
	v, e := readBody[protocol.ConversationApproval](ctx, db, "SELECT body FROM agent_approvals WHERE id=?1", id.String())
	if e != nil {
		return protocol.ConversationApproval{}, e
	}
	if v == nil {
		return protocol.ConversationApproval{}, fault(protocol.ErrorCodeNotFound, "approval does not exist")
	}
	return *v, nil
}
func putApproval(ctx context.Context, db *sql.Conn, value protocol.ConversationApproval) error {
	body, err := encode(value)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "UPDATE agent_approvals SET body=?2 WHERE id=?1", value.ID.String(), body)
	return err
}
func BeginApproval(ctx context.Context, db *sql.Conn, turn protocol.TurnID, identity canonical.ToolIdentity, binding *ApprovalBinding, now time.Time) (protocol.ConversationApproval, []protocol.Event, error) {
	_ = now
	value := protocol.DefaultConversationApproval()
	run, err := activeAgentRun(ctx, db, turn)
	if err != nil {
		return value, nil, err
	}
	entry, index, _, err := locateAgentCall(ctx, db, turn, identity)
	if err != nil {
		return value, nil, err
	}
	capture, err := readTurnCapture(ctx, db, turn)
	if err != nil {
		return value, nil, err
	}
	if binding != nil {
		if binding.Tool != identity.Name || !containsPlugin(capture.Plugins, binding.Owner) {
			return value, nil, fault(protocol.ErrorCodePermissionDenied, "approval owner is outside the admitted tool catalog")
		}
	}
	value.ID = protocol.NewApprovalID()
	value.Session = run.Session
	value.Turn = turn
	value.Entry = entry
	value.Index = index
	value.State = protocol.ConversationApprovalStatePending
	value.Source = protocol.ConversationApprovalSourceUser
	value.CanRemember = binding != nil
	if binding != nil {
		if command := approvalCommand(binding, identity.Arguments); command != nil {
			if words := approvalPrefixWords(*command); len(words) > 0 {
				prefix := strings.Join(words[:min(2, len(words))], " ")
				value.Prefix = &prefix
			}
		}
	}
	if capture.Config.Permission == protocol.PermissionFull {
		value.State = protocol.ConversationApprovalStateApproved
		value.Source = protocol.ConversationApprovalSourceFull
		value.CanRemember = false
		value.Prefix = nil
	}
	if binding != nil && value.State == protocol.ConversationApprovalStatePending {
		body, e := encode(binding)
		if e != nil {
			return value, nil, e
		}
		choices, e := readBodies[protocol.ConversationApprovalChoice](ctx, db, "SELECT choice FROM agent_approval_rules WHERE session=?1 AND worktree=?2 AND binding=?3 ORDER BY rowid", run.Session.String(), run.Worktree.String(), body)
		if e != nil {
			return value, nil, e
		}
		for _, choice := range choices {
			if choice.Kind == "session" {
				value.State = protocol.ConversationApprovalStateApproved
				value.Source = protocol.ConversationApprovalSourceSessionRule
				value.Choice = &choice
				break
			}
			if choice.Kind == "prefix" {
				command := approvalCommand(binding, identity.Arguments)
				prefix, ok := choice.Data.(*string)
				if command != nil && ok && approvalPrefixMatches(*command, *prefix) {
					value.State = protocol.ConversationApprovalStateApproved
					value.Source = protocol.ConversationApprovalSourcePrefixRule
					value.Choice = &choice
				}
			}
		}
	}
	var exists bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_approvals WHERE turn=?1 AND call=?2)", turn.String(), *identity.ID).Scan(&exists); err != nil {
		return value, nil, err
	}
	if exists {
		return value, nil, fault(protocol.ErrorCodeConflict, "tool call already requested approval")
	}
	body, err := encode(value)
	if err != nil {
		return value, nil, err
	}
	owner, err := encode(binding)
	if err != nil {
		return value, nil, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed,prepared) VALUES(?1,?2,?3,?4,?5,?6,0,?7)", value.ID.String(), turn.String(), *identity.ID, body, owner, protocol.NewRequestID().String(), []byte("null"))
	if err != nil {
		return value, nil, err
	}
	events, err := activityEvent(ctx, db, run.Session)
	return value, events, err
}
func containsPlugin(values []protocol.PluginReference, want protocol.PluginReference) bool {
	a, _ := encode(want)
	for _, v := range values {
		b, _ := encode(v)
		if bytes.Equal(a, b) {
			return true
		}
	}
	return false
}
func resolveApproval(ctx context.Context, db *sql.Conn, input protocol.CommandResolveApprovalPayload) (protocol.Output, []protocol.Event, error) {
	value, err := ReadApproval(ctx, db, input.Approval)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if value.Session != input.Session {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeWrongTarget, "approval belongs to another session")
	}
	state := protocol.ConversationApprovalStateApproved
	var choice *protocol.ConversationApprovalChoice
	var reason *string
	switch input.Decision.Kind {
	case "approve":
		choice = &protocol.ConversationApprovalChoice{Kind: "once"}
	case "approve_session":
		choice = &protocol.ConversationApprovalChoice{Kind: "session"}
	case "approve_prefix":
		prefix, ok := input.Decision.Data.(*string)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid command prefix")
		}
		words := approvalPrefixWords(*prefix)
		if len(words) == 0 {
			return protocol.Output{}, nil, invalid("invalid command prefix")
		}
		selected := strings.Join(words, " ")
		if value.State == protocol.ConversationApprovalStatePending {
			binding, e := readApprovalBinding(ctx, db, value.ID)
			if e != nil {
				return protocol.Output{}, nil, e
			}
			if binding == nil {
				return protocol.Output{}, nil, invalid("tool approval cannot remember a prefix")
			}
			call, e := approvalRequest(ctx, db, value)
			if e != nil {
				return protocol.Output{}, nil, e
			}
			command := approvalCommand(binding, call.Arguments)
			if command == nil {
				return protocol.Output{}, nil, invalid("tool approval has no shell command")
			}
			if !approvalPrefixMatches(*command, selected) {
				return protocol.Output{}, nil, invalid("prefix must contain the command's first full words")
			}
		}
		choice = &protocol.ConversationApprovalChoice{Kind: "prefix", Data: &selected}
	case "deny":
		state = protocol.ConversationApprovalStateDenied
	case "deny_with_reason":
		state = protocol.ConversationApprovalStateDenied
		v, ok := input.Decision.Data.(*string)
		if !ok || strings.TrimSpace(*v) == "" || strings.ContainsRune(*v, 0) || utf8.RuneCountInString(*v) > 2000 {
			return protocol.Output{}, nil, invalid("approval reason is invalid")
		}
		reason = v
	default:
		return protocol.Output{}, nil, invalid("invalid approval decision")
	}
	if value.State != protocol.ConversationApprovalStatePending {
		a, _ := encode(value.Choice)
		b, _ := encode(choice)
		if value.State == state && bytes.Equal(a, b) && equalString(value.Reason, reason) {
			return protocol.Output{Kind: "approval", Data: value}, nil, nil
		}
		return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "approval has already closed")
	}
	run, err := activeAgentRun(ctx, db, value.Turn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if choice != nil && (choice.Kind == "session" || choice.Kind == "prefix") {
		if !value.CanRemember || value.Source != protocol.ConversationApprovalSourceUser {
			return protocol.Output{}, nil, invalid("tool approval cannot be remembered")
		}
		binding, err := readApprovalBinding(ctx, db, value.ID)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if binding == nil {
			return protocol.Output{}, nil, invalid("tool approval has no captured owner")
		}
		call, err := approvalRequest(ctx, db, value)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		capture, err := readTurnCapture(ctx, db, value.Turn)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if binding.Tool != call.Name || !containsPlugin(capture.Plugins, binding.Owner) {
			return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "approval owner is outside the admitted tool catalog")
		}
		body, e := encode(choice)
		if e != nil {
			return protocol.Output{}, nil, e
		}
		bound, e := encode(binding)
		if e != nil {
			return protocol.Output{}, nil, e
		}
		if _, err = db.ExecContext(ctx, "INSERT OR IGNORE INTO agent_approval_rules(session,worktree,binding,choice,approval) VALUES(?1,?2,?3,?4,?5)", run.Session.String(), run.Worktree.String(), bound, body, value.ID.String()); err != nil {
			return protocol.Output{}, nil, err
		}
		if choice.Kind == "session" {
			if err = applySessionApproval(ctx, db, run, *binding, capture.Config.Resource); err != nil {
				return protocol.Output{}, nil, err
			}
		}
	}
	value.State, value.Choice, value.Reason = state, choice, reason
	if err = putApproval(ctx, db, value); err != nil {
		return protocol.Output{}, nil, err
	}
	events, err := activityEvent(ctx, db, value.Session)
	return protocol.Output{Kind: "approval", Data: value}, events, err
}
func equalString(a, b *string) bool { return a == nil && b == nil || a != nil && b != nil && *a == *b }
func ExpireApproval(ctx context.Context, db *sql.Conn, id protocol.ApprovalID) (protocol.ConversationApproval, []protocol.Event, error) {
	value, err := ReadApproval(ctx, db, id)
	if err != nil {
		return value, nil, err
	}
	if value.State != protocol.ConversationApprovalStatePending {
		return value, nil, nil
	}

	value.State = protocol.ConversationApprovalStateCancelled
	if err = putApproval(ctx, db, value); err != nil {
		return value, nil, err
	}
	events, err := activityEvent(ctx, db, value.Session)
	return value, events, err
}
func closeInteractions(ctx context.Context, db *sql.Conn, turn protocol.TurnID, state string) error {
	approvals, err := readBodies[protocol.ConversationApproval](ctx, db, "SELECT body FROM agent_approvals WHERE turn=?1 AND json_extract(body,'$.state')='pending'", turn.String())
	if err != nil {
		return err
	}
	for _, v := range approvals {
		v.State = protocol.ConversationApprovalState(state)
		if err = putApproval(ctx, db, v); err != nil {
			return err
		}
	}
	questions, err := readBodies[protocol.ConversationQuestion](ctx, db, "SELECT body FROM agent_questions WHERE turn=?1 AND json_extract(body,'$.state.kind')='pending'", turn.String())
	if err != nil {
		return err
	}
	for _, v := range questions {
		v.State = protocol.ConversationQuestionState{Kind: state}
		if err = putQuestion(ctx, db, v); err != nil {
			return err
		}
	}
	return nil
}
