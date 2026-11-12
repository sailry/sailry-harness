package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ReadWorkspaceRoot captures database ownership only. Filesystem work follows
// after the caller closes its read lease.
func ReadWorkspaceRoot(ctx context.Context, db *sql.Conn, worktree protocol.WorktreeID, internal bool) (string, error) {
	var root string
	query := "SELECT w.path FROM worktrees w WHERE w.id=?1 AND NOT EXISTS(SELECT 1 FROM removed_worktrees r WHERE r.worktree=w.id) AND NOT EXISTS(SELECT 1 FROM removed_projects r WHERE r.project=w.project)"
	if internal {
		query = "SELECT path FROM worktrees WHERE id=?1"
	}
	err := db.QueryRowContext(ctx, query, worktree.String()).Scan(&root)
	if errors.Is(err, sql.ErrNoRows) {
		return "", fault(protocol.ErrorCodeNotFound, "worktree does not exist on this Node")
	}
	return root, err
}
func SupportsFilesCommand(kind string) bool {
	switch kind {
	case "read_attachment", "discard_attachment", "finish_attachment_upload", "list_file_checkpoints", "read_turn_diff", "read_turn_hunks", "read_artifact", "list_artifacts", "list_artifact_revisions":
		return true
	}
	return false
}
func ReadFilesCommand(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	switch request.Command.Kind {
	case "read_attachment":
		v := request.Command.Data.(*protocol.CommandReadAttachmentPayload)
		value, err := ReadAttachment(ctx, db, v.Worktree, v.Attachment)
		return protocol.Output{Kind: "attachment", Data: value}, err
	case "list_file_checkpoints":
		v := request.Command.Data.(*protocol.CommandListFileCheckpointsPayload)
		value, err := ListFileCheckpoints(ctx, db, v.Session, v.Turn, v.Before, v.Limit)
		return protocol.Output{Kind: "file_checkpoints", Data: value}, err
	case "read_turn_diff":
		v := request.Command.Data.(*protocol.CommandReadTurnDiffPayload)
		value, err := ReadTurnDiff(ctx, db, v.Session, v.Turn)
		return protocol.Output{Kind: "turn_diff", Data: value}, err
	case "read_turn_hunks":
		v := request.Command.Data.(*protocol.CommandReadTurnHunksPayload)
		value, err := ReadTurnHunks(ctx, db, v.Session, v.Turn, v.Path)
		return protocol.Output{Kind: "turn_hunks", Data: value}, err
	case "list_artifacts", "list_artifact_revisions", "read_artifact":
		return ReadArtifactCommand(ctx, db, request)
	}
	return protocol.Output{}, invalid("SQL file metadata command required")
}
func FinishFilesCommand(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, output protocol.Output, effectErr error) ([]protocol.Event, error) {
	if effectErr != nil {
		return nil, nil
	}
	if request.Command.Kind == "finish_attachment_upload" {
		v := request.Command.Data.(*protocol.CommandFinishAttachmentUploadPayload)
		attachment, ok := filePayload[protocol.Attachment](output.Data)
		if output.Kind != "attachment" || !ok || attachment.Spec.Worktree != v.Worktree {
			return nil, invalid("attachment publication returned another owner")
		}
		if err := InsertAttachment(ctx, db, caller, attachment); err != nil {
			return nil, err
		}
	}
	return nil, nil
}
func filePayload[T any](data any) (T, bool) {
	if v, ok := data.(T); ok {
		return v, true
	}
	if v, ok := data.(*T); ok && v != nil {
		return *v, true
	}
	var empty T
	return empty, false
}
