package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func attachmentMissing(id protocol.AttachmentID) error {
	return fault(protocol.ErrorCodeNotFound, "attachment "+id.String()+" is unavailable")
}
func ReadAttachment(ctx context.Context, db *sql.Conn, worktree protocol.WorktreeID, id protocol.AttachmentID) (protocol.Attachment, error) {
	value, err := readBody[protocol.Attachment](ctx, db, "SELECT body FROM attachments WHERE id=?1 AND worktree=?2", id.String(), worktree.String())
	if err != nil {
		return protocol.Attachment{}, err
	}
	if value == nil {
		return protocol.Attachment{}, attachmentMissing(id)
	}
	if value.ID != id || value.Spec.Worktree != worktree {
		return protocol.Attachment{}, errors.New("attachment owner mismatch")
	}
	return *value, nil
}
func InsertAttachment(ctx context.Context, db *sql.Conn, caller protocol.NodeID, attachment protocol.Attachment) error {
	if err := files.ValidateAttachment(attachment.Spec); err != nil {
		return err
	}
	body, err := encode(attachment)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO attachments(id,worktree,caller,body) VALUES(?1,?2,?3,?4)", attachment.ID.String(), attachment.Spec.Worktree.String(), caller[:], body)
	return err
}
func DiscardAttachment(ctx context.Context, db *sql.Conn, caller protocol.NodeID, worktree protocol.WorktreeID, id protocol.AttachmentID) error {
	var owner []byte
	err := db.QueryRowContext(ctx, "SELECT caller FROM attachments WHERE id=?1 AND worktree=?2", id.String(), worktree.String()).Scan(&owner)
	if errors.Is(err, sql.ErrNoRows) || err == nil && !bytes.Equal(owner, caller[:]) {
		return attachmentMissing(id)
	}
	if err != nil {
		return err
	}
	var used bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM turn_attachments WHERE attachment=?1)", id.String()).Scan(&used); err != nil {
		return err
	}
	if used {
		return fault(protocol.ErrorCodeConflict, "attachment is used by a conversation")
	}
	_, err = db.ExecContext(ctx, "DELETE FROM attachments WHERE id=?1", id.String())
	return err
}
func BindTurnAttachments(ctx context.Context, db *sql.Conn, turn protocol.TurnID, ids []protocol.AttachmentID) error {
	var worktree string
	if err := db.QueryRowContext(ctx, "SELECT s.worktree FROM turns t JOIN sessions s ON s.id=t.session WHERE t.id=?1", turn.String()).Scan(&worktree); err != nil {
		return err
	}
	owner, err := protocol.ParseWorktreeID(worktree)
	if err != nil {
		return err
	}
	seen := map[protocol.AttachmentID]bool{}
	for _, id := range ids {
		if seen[id] {
			return invalid("duplicate attachment reference")
		}
		seen[id] = true
		if _, err = ReadAttachment(ctx, db, owner, id); err != nil {
			return err
		}
	}
	if _, err = db.ExecContext(ctx, "DELETE FROM turn_attachments WHERE turn=?1", turn.String()); err != nil {
		return err
	}
	for index, id := range ids {
		if _, err = db.ExecContext(ctx, "INSERT INTO turn_attachments(turn,attachment,position) VALUES(?1,?2,?3)", turn.String(), id.String(), index); err != nil {
			return err
		}
	}
	return nil
}
func ListTurnAttachments(ctx context.Context, db *sql.Conn, turn protocol.TurnID) ([]protocol.Attachment, error) {
	values, err := readBodies[protocol.Attachment](ctx, db, "SELECT a.body FROM turn_attachments r JOIN attachments a ON a.id=r.attachment WHERE r.turn=?1 ORDER BY r.position", turn.String())
	if values == nil {
		values = []protocol.Attachment{}
	}
	return values, err
}
func GetTurnAttachment(ctx context.Context, db *sql.Conn, turn protocol.TurnID, id protocol.AttachmentID) (protocol.Attachment, error) {
	var worktree string
	err := db.QueryRowContext(ctx, "SELECT a.worktree FROM turn_attachments r JOIN attachments a ON a.id=r.attachment WHERE r.turn=?1 AND r.attachment=?2", turn.String(), id.String()).Scan(&worktree)
	if errors.Is(err, sql.ErrNoRows) {
		return protocol.Attachment{}, attachmentMissing(id)
	}
	if err != nil {
		return protocol.Attachment{}, err
	}
	owner, err := protocol.ParseWorktreeID(worktree)
	if err != nil {
		return protocol.Attachment{}, err
	}
	return ReadAttachment(ctx, db, owner, id)
}
func ResolveTurnAttachment(ctx context.Context, db *sql.Conn, turn protocol.TurnID, id protocol.AttachmentID) (protocol.Attachment, error) {
	var body []byte
	err := db.QueryRowContext(ctx, `SELECT a.body FROM attachments a WHERE a.id=?2 AND EXISTS(SELECT 1 FROM turn_attachments r JOIN conversation_turns h ON h.turn=r.turn JOIN turns t ON t.session=h.session JOIN agent_runs origin ON origin.turn=r.turn WHERE t.id=?1 AND r.attachment=a.id AND (r.turn=t.id OR (json_extract(origin.body,'$.joined')=t.id AND json_extract(origin.body,'$.status')='continued') OR (json_extract(origin.body,'$.status') IN ('completed','continued','failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=origin.turn))))`, turn.String(), id.String()).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return protocol.Attachment{}, attachmentMissing(id)
	}
	if err != nil {
		return protocol.Attachment{}, err
	}
	var value protocol.Attachment
	err = json.Unmarshal(body, &value)
	if err == nil && value.ID != id {
		return value, errors.New("attachment owner mismatch")
	}
	return value, err
}
func RetainedAttachments(ctx context.Context, db *sql.Conn) (map[protocol.AttachmentID]bool, error) {
	rows, err := db.QueryContext(ctx, "SELECT id FROM attachments")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := map[protocol.AttachmentID]bool{}
	for rows.Next() {
		var value string
		if err = rows.Scan(&value); err != nil {
			return nil, err
		}
		id, err := protocol.ParseAttachmentID(value)
		if err != nil {
			return nil, err
		}
		out[id] = true
	}
	return out, rows.Err()
}
