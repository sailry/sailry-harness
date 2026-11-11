package store

import (
	"context"
	"database/sql"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestRemovedSessionStorage(t *testing.T) {
	t.Parallel()
	// The fixture turn holds a checkpoint of two files, a bound attachment
	// and a published artifact; one more upload was never bound.
	fixture := func(t *testing.T) (*Store, protocol.Session, protocol.TurnID, protocol.AttachmentID) {
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		var unbound protocol.AttachmentID
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			approval := protocol.DefaultConversationApproval()
			approval.ID, approval.Session, approval.Turn = protocol.NewApprovalID(), session.ID, turn
			approval.State, approval.Source = protocol.ConversationApprovalStateApproved, protocol.ConversationApprovalSourceUser
			body, err := encode(approval)
			if err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,'write',?3,?4,?5,1)", approval.ID.String(), turn.String(), body, []byte("null"), protocol.NewRequestID().String()); err != nil {
				return err
			}
			row, err := NextCounter(ctx, db, CounterCheckpoints)
			if err != nil {
				return err
			}
			checkpoint := protocol.NewCheckpointID()
			if _, err = db.ExecContext(ctx, "INSERT INTO file_checkpoints(rowid,id,turn,approval,body,before_text,after_text) VALUES(?1,?2,?3,?4,'{}','before','after')", row, checkpoint.String(), turn.String(), approval.ID.String()); err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO file_checkpoint_entries(id,checkpoint,position,body,before_text,after_text) VALUES(?1,?2,1,'{}','before','after')", protocol.NewCheckpointID().String(), checkpoint.String()); err != nil {
				return err
			}
			for index := range 2 {
				attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Worktree: session.Worktree, Name: "input.txt", MediaType: "text/plain", Size: 5, Revision: fmt.Sprintf("%x", blake3.Sum256([]byte("input")))}}
				if err = InsertAttachment(ctx, db, node, attachment); err != nil {
					return err
				}
				if index == 1 {
					unbound = attachment.ID
				} else if err = BindTurnAttachments(ctx, db, turn, []protocol.AttachmentID{attachment.ID}); err != nil {
					return err
				}
			}
			artifact := protocol.ArtifactSummary{ID: protocol.NewArtifactID(), Kind: protocol.ArtifactKindDocument, Session: session.ID, Turn: turn, Package: protocol.PluginReference{Name: "document-fixture", Digest: strings.Repeat("a", 64)}, Title: "Report", Revision: 1, Size: 16}
			if body, err = encode(artifact); err != nil {
				return err
			}
			if row, err = NextCounter(ctx, db, CounterArtifacts); err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO artifacts(rowid,id,session,body) VALUES(?1,?2,?3,?4)", row, artifact.ID.String(), session.ID.String(), body); err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO artifact_revisions(artifact,revision,body,digest,warnings) VALUES(?1,1,?2,?3,'[]')", artifact.ID.String(), body, strings.Repeat("b", 64)); err != nil {
				return err
			}
			content := protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{{Kind: "artifact", Data: &protocol.ToolContentBlockArtifactPayload{Artifact: artifact}}}}
			if body, err = encode(map[string]any{"sailry_content": content}); err != nil {
				return err
			}
			if body, err = encode(agentJSON(body)); err != nil {
				return err
			}
			message := canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_response", Data: &canonical.ToolResponse{ID: "publish", Name: "fixture_publish", Status: canonical.ToolSuccess, Revision: fmt.Sprintf("%x", blake3.Sum256(body)), Result: canonical.Payload{Value: body}}})
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "assistant"
			if _, _, err = AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
			_, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return storage, session, turn, unbound
	}
	remove := func(t *testing.T, storage *Store, id protocol.SessionID) {
		t.Helper()
		ctx := context.Background()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			current, err := ReadSession(ctx, db, id)
			if err != nil {
				return err
			}
			_, _, err = removeSession(ctx, db, id, current.Revision)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	// held counts the fixture turn's checkpoints, checkpoint entries,
	// bindings, attachments and artifact revisions, then the directories that
	// may go.
	held := func(t *testing.T, storage *Store) string {
		t.Helper()
		var owners []protocol.SessionID
		if err := storage.Node.Read(context.Background(), func(db *sql.Conn) error {
			var err error
			owners, err = RemovedArtifactOwners(context.Background(), db)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		counts := []string{}
		for _, table := range []string{"file_checkpoints", "file_checkpoint_entries", "turn_attachments", "attachments", "artifacts", "artifact_revisions"} {
			counts = append(counts, fmt.Sprint(schemaInteger(t, storage.Node, "SELECT count(*) FROM "+table)))
		}
		return fmt.Sprintf("%s owners=%d", strings.Join(counts, ","), len(owners))
	}

	t.Run("a removed session loses what only it reached", func(t *testing.T) {
		t.Parallel()
		storage, session, _, unbound := fixture(t)
		if got := held(t, storage); got != "1,1,1,2,1,1 owners=0" {
			t.Fatalf("fixture: %s", got)
		}
		remove(t, storage, session.ID)
		// Only the upload that no turn bound stays; its composer owns it.
		if got := held(t, storage); got != "0,0,0,1,0,0 owners=1" {
			t.Fatalf("removed session kept storage: %s", got)
		}
		if schemaInteger(t, storage.Node, fmt.Sprintf("SELECT count(*) FROM attachments WHERE id='%s'", unbound.String())) != 1 {
			t.Fatal("an unbound upload was deleted")
		}
		if schemaInteger(t, storage.Node, "SELECT count(*) FROM agent_events") == 0 {
			t.Fatal("removal deleted canonical history")
		}
	})
	t.Run("a fork keeps what its shared turns hold", func(t *testing.T) {
		t.Parallel()
		storage, session, turn, _ := fixture(t)
		ctx := context.Background()
		var fork protocol.Session
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			fork, _, err = forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: session.ID, Through: turn, ExpectedRevision: session.Revision})
			return err
		}); err != nil {
			t.Fatal(err)
		}
		remove(t, storage, session.ID)
		if got := held(t, storage); got != "1,1,1,2,1,1 owners=0" {
			t.Fatalf("removal took what a fork still shows: %s", got)
		}
		remove(t, storage, fork.ID)
		if got := held(t, storage); got != "0,0,0,1,0,0 owners=2" {
			t.Fatalf("the last fork left storage behind: %s", got)
		}
	})
	t.Run("a rewound turn stays until its session is removed", func(t *testing.T) {
		t.Parallel()
		storage, session, turn, _ := fixture(t)
		ctx := context.Background()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			revision, err := historyRevision(ctx, db, session.ID)
			if err != nil {
				return err
			}
			_, _, err = rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{Session: session.ID, ExpectedHead: turn, ExpectedRevision: revision}, false)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if got := held(t, storage); got != "1,1,1,2,1,1 owners=0" {
			t.Fatalf("rewind deleted storage: %s", got)
		}
		remove(t, storage, session.ID)
		if got := held(t, storage); got != "0,0,0,1,0,0 owners=1" {
			t.Fatalf("removed session kept rewound storage: %s", got)
		}
	})
}
