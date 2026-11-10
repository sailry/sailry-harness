package store

import (
	"context"
	"database/sql"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func startSession(ctx context.Context, db *sql.Conn, node protocol.NodeID, profile string, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	draft := request.Command.Data.(*protocol.ConversationStart)
	config := draft.Config
	if config == nil {
		defaults, err := ReadDefaults(ctx, db)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		config = defaults.Config
	}
	if config == nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "Node model defaults are not configured")
	}
	if draft.Association != nil {
		if err := validateSessionAssociation(*draft.Association); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	roles, err := CaptureSessionRoles(ctx, db, node, draft.Execution, draft.Team, draft.Models)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	effective := *config
	if draft.Execution == protocol.ConversationExecutionTeam {
		effective, err = configureSessionTeam(ctx, db, effective, draft.Team)
		if err != nil {
			return protocol.Output{}, nil, err
		}
	}
	output, _, err := createSession(ctx, db, node, profile, protocol.CommandCreateSessionPayload{Project: draft.Project, Worktree: draft.Worktree, Config: &effective}, roles)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	session := output.Data.(protocol.Session)
	if draft.Association != nil {
		if _, err = db.ExecContext(ctx, "INSERT INTO session_associations(session,namespace,kind,identity) VALUES(?1,?2,?3,?4)", session.ID.String(), draft.Association.Namespace, draft.Association.Kind, draft.Association.ID); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	output, _, err = renameSession(ctx, db, protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: session.Revision, Title: draft.Title})
	if err != nil {
		return protocol.Output{}, nil, err
	}
	session = output.Data.(protocol.Session)
	if err = validateAgentInput(draft.Message); err != nil {
		return protocol.Output{}, nil, err
	}
	turn, err := admitAgentTurn(ctx, db, node, caller, request, session, protocol.ConversationRunKindTask, true)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err = bindAgentAttachments(ctx, db, turn.ID, session.Worktree, draft.Message.Attachments); err != nil {
		return protocol.Output{}, nil, err
	}
	session, err = ReadSession(ctx, db, session.ID)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	return protocol.Output{Kind: "queued_turn", Data: turn}, []protocol.Event{{Kind: "task_started", Data: protocol.EventTaskStartedPayload{Session: &session, Turn: turn}}}, nil
}

func submitTask(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	if request.Plugin != nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "task submission requires a native controller")
	}
	input := request.Command.Data.(*protocol.CommandSubmitTaskPayload)
	session, err := ReadSession(ctx, db, input.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if input.ExpectedRevision != nil {
		if err = checkRevision(session.Revision, *input.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	if err = validateAgentInput(input.Message); err != nil {
		return protocol.Output{}, nil, err
	}
	turn, err := admitAgentTurn(ctx, db, node, caller, request, session, protocol.ConversationRunKindTask, true)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err = bindAgentAttachments(ctx, db, turn.ID, session.Worktree, input.Message.Attachments); err != nil {
		return protocol.Output{}, nil, err
	}
	activity, err := readActivity(ctx, db, session.ID)
	return protocol.Output{Kind: "queued_turn", Data: turn}, []protocol.Event{{Kind: "turn_queued", Data: protocol.EventTurnQueuedPayload{Activity: &activity, Turn: turn}}}, err
}

func validateSessionAssociation(association protocol.ConversationAssociation) error {
	label := func(value string) bool {
		if value == "" || len(value) > 64 {
			return false
		}
		for _, character := range value {
			if character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || character == '-' || character == '_' || character == '.' {
				continue
			}
			return false
		}
		return true
	}
	if !label(association.Namespace) || !label(association.Kind) || strings.TrimSpace(association.ID) == "" || len(association.ID) > 256 || strings.TrimSpace(association.ID) != association.ID || strings.ContainsFunc(association.ID, unicode.IsControl) {
		return invalid("session association is invalid")
	}
	return nil
}
