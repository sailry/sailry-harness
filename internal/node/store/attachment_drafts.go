package store

import (
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// InsertDraftAttachment records a file published in the profile's attachment
// store before its controller has created a conversation or worktree.
func InsertDraftAttachment(ctx context.Context, db *sql.Conn, caller protocol.NodeID, value protocol.Attachment, now time.Time) error {
	if value.Spec.Worktree != (protocol.WorktreeID{}) {
		return invalid("draft attachment has a worktree")
	}
	if err := files.ValidateAttachment(value.Spec); err != nil {
		return err
	}
	user, err := CapturedUser(ctx, db)
	if err != nil {
		return err
	}
	body, err := encode(value)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO draft_attachments(id,user,caller,created_ms,body) VALUES(?1,?2,?3,?4,?5)", value.ID.String(), user.String(), caller[:], max(now.UnixMilli(), 0), body)
	return err
}

func ReadDraftAttachment(ctx context.Context, db *sql.Conn, caller protocol.NodeID, id protocol.AttachmentID) (protocol.Attachment, error) {
	user, err := CapturedUser(ctx, db)
	if err != nil {
		return protocol.Attachment{}, err
	}
	value, err := findDraftAttachment(ctx, db, caller, user, id)
	if err != nil {
		return protocol.Attachment{}, err
	}
	if value == nil {
		return protocol.Attachment{}, attachmentMissing(id)
	}
	return *value, nil
}

func findDraftAttachment(ctx context.Context, db *sql.Conn, caller protocol.NodeID, user protocol.UserID, id protocol.AttachmentID) (*protocol.Attachment, error) {
	value, err := readBody[protocol.Attachment](ctx, db, "SELECT body FROM draft_attachments WHERE id=?1 AND user=?2 AND caller=?3", id.String(), user.String(), caller[:])
	if err == nil && value != nil && (value.ID != id || value.Spec.Worktree != (protocol.WorktreeID{})) {
		return nil, errors.New("draft attachment owner mismatch")
	}
	return value, err
}

func DiscardDraftAttachment(ctx context.Context, db *sql.Conn, caller protocol.NodeID, id protocol.AttachmentID) error {
	user, err := CapturedUser(ctx, db)
	if err != nil {
		return err
	}
	result, err := db.ExecContext(ctx, "DELETE FROM draft_attachments WHERE id=?1 AND user=?2 AND caller=?3", id.String(), user.String(), caller[:])
	if err != nil {
		return err
	}
	deleted, err := result.RowsAffected()
	if err == nil && deleted == 0 {
		return attachmentMissing(id)
	}
	return err
}

// adoptDraftAttachments moves only this controller's drafts into the actual
// worktree in the session start transaction. The normal binding check still
// checks every reference, including pre-existing worktree attachments.
func adoptDraftAttachments(ctx context.Context, db *sql.Conn, caller protocol.NodeID, worktree protocol.WorktreeID, ids []protocol.AttachmentID) error {
	if len(ids) == 0 {
		return nil
	}
	user, err := CapturedUser(ctx, db)
	if err != nil {
		return err
	}
	for _, id := range ids {
		value, err := findDraftAttachment(ctx, db, caller, user, id)
		if err != nil {
			return err
		}
		if value == nil {
			continue
		}
		value.Spec.Worktree = worktree
		if err = InsertAttachment(ctx, db, caller, *value); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "DELETE FROM draft_attachments WHERE id=?1", id.String()); err != nil {
			return err
		}
	}
	return nil
}

// PruneDraftAttachments keeps abandoned uploads for the same retry window as
// their finish receipts. A later collection removes the unreferenced files.
func PruneDraftAttachments(ctx context.Context, db *sql.Conn, now time.Time) error {
	_, err := db.ExecContext(ctx, "DELETE FROM draft_attachments WHERE created_ms<?1", now.Add(-admission.Retention).UnixMilli())
	return err
}
