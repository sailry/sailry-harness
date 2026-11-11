package store

import (
	"context"
	"database/sql"
	"runtime"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConfigureWorkspaceApproval applies the captured native operation's source
// permission policy before its approval is committed and shown to controllers.
func ConfigureWorkspaceApproval(ctx context.Context, db *sql.Conn, turn protocol.TurnID, value protocol.ConversationApproval, operation protocol.ToolOperation, command protocol.Command, changes *protocol.ChangePreview) (protocol.ConversationApproval, error) {
	capture, err := readTurnCapture(ctx, db, turn)
	if err != nil {
		return value, err
	}
	if operation == protocol.ToolOperationWriteArtifact {
		// An artifact is stored with its own conversation, so every mode and
		// permission level writes it without a prompt, a planning turn too.
		return storageApproval(ctx, db, value)
	}
	if capture.Config.Mode == protocol.WorkModePlan {
		return value, fault(protocol.ErrorCodePermissionDenied, "planning turns cannot authorize tools with side effects")
	}
	value.Changes = changes
	supported := runtime.GOOS == "darwin" || runtime.GOOS == "linux"
	if operation == protocol.ToolOperationRunCommand {
		input := command.Data.(*protocol.CommandRunCommandPayload)
		access := input.Access
		value.Access = &access
		if supported && access == protocol.ProcessAccessWorkspace {
			value.Source = protocol.ConversationApprovalSourceSandbox
			value.State = protocol.ConversationApprovalStateApproved
			value.CanRemember = false
			value.Prefix = nil
		} else if supported && access == protocol.ProcessAccessUnrestricted && capture.Config.Permission != protocol.PermissionFull {
			value.Source = protocol.ConversationApprovalSourceUser
			value.State = protocol.ConversationApprovalStatePending
			value.CanRemember = false
			value.Prefix = nil
		}
	} else if operation == protocol.ToolOperationSetValue || operation == protocol.ToolOperationDeleteValue || operation == protocol.ToolOperationStorageTransaction {
		// A plugin writes only its own namespace, so the write is authorized
		// by the storage scope instead of a user prompt.
		return storageApproval(ctx, db, value)
	} else if capture.Config.Permission == protocol.PermissionProject {
		switch operation {
		case protocol.ToolOperationWriteFile, protocol.ToolOperationEditFile, protocol.ToolOperationEditNotebook, protocol.ToolOperationDeleteFile, protocol.ToolOperationMoveFile, protocol.ToolOperationApplyChanges, protocol.ToolOperationCreateDirectory:
			value.Source = protocol.ConversationApprovalSourceProject
			value.State = protocol.ConversationApprovalStateApproved
			value.CanRemember = false
			value.Prefix = nil
		}
	}
	if err = putApproval(ctx, db, value); err != nil {
		return value, err
	}
	return value, nil
}

// storageApproval settles an approval by the scope its write is confined to.
func storageApproval(ctx context.Context, db *sql.Conn, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
	value.Source = protocol.ConversationApprovalSourceStorage
	value.State = protocol.ConversationApprovalStateApproved
	value.CanRemember = false
	value.Prefix = nil
	value.Choice = nil
	return value, putApproval(ctx, db, value)
}

// ApprovedWorkspaceRequest keeps the exact durable identity already claimed by
// the loop; preparing an effect never creates another request after approval.
func ApprovedWorkspaceRequest(ctx context.Context, db *sql.Conn, turn protocol.TurnID, approval protocol.ApprovalID, request protocol.Request) error {
	run, err := activeAgentRun(ctx, db, turn)
	if err != nil {
		return err
	}
	value, err := ReadApproval(ctx, db, approval)
	if err != nil {
		return err
	}
	if value.Turn != turn || value.Session != run.Session || value.State != protocol.ConversationApprovalStateApproved {
		return fault(protocol.ErrorCodePermissionDenied, "tool approval is unavailable")
	}
	var claimed bool
	var id string
	if err = db.QueryRowContext(ctx, "SELECT request,claimed FROM agent_approvals WHERE id=?1", approval.String()).Scan(&id, &claimed); err != nil {
		return err
	}
	if !claimed || id != request.ID.String() {
		return fault(protocol.ErrorCodePermissionDenied, "tool operation differs from its admitted approval")
	}
	if input, ok := request.Command.Data.(*protocol.CommandRunCommandPayload); ok {
		if value.Access == nil || *value.Access != input.Access {
			return fault(protocol.ErrorCodePermissionDenied, "prepared command access differs from approval")
		}
	}
	return nil
}
