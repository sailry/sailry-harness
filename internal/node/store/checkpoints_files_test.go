package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestFileCheckpointAndAttachmentOwnership(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	var root string
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		root, err = ReadWorkspaceRoot(ctx, db, session.Worktree, false)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		t.Fatal(err)
	}
	canonical, err := filepath.EvalSymlinks(root)
	if err != nil {
		t.Fatal(err)
	}
	if err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "UPDATE worktrees SET path=?2 WHERE id=?1", session.Worktree.String(), canonical)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	root = canonical
	if err := os.WriteFile(filepath.Join(root, "note.txt"), []byte("before\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	revision := fmt.Sprintf("%x", blake3.Sum256([]byte("before\n")))
	request := protocol.NewRequest(node, protocol.Command{Kind: "write_file", Data: &protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "note.txt", Text: "after\n", ExpectedRevision: &revision}})
	prepared, err := files.Prepare(ctx, root, request.Command, core.RestrictedFiles)
	if err != nil {
		t.Fatal(err)
	}
	approval := protocol.DefaultConversationApproval()
	approval.ID, approval.Session, approval.Turn = protocol.NewApprovalID(), session.ID, turn
	approval.State, approval.Source = protocol.ConversationApprovalStateApproved, protocol.ConversationApprovalSourceUser
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		body, err := encode(request)
		if err != nil {
			return err
		}
		if _, err = admission.Admit(ctx, db, node, request, body); err != nil {
			return err
		}
		body, err = encode(approval)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,'files-fixture',?3,?4,?5,1)", approval.ID.String(), turn.String(), body, []byte("null"), request.ID.String()); err != nil {
			return err
		}
		return CaptureFileCheckpointsRequest(ctx, db, turn, approval.ID, request, prepared)
	})
	if err != nil {
		t.Fatal(err)
	}
	output, err := files.Apply(ctx, root, prepared)
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := admission.Complete(ctx, db, node, request.ID, link.Response{Output: output}); err != nil {
			return err
		}
		_, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	var content protocol.ConversationCheckpointContent
	err = storage.Node.Read(ctx, func(db *sql.Conn) error {
		page, err := ListFileCheckpoints(ctx, db, session.ID, turn, nil, 100)
		if err != nil {
			return err
		}
		if len(page.Files) != 1 || files.CheckpointReceipt(page.Files[0]) != protocol.ConversationCheckpointReceiptWritten {
			return fmt.Errorf("checkpoint receipt is not a confirmed write: %+v", page)
		}
		content, err = PrepareCheckpointRestore(ctx, db, session.ID, page.Files[0].ID, session.Worktree, root)
		if err != nil {
			return err
		}
		diff, err := ReadTurnDiff(ctx, db, session.ID, turn)
		if err != nil {
			return err
		}
		if diff.Partial || len(diff.Files) != 1 || diff.Files[0].Additions != 1 || diff.Files[0].Deletions != 1 {
			return fmt.Errorf("turn diff: %+v", diff)
		}
		call, result := "files-fixture", json.RawMessage(`{}`)
		raw := &rawAgentTool{ID: &call, Result: &result}
		detail := protocol.ToolDetail{Resolved: &protocol.ToolDetailResolved{Output: &protocol.ToolDetailOutput{Preview: "replacement fragment"}}}
		if err := toolCheckpointDetail(ctx, db, turn, raw, &detail); err != nil {
			return err
		}
		if body := detail.Resolved.Output.Body; body == nil || body.Path == nil || *body.Path != "note.txt" || body.Diff == nil || *body.Diff != protocol.ToolDisplayProjectionFormatUnified || !strings.Contains(body.Text, "-before\n+after\n") || detail.Resolved.Output.Preview != "" {
			return fmt.Errorf("tool detail did not use its saved checkpoint: %+v", detail.Resolved.Output)
		}
		raw.Failure = &protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime}
		detail.Resolved.Output.Body = nil
		if err := toolCheckpointDetail(ctx, db, turn, raw, &detail); err != nil || detail.Resolved.Output.Body != nil {
			return fmt.Errorf("failed tool projected a successful checkpoint diff: %v", err)
		}
		hunks, err := ReadTurnHunks(ctx, db, session.ID, turn, "note.txt")
		if err != nil {
			return err
		}
		if len(hunks.Hunks) != 1 {
			return fmt.Errorf("turn hunks: %+v", hunks)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = files.RestoreCheckpoint(ctx, root, content); err != nil {
		t.Fatal(err)
	}
	restored, err := os.ReadFile(filepath.Join(root, "note.txt"))
	if err != nil || string(restored) != "before\n" {
		t.Fatalf("restored %q %v", restored, err)
	}
	if _, err = files.RestoreCheckpoint(ctx, root, content); err == nil {
		t.Fatal("restore overwrote an independently changed revision")
	}
	attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Worktree: session.Worktree, Name: "input.txt", MediaType: "text/plain", Size: 5, Revision: fmt.Sprintf("%x", blake3.Sum256([]byte("input")))}}
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := InsertAttachment(ctx, db, node, attachment); err != nil {
			return err
		}
		other := node
		other[0]++
		if err := DiscardAttachment(ctx, db, other, session.Worktree, attachment.ID); err == nil {
			return fmt.Errorf("another caller discarded an attachment")
		}
		if err := BindTurnAttachments(ctx, db, turn, []protocol.AttachmentID{attachment.ID}); err != nil {
			return err
		}
		if err := DiscardAttachment(ctx, db, node, session.Worktree, attachment.ID); err == nil {
			return fmt.Errorf("a bound attachment was discarded")
		}
		resolved, err := ResolveTurnAttachment(ctx, db, turn, attachment.ID)
		if err != nil {
			return err
		}
		if resolved.ID != attachment.ID {
			return fmt.Errorf("attachment binding changed identity")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	// A checkpoint draws its row from the watermark, so its profile reopens.
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	reopened := reopenSchemaFixture(t, storage.Profile, node)
	if got := schemaInteger(t, reopened.Node, "SELECT value FROM counters WHERE namespace='file_checkpoints'"); got != 1 {
		t.Fatalf("checkpoint watermark: %d", got)
	}
	// Checkpoints recorded before that took SQLite's own rows; startup
	// raises the watermark over them instead of refusing the profile.
	schemaExecute(t, reopened.Node, "DELETE FROM counters WHERE namespace='file_checkpoints'")
	if err := reopened.Close(); err != nil {
		t.Fatal(err)
	}
	repaired := reopenSchemaFixture(t, storage.Profile, node)
	if got := schemaInteger(t, repaired.Node, "SELECT value FROM counters WHERE namespace='file_checkpoints'"); got != 1 {
		t.Fatalf("repaired checkpoint watermark: %d", got)
	}
	if backups, err := filepath.Glob(filepath.Join(storage.Profile, "storage", "backups", "node-before-upgrade-*")); err != nil || len(backups) != 1 {
		t.Fatalf("watermark repair backups: %v, %v", backups, err)
	}
}
