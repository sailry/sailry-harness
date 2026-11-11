package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
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
	var settled []*protocol.ConversationCheckpointVersion
	err = storage.Node.Read(ctx, func(db *sql.Conn) error {
		page, err := ListFileCheckpoints(ctx, db, session.ID, turn, nil, 100)
		if err != nil {
			return err
		}
		if len(page.Files) != 1 || files.CheckpointReceipt(page.Files[0]) != protocol.ConversationCheckpointReceiptWritten {
			return fmt.Errorf("checkpoint receipt is not a confirmed write: %+v", page)
		}
		content, settled, err = PrepareCheckpointRestore(ctx, db, session.ID, page.Files[0].ID, session.Worktree, root)
		if err != nil {
			return err
		}
		if len(settled) != 1 || settled[0] == nil || *settled[0] != *page.Files[0].Before {
			return fmt.Errorf("restored versions: %+v", settled)
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
	if _, err = files.RestoreCheckpoint(ctx, root, content, settled); err != nil {
		t.Fatal(err)
	}
	restored, err := os.ReadFile(filepath.Join(root, "note.txt"))
	if err != nil || string(restored) != "before\n" {
		t.Fatalf("restored %q %v", restored, err)
	}
	// An Undo repeated after a restart finds the file restored and leaves it.
	stamp, err := os.Stat(filepath.Join(root, "note.txt"))
	if err != nil {
		t.Fatal(err)
	}
	again, err := files.RestoreCheckpoint(ctx, root, content, settled)
	if err != nil || again.Revision == nil || *again.Revision != content.File.Before.Revision {
		t.Fatalf("repeated restore: %+v, %v", again, err)
	}
	if after, err := os.Stat(filepath.Join(root, "note.txt")); err != nil || !after.ModTime().Equal(stamp.ModTime()) {
		t.Fatalf("repeated restore rewrote the file: %v", err)
	}
	if err := os.WriteFile(filepath.Join(root, "note.txt"), []byte("independent\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err = files.RestoreCheckpoint(ctx, root, content, settled); err == nil {
		t.Fatal("restore overwrote an independently changed revision")
	}
	if kept, err := os.ReadFile(filepath.Join(root, "note.txt")); err != nil || string(kept) != "independent\n" {
		t.Fatalf("independent content: %q %v", kept, err)
	}
	// Other changes of the turn to the same file, one captured earlier and one
	// later: a file that holds what it had before the earlier one is restored
	// too. The fixture rows are rolled back.
	earlier, later := checkpointVersion("earlier\n"), checkpointVersion("after\n")
	rolledBack := errors.New("fixture rows")
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		for row, before := range map[int]protocol.ConversationCheckpointVersion{0: earlier, 2: later} {
			other := approval
			other.ID = protocol.NewApprovalID()
			body, err := encode(other)
			if err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,?3,?4,?5,?6,1)", other.ID.String(), turn.String(), fmt.Sprint("other-", row), body, []byte("null"), protocol.NewRequestID().String()); err != nil {
				return err
			}
			if body, err = encode(checkpointRecord{Worktree: session.Worktree, Path: "note.txt", Before: &before, After: checkpointVersion("other\n")}); err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO file_checkpoints(rowid,id,turn,approval,body,before_text,after_text) VALUES(?1,?2,?3,?4,?5,'','other\n')", row, protocol.NewCheckpointID().String(), turn.String(), other.ID.String(), body); err != nil {
				return err
			}
		}
		_, chain, err := PrepareCheckpointRestore(ctx, db, session.ID, content.File.ID, session.Worktree, root)
		if err != nil {
			return err
		}
		if len(chain) != 2 || *chain[0] != earlier || *chain[1] != *content.File.Before {
			return fmt.Errorf("restored versions of a repeated change: %+v", chain)
		}
		return rolledBack
	})
	if !errors.Is(err, rolledBack) {
		t.Fatal(err)
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
