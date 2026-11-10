package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type checkpointRecord struct {
	Worktree protocol.WorktreeID                     `json:"worktree"`
	Path     string                                  `json:"path"`
	Before   *protocol.ConversationCheckpointVersion `json:"before"`
	After    protocol.ConversationCheckpointVersion  `json:"after"`
	Digest   string                                  `json:"digest"`
	Set      bool                                    `json:"set,omitempty"`
}

func checkpointVersion(text string) protocol.ConversationCheckpointVersion {
	return protocol.ConversationCheckpointVersion{Revision: fmt.Sprintf("%x", blake3.Sum256([]byte(text))), Size: uint64(len(text))}
}

// CaptureFileCheckpoints reads the already admitted immutable request. Call it in
// the admission transaction before releasing the prepared filesystem effect.
func CaptureFileCheckpoints(ctx context.Context, db *sql.Conn, turn protocol.TurnID, approval protocol.ApprovalID, prepared *files.Prepared) error {
	var body []byte
	err := db.QueryRowContext(ctx, "SELECT r.body FROM agent_approvals a JOIN turns t ON t.id=a.turn JOIN requests r ON r.caller=t.caller AND r.id=a.request WHERE a.id=?1 AND a.turn=?2", approval.String(), turn.String()).Scan(&body)
	if err != nil {
		return err
	}
	var request protocol.Request
	if err = json.Unmarshal(body, &request); err != nil {
		return err
	}
	return CaptureFileCheckpointsRequest(ctx, db, turn, approval, request, prepared)
}
func CaptureFileCheckpointsRequest(ctx context.Context, db *sql.Conn, turn protocol.TurnID, approval protocol.ApprovalID, request protocol.Request, prepared *files.Prepared) error {
	if prepared == nil || prepared.Rejected != nil {
		return invalid("checkpoint requires prepared file content")
	}
	run, err := activeAgentRun(ctx, db, turn)
	if err != nil {
		return err
	}
	authorized, err := ReadApproval(ctx, db, approval)
	if err != nil {
		return err
	}
	if authorized.Turn != turn || authorized.State != protocol.ConversationApprovalStateApproved {
		return fault(protocol.ErrorCodePermissionDenied, "file checkpoint requires the approved turn")
	}
	var requestID string
	if err = db.QueryRowContext(ctx, "SELECT request FROM agent_approvals WHERE id=?1 AND turn=?2", approval.String(), turn.String()).Scan(&requestID); err != nil {
		return err
	}
	if request.ID.String() != requestID {
		return invalid("checkpoint request differs from the approved call")
	}
	a, err := json.Marshal(prepared.Command)
	if err != nil {
		return err
	}
	b, err := json.Marshal(request.Command)
	if err != nil {
		return err
	}
	if !bytes.Equal(a, b) {
		return invalid("prepared command differs from the approved request")
	}
	var target struct {
		Worktree protocol.WorktreeID `json:"worktree"`
	}
	payload, err := json.Marshal(request.Command.Data)
	if err != nil {
		return err
	}
	if err = json.Unmarshal(payload, &target); err != nil {
		return err
	}
	if target.Worktree != run.Worktree {
		return fault(protocol.ErrorCodeWrongTarget, "file checkpoint belongs to another worktree")
	}
	encoded, err := json.Marshal(request)
	if err != nil {
		return err
	}
	digest := fmt.Sprintf("%x", blake3.Sum256(encoded))
	var existing string
	err = db.QueryRowContext(ctx, "SELECT id FROM file_checkpoints WHERE approval=?1", approval.String()).Scan(&existing)
	if err == nil {
		return fault(protocol.ErrorCodeConflict, "approved file change already has a checkpoint")
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	parent := protocol.NewCheckpointID()
	position := 0
	for _, entry := range prepared.Changes {
		if entry.After == nil {
			continue
		}
		if entry.Before != nil && *entry.Before == *entry.After {
			continue
		}
		record := checkpointRecord{Worktree: run.Worktree, Path: entry.Path, After: checkpointVersion(*entry.After), Digest: digest, Set: request.Command.Kind == "apply_changes"}
		if entry.Before != nil {
			v := checkpointVersion(*entry.Before)
			record.Before = &v
		}
		body, err := encode(record)
		if err != nil {
			return err
		}
		if position == 0 {
			_, err = db.ExecContext(ctx, "INSERT INTO file_checkpoints(id,turn,approval,body,before_text,after_text) VALUES(?1,?2,?3,?4,?5,?6)", parent.String(), turn.String(), approval.String(), body, entry.Before, *entry.After)
		} else {
			_, err = db.ExecContext(ctx, "INSERT INTO file_checkpoint_entries(id,checkpoint,position,body,before_text,after_text) VALUES(?1,?2,?3,?4,?5,?6)", protocol.NewCheckpointID().String(), parent.String(), position, body, entry.Before, *entry.After)
		}
		if err != nil {
			return err
		}
		position++
	}
	return nil
}
func checkpointFile(ctx context.Context, db *sql.Conn, session protocol.SessionID, id protocol.CheckpointID) (protocol.ConversationCheckpointFile, error) {
	out := protocol.DefaultConversationCheckpointFile()
	var body, caller []byte
	var turn, request string
	err := db.QueryRowContext(ctx, `SELECT c.body,c.turn,t.caller,a.request FROM file_checkpoints c JOIN agent_approvals a ON a.id=c.approval JOIN turns t ON t.id=c.turn WHERE c.id=?1 UNION ALL SELECT e.body,c.turn,t.caller,a.request FROM file_checkpoint_entries e JOIN file_checkpoints c ON c.id=e.checkpoint JOIN agent_approvals a ON a.id=c.approval JOIN turns t ON t.id=c.turn WHERE e.id=?1`, id.String()).Scan(&body, &turn, &caller, &request)
	if errors.Is(err, sql.ErrNoRows) {
		return out, fault(protocol.ErrorCodeNotFound, "file checkpoint does not exist")
	}
	if err != nil {
		return out, err
	}
	var record checkpointRecord
	if err = json.Unmarshal(body, &record); err != nil {
		return out, err
	}
	turnID, err := protocol.ParseTurnID(turn)
	if err != nil {
		return out, err
	}
	if _, err = visibleAgentRun(ctx, db, session, turnID); err != nil {
		return out, err
	}
	if len(caller) != 32 {
		return out, errors.New("invalid checkpoint caller")
	}
	var node protocol.NodeID
	copy(node[:], caller)
	requestID, err := protocol.ParseRequestID(request)
	if err != nil {
		return out, err
	}
	out.Outcome, err = admission.Inspect(ctx, db, node, requestID, record.Digest)
	out.ID, out.Turn, out.Worktree, out.Path, out.Before, out.After = id, turnID, record.Worktree, record.Path, record.Before, record.After
	return out, err
}
func ListFileCheckpoints(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, before *protocol.CheckpointID, limit uint16) (protocol.ConversationCheckpointPage, error) {
	out := protocol.ConversationCheckpointPage{Session: session, Turn: turn, Files: []protocol.ConversationCheckpointFile{}}
	if _, err := visibleAgentRun(ctx, db, session, turn); err != nil {
		return out, err
	}
	if limit < 1 || limit > 100 {
		return out, invalid("file checkpoint page limit must be between 1 and 100")
	}
	query := `SELECT id,owner,position FROM (SELECT c.id,c.rowid AS owner,0 AS position FROM file_checkpoints c WHERE c.turn=?1 UNION ALL SELECT e.id,c.rowid AS owner,e.position FROM file_checkpoint_entries e JOIN file_checkpoints c ON c.id=e.checkpoint WHERE c.turn=?1)`
	boundaryOwner, boundaryPosition := int64(1<<63-1), int64(1<<63-1)
	if before != nil {
		err := db.QueryRowContext(ctx, query+" WHERE id=?2", turn.String(), before.String()).Scan(new(string), &boundaryOwner, &boundaryPosition)
		if errors.Is(err, sql.ErrNoRows) {
			return out, fault(protocol.ErrorCodeWrongTarget, "file checkpoint cursor belongs to another turn")
		}
		if err != nil {
			return out, err
		}
	}
	rows, err := db.QueryContext(ctx, query+" WHERE owner<?2 OR (owner=?2 AND position<?3) ORDER BY owner DESC,position DESC LIMIT ?4", turn.String(), boundaryOwner, boundaryPosition, int(limit)+1)
	if err != nil {
		return out, err
	}
	var ids []protocol.CheckpointID
	for rows.Next() {
		var id string
		var owner, position int64
		if err = rows.Scan(&id, &owner, &position); err != nil {
			rows.Close()
			return out, err
		}
		value, err := protocol.ParseCheckpointID(id)
		if err != nil {
			rows.Close()
			return out, err
		}
		ids = append(ids, value)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return out, err
	}
	more := len(ids) > int(limit)
	for _, id := range ids[:min(len(ids), int(limit))] {
		file, err := checkpointFile(ctx, db, session, id)
		if err != nil {
			return out, err
		}
		out.Files = append(out.Files, file)
	}
	if more {
		out.Next = &out.Files[len(out.Files)-1].ID
	}
	return out, nil
}
func CheckpointContent(ctx context.Context, db *sql.Conn, session protocol.SessionID, id protocol.CheckpointID) (protocol.ConversationCheckpointContent, error) {
	file, err := checkpointFile(ctx, db, session, id)
	out := protocol.ConversationCheckpointContent{Session: session, File: file}
	if err != nil {
		return out, err
	}
	var before sql.NullString
	err = db.QueryRowContext(ctx, "SELECT before_text,after_text FROM file_checkpoints WHERE id=?1 UNION ALL SELECT before_text,after_text FROM file_checkpoint_entries WHERE id=?1", id.String()).Scan(&before, &out.After)
	if err != nil {
		return out, err
	}
	if before.Valid {
		out.Before = &before.String
	}
	if checkpointVersion(out.After) != file.After || before.Valid != (file.Before != nil) || before.Valid && checkpointVersion(before.String) != *file.Before {
		return out, errors.New("checkpoint content does not match its revision")
	}
	return out, nil
}
func ReadFileCheckpoint(ctx context.Context, db *sql.Conn, session protocol.SessionID, id protocol.CheckpointID) (protocol.ConversationCheckpointContent, error) {
	file, err := checkpointFile(ctx, db, session, id)
	if err != nil {
		return protocol.ConversationCheckpointContent{}, err
	}
	size := file.After.Size
	if file.Before != nil {
		size += file.Before.Size
	}
	if size > protocol.MAX_FRAME_BYTES/8 {
		return protocol.ConversationCheckpointContent{}, invalid("checkpoint content exceeds the response limit; use the turn diff or restore the checkpoint")
	}
	return CheckpointContent(ctx, db, session, id)
}
func PrepareCheckpointRestore(ctx context.Context, db *sql.Conn, session protocol.SessionID, id protocol.CheckpointID, worktree protocol.WorktreeID, root string) (protocol.ConversationCheckpointContent, error) {
	content, err := CheckpointContent(ctx, db, session, id)
	if err != nil {
		return content, err
	}
	captured, err := ReadWorkspaceRoot(ctx, db, worktree, false)
	if err != nil {
		return content, err
	}
	if content.File.Worktree != worktree || captured != root {
		return content, fault(protocol.ErrorCodeWrongTarget, "file checkpoint worktree changed")
	}
	run, err := ReadAgentRun(ctx, db, content.File.Turn)
	if err != nil {
		return content, err
	}
	if run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping {
		return content, fault(protocol.ErrorCodeBusy, "file checkpoint turn is still active")
	}
	receipt := files.CheckpointReceipt(content.File)
	if receipt == protocol.ConversationCheckpointReceiptPending {
		return content, fault(protocol.ErrorCodeBusy, "checkpoint write is still pending")
	}
	if receipt == protocol.ConversationCheckpointReceiptAbsent {
		return content, fault(protocol.ErrorCodeConflict, "checkpoint write did not complete")
	}
	return content, nil
}
