package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"runtime"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testCommandApprovalRules follows run_command approvals through the sandbox
// setting, a remembered prefix and the command that is finally prepared.
func testCommandApprovalRules(t *testing.T) {
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	operation := protocol.ToolOperationRunCommand
	binding := &ApprovalBinding{Tool: "run_command", Operation: &operation}
	sequence := 0
	// begin records one model call and settles its approval source.
	begin := func(db *sql.Conn, command string, access protocol.ProcessAccess) (protocol.ConversationApproval, error) {
		sequence++
		id := fmt.Sprintf("call-%d", sequence)
		arguments, err := encode(map[string]any{"command": command})
		if err != nil {
			return protocol.ConversationApproval{}, err
		}
		record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: binding.Tool, Arguments: arguments}})), now)
		record.Provenance.Author = "sailry"
		if _, _, err = AppendCanonical(ctx, db, session.ID, record); err != nil {
			return protocol.ConversationApproval{}, err
		}
		value, _, err := BeginApproval(ctx, db, turn, canonical.ToolIdentity{ID: &id, Name: binding.Tool, Arguments: arguments}, binding, now)
		if err != nil {
			return value, err
		}
		payload := protocol.DefaultCommandRunCommandPayload()
		payload.Turn, payload.Command, payload.Access = turn, command, access
		return ConfigureWorkspaceApproval(ctx, db, turn, value, operation, protocol.Command{Kind: "run_command", Data: &payload}, nil)
	}
	// prepared claims the approval and checks the command about to run.
	prepared := func(db *sql.Conn, value protocol.ConversationApproval, command string) error {
		var id string
		if err := db.QueryRowContext(ctx, "UPDATE agent_approvals SET claimed=1 WHERE id=?1 RETURNING request", value.ID.String()).Scan(&id); err != nil {
			return err
		}
		payload := protocol.DefaultCommandRunCommandPayload()
		payload.Turn, payload.Command, payload.Access = turn, command, protocol.ProcessAccessUnrestricted
		request := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &payload})
		var err error
		if request.ID, err = protocol.ParseRequestID(id); err != nil {
			return err
		}
		return ApprovedWorkspaceRequest(ctx, db, turn, value.ID, request)
	}
	sandbox := func(db *sql.Conn, enabled bool) error {
		current, err := ReadSandboxSettings(ctx, db)
		if err != nil {
			return err
		}
		current.Enabled = enabled
		_, err = SaveSandboxSettings(ctx, db, current)
		return err
	}
	pending, user := protocol.ConversationApprovalStatePending, protocol.ConversationApprovalSourceUser
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		confined := runtime.GOOS == "darwin" || runtime.GOOS == "linux"
		if confined {
			value, err := begin(db, "git status", protocol.ProcessAccessWorkspace)
			if err != nil || value.Source != protocol.ConversationApprovalSourceSandbox || value.State != protocol.ConversationApprovalStateApproved {
				return fmt.Errorf("confined command was not approved by the sandbox: %+v, %v", value, err)
			}
			value, err = begin(db, "git status", protocol.ProcessAccessUnrestricted)
			if err != nil || value.Source != user || value.State != pending || value.CanRemember || value.Prefix != nil {
				return fmt.Errorf("leaving the sandbox must ask every time: %+v, %v", value, err)
			}
		}
		if err := sandbox(db, false); err != nil {
			return err
		}
		// Without a sandbox the command is an ordinary approval.
		first, err := begin(db, "git status --short", protocol.ProcessAccessUnrestricted)
		if err != nil || first.Source != user || first.State != pending || !first.CanRemember || first.Prefix == nil || *first.Prefix != "git status" {
			return fmt.Errorf("command without a sandbox cannot be remembered: %+v, %v", first, err)
		}
		output, _, err := resolveApproval(ctx, db, protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: first.ID, Decision: protocol.ConversationDecision{Kind: "approve_prefix", Data: first.Prefix}})
		if err != nil {
			return err
		}
		first = output.Data.(protocol.ConversationApproval)
		if err := prepared(db, first, "git status --short"); err != nil {
			return fmt.Errorf("approved command was refused: %w", err)
		}
		// A handler that prepares another command gains nothing from the prefix.
		var refused *protocol.Fault
		if err := prepared(db, first, "rm -rf build"); !errors.As(err, &refused) || refused.Code != protocol.ErrorCodePermissionDenied || refused.Message != "prepared command is outside the approved prefix" {
			return fmt.Errorf("prepared command left its prefix: %v", err)
		}
		matched, err := begin(db, "git status -v", protocol.ProcessAccessUnrestricted)
		if err != nil || matched.Source != protocol.ConversationApprovalSourcePrefixRule || matched.State != protocol.ConversationApprovalStateApproved {
			return fmt.Errorf("prefix rule did not answer: %+v, %v", matched, err)
		}
		other, err := begin(db, "git push", protocol.ProcessAccessUnrestricted)
		if err != nil || other.Source != user || other.State != pending {
			return fmt.Errorf("prefix rule answered another command: %+v, %v", other, err)
		}
		if !confined {
			return nil
		}
		// The rule never lets a command out of a sandbox turned on again.
		if err := sandbox(db, true); err != nil {
			return err
		}
		escape, err := begin(db, "git status", protocol.ProcessAccessUnrestricted)
		if err != nil || escape.Source != user || escape.State != pending || escape.CanRemember || escape.Choice != nil {
			return fmt.Errorf("prefix rule left the sandbox: %+v, %v", escape, err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

// testCommandTargetApprovals settles stop and write approvals by the access
// their target command was admitted with in the same session.
func testCommandTargetApprovals(t *testing.T) {
	if runtime.GOOS != "darwin" && runtime.GOOS != "linux" {
		t.Skip("workspace sandbox is unavailable on this platform")
	}
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	launch := func(db *sql.Conn, access protocol.ProcessAccess) (protocol.RequestID, error) {
		payload := protocol.DefaultCommandRunCommandPayload()
		payload.Turn, payload.Command, payload.Access, payload.Background = turn, "npm run dev", access, true
		request := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &payload})
		body, err := encode(request)
		if err == nil {
			_, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body)
		}
		return request.ID, err
	}
	sequence := 0
	settle := func(db *sql.Conn, operation protocol.ToolOperation, target protocol.SessionID, id protocol.RequestID) (protocol.ConversationApproval, error) {
		sequence++
		call := fmt.Sprintf("target-%d", sequence)
		name, command := "stop_command", protocol.Command{Kind: "stop_command", Data: &protocol.CommandStopCommandPayload{Session: target, ID: id}}
		if operation == protocol.ToolOperationWriteCommand {
			name, command = "write_command", protocol.Command{Kind: "write_command", Data: &protocol.CommandWriteCommandPayload{Session: target, ID: id, Input: "q\n"}}
		}
		arguments, err := encode(map[string]any{"id": id})
		if err != nil {
			return protocol.ConversationApproval{}, err
		}
		record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &canonical.ToolRequest{ID: call, Name: name, Arguments: arguments}})), now)
		record.Provenance.Author = "sailry"
		if _, _, err = AppendCanonical(ctx, db, session.ID, record); err != nil {
			return protocol.ConversationApproval{}, err
		}
		value, _, err := BeginApproval(ctx, db, turn, canonical.ToolIdentity{ID: &call, Name: name, Arguments: arguments}, &ApprovalBinding{Tool: name, Operation: &operation}, now)
		if err != nil {
			return value, err
		}
		return ConfigureWorkspaceApproval(ctx, db, turn, value, operation, command, nil)
	}
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		confined, err := launch(db, protocol.ProcessAccessWorkspace)
		if err != nil {
			return err
		}
		unrestricted, err := launch(db, protocol.ProcessAccessUnrestricted)
		if err != nil {
			return err
		}
		for _, operation := range []protocol.ToolOperation{protocol.ToolOperationStopCommand, protocol.ToolOperationWriteCommand} {
			value, err := settle(db, operation, session.ID, confined)
			if err != nil || value.Source != protocol.ConversationApprovalSourceSandbox || value.State != protocol.ConversationApprovalStateApproved {
				return fmt.Errorf("%s of a sandboxed command asked: %+v, %v", operation, value, err)
			}
			for _, row := range []struct {
				name    string
				session protocol.SessionID
				id      protocol.RequestID
			}{{"unrestricted", session.ID, unrestricted}, {"other session", protocol.NewSessionID(), confined}, {"unknown", session.ID, protocol.NewRequestID()}} {
				value, err := settle(db, operation, row.session, row.id)
				if err != nil || value.State != protocol.ConversationApprovalStatePending || value.Source != protocol.ConversationApprovalSourceUser {
					return fmt.Errorf("%s of an %s command was settled without the user: %+v, %v", operation, row.name, value, err)
				}
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
