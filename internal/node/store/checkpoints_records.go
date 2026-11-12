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
	// Restored is set once restore_file_checkpoint undid the change. It
	// lives in the checkpoint's row, so it goes when the row is pruned with
	// its removed session.
	Restored bool `json:"restored,omitempty"`
}

// turnCheckpoints lists the files of turn ?1 in the order their changes were
// captured: the approval's row, then its later files.
const turnCheckpoints = `SELECT c.id,c.body,c.rowid AS owner,0 AS position FROM file_checkpoints c WHERE c.turn=?1
UNION ALL SELECT e.id,e.body,c.rowid AS owner,e.position FROM file_checkpoint_entries e JOIN file_checkpoints c ON c.id=e.checkpoint WHERE c.turn=?1`

func checkpointVersion(text string) protocol.ConversationCheckpointVersion {
	return protocol.ConversationCheckpointVersion{Revision: fmt.Sprintf("%x", blake3.Sum256([]byte(text))), Size: uint64(len(text))}
}

// CaptureFileCheckpointsRequest records what the approved request's prepared
// change replaces. Call it in the admission transaction before releasing the
// prepared filesystem effect.
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
			var row uint64
			if row, err = NextCounter(ctx, db, CounterCheckpoints); err != nil {
				return err
			}
			_, err = db.ExecContext(ctx, "INSERT INTO file_checkpoints(rowid,id,turn,approval,body,before_text,after_text) VALUES(?1,?2,?3,?4,?5,?6,?7)", row, parent.String(), turn.String(), approval.String(), body, entry.Before, *entry.After)
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
	out, err := readCheckpointFile(ctx, db, id)
	if err != nil {
		return out, err
	}
	_, err = visibleAgentRun(ctx, db, session, out.Turn)
	return out, err
}

// readCheckpointFile loads a checkpoint without checking that its turn is
// visible in a session; the caller has checked the turn.
func readCheckpointFile(ctx context.Context, db *sql.Conn, id protocol.CheckpointID) (protocol.ConversationCheckpointFile, error) {
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
	out.Restored = record.Restored
	return out, err
}

// MarkCheckpointRestored records that a checkpoint's change was undone, in
// the transaction that stores the restore's receipt, so the fact survives a
// restart and reaches every controller with the checkpoint list.
func MarkCheckpointRestored(ctx context.Context, db *sql.Conn, id protocol.CheckpointID) error {
	for _, table := range []string{"file_checkpoints", "file_checkpoint_entries"} {
		// Only this member changes; the rest of the record is kept as stored.
		record, err := readBody[map[string]json.RawMessage](ctx, db, "SELECT body FROM "+table+" WHERE id=?1", id.String())
		if err != nil {
			return err
		}
		if record == nil {
			continue
		}
		(*record)["restored"] = json.RawMessage("true")
		body, err := encode(record)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "UPDATE "+table+" SET body=?2 WHERE id=?1", id.String(), body)
		return err
	}
	return fault(protocol.ErrorCodeNotFound, "file checkpoint does not exist")
}
func ListFileCheckpoints(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, before *protocol.CheckpointID, limit uint16) (protocol.ConversationCheckpointPage, error) {
	out := protocol.ConversationCheckpointPage{Session: session, Turn: turn, Files: []protocol.ConversationCheckpointFile{}}
	if _, err := visibleAgentRun(ctx, db, session, turn); err != nil {
		return out, err
	}
	if limit < 1 || limit > 100 {
		return out, invalid("file checkpoint page limit must be between 1 and 100")
	}
	query := `SELECT id,owner,position FROM (` + turnCheckpoints + `)`
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
		file, err := readCheckpointFile(ctx, db, id)
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
	if err != nil {
		return protocol.ConversationCheckpointContent{Session: session, File: file}, err
	}
	return checkpointContent(ctx, db, session, file)
}
func checkpointContent(ctx context.Context, db *sql.Conn, session protocol.SessionID, file protocol.ConversationCheckpointFile) (protocol.ConversationCheckpointContent, error) {
	out := protocol.ConversationCheckpointContent{Session: session, File: file}
	id := file.ID
	var before sql.NullString
	err := db.QueryRowContext(ctx, "SELECT before_text,after_text FROM file_checkpoints WHERE id=?1 UNION ALL SELECT before_text,after_text FROM file_checkpoint_entries WHERE id=?1", id.String()).Scan(&before, &out.After)
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

// PrepareCheckpointRestore captures a checkpoint's content for a restore and
// the versions that already count as restored: what the file held before this
// change or before an earlier change of the same turn to the same file.
func PrepareCheckpointRestore(ctx context.Context, db *sql.Conn, session protocol.SessionID, id protocol.CheckpointID, worktree protocol.WorktreeID, root string) (protocol.ConversationCheckpointContent, []*protocol.ConversationCheckpointVersion, error) {
	content, err := CheckpointContent(ctx, db, session, id)
	if err != nil {
		return content, nil, err
	}
	captured, err := ReadWorkspaceRoot(ctx, db, worktree, false)
	if err != nil {
		return content, nil, err
	}
	if content.File.Worktree != worktree || captured != root {
		return content, nil, fault(protocol.ErrorCodeWrongTarget, "file checkpoint worktree changed")
	}
	run, err := ReadAgentRun(ctx, db, content.File.Turn)
	if err != nil {
		return content, nil, err
	}
	if run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping {
		return content, nil, fault(protocol.ErrorCodeBusy, "file checkpoint turn is still active")
	}
	receipt := files.CheckpointReceipt(content.File)
	if receipt == protocol.ConversationCheckpointReceiptPending {
		return content, nil, fault(protocol.ErrorCodeBusy, "checkpoint write is still pending")
	}
	if receipt == protocol.ConversationCheckpointReceiptAbsent {
		return content, nil, fault(protocol.ErrorCodeConflict, "checkpoint write did not complete")
	}
	records, err := readBodies[checkpointRecord](ctx, db, `SELECT body FROM (`+turnCheckpoints+`) WHERE owner<(SELECT owner FROM (`+turnCheckpoints+`) WHERE id=?2)
OR owner=(SELECT owner FROM (`+turnCheckpoints+`) WHERE id=?2) AND position<=(SELECT position FROM (`+turnCheckpoints+`) WHERE id=?2) ORDER BY owner,position`, content.File.Turn.String(), id.String())
	if err != nil {
		return content, nil, err
	}
	var settled []*protocol.ConversationCheckpointVersion
	for _, record := range records {
		if record.Worktree == content.File.Worktree && record.Path == content.File.Path {
			settled = append(settled, record.Before)
		}
	}
	return content, settled, nil
}
