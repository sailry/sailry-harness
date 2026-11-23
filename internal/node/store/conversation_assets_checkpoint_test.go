package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func BenchmarkConversationAssetsAppendIncremental(b *testing.B) {
	reads := new(ConversationAssetReads)
	benchmarkConversationAssetsAppend(b, true, func(ctx context.Context, db *sql.Conn, session protocol.SessionID, query protocol.ConversationAssetsQuery, token string) (protocol.ConversationAssetsPage, string, bool, error) {
		query.Checkpoint = &token
		page, err := reads.List(ctx, db, session, query)
		return page, page.Checkpoint, page.Delta, err
	})
}

func TestConversationAssetsCheckpoint(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var reads ConversationAssetReads
	list := func(token string, before *uint64, limit uint16) protocol.ConversationAssetsPage {
		t.Helper()
		var page protocol.ConversationAssetsPage
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			page, err = reads.List(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: limit, Before: before, Checkpoint: &token})
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if page.Checkpoint == "" {
			t.Fatal("missing checkpoint")
		}
		return page
	}
	appendTexts := func(texts ...string) {
		t.Helper()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			for _, text := range texts {
				message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, text))
				record := canonical.NewRecord(turn, "message", &message, now)
				record.Provenance.Author = "assistant"
				if _, err := insertConversationAssetFixture(ctx, db, session.ID, record); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
	write := func(sqlText string, args ...any) {
		t.Helper()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, sqlText, args...)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	empty := list("", nil, 20)
	if empty.Delta || len(empty.Groups) != 0 || empty.NextBefore != nil {
		t.Fatalf("unexpected empty head: %+v", empty)
	}
	if warm := list(empty.Checkpoint, nil, 20); !warm.Delta || len(warm.Groups) != 0 {
		t.Fatalf("empty head could not be refreshed: %+v", warm)
	}
	write("UPDATE worktrees SET path='/source' WHERE id=?1", session.Worktree.String())
	appendTexts("[Report](/source/report.pdf)", "[New](new.pdf)")
	first := list("", nil, 1)
	if first.Delta || len(first.Groups) != 1 || first.NextBefore == nil {
		t.Fatalf("invalid first page: %+v", first)
	}
	second := list(first.Checkpoint, first.NextBefore, 20)
	if second.Delta || len(second.Groups) != 1 || second.NextBefore != nil {
		t.Fatalf("invalid continuation: %+v", second)
	}
	appendTexts("No published file")
	quiet := list(second.Checkpoint, nil, 20)
	if !quiet.Delta || len(quiet.Groups) != 0 || quiet.NextBefore != nil {
		t.Fatalf("empty append replayed the retained history: %+v", quiet)
	}
	appendTexts("[Latest](latest.pdf)")
	latest := list(quiet.Checkpoint, nil, 20)
	if !latest.Delta || len(latest.Groups) != 1 || latest.Groups[0].Sequence <= first.Groups[0].Sequence {
		t.Fatalf("newest publication not isolated: %+v", latest)
	}
	write("UPDATE worktrees SET path='/moved' WHERE id=?1", session.Worktree.String())
	err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		_, err := reads.List(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20, Before: first.NextBefore, Checkpoint: &first.Checkpoint})
		return err
	})
	if !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
		t.Fatalf("joined a continuation to a dirty retained page: %v", err)
	}
	dirty := list(latest.Checkpoint, nil, 20)
	if dirty.Delta || len(dirty.Groups) != 2 {
		t.Fatalf("moved worktree retained an absolute path: %+v", dirty)
	}
	appendTexts("[A](a.pdf)", "[B](b.pdf)")
	burst := list(dirty.Checkpoint, nil, 1)
	if burst.Delta || len(burst.Groups) != 1 || burst.NextBefore == nil {
		t.Fatalf("incomplete delta did not restart as a full page: %+v", burst)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "DELETE FROM conversation_events WHERE session=?1 AND sequence=?2", session.ID.String(), burst.Groups[0].Sequence); err != nil {
			return err
		}
		_, err := db.ExecContext(ctx, "UPDATE sessions SET history_revision=history_revision+1 WHERE id=?1", session.ID.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	replaced := list(burst.Checkpoint, nil, 20)
	if replaced.Delta || replaced.Revision == burst.Revision || len(replaced.Groups) != 3 || replaced.Groups[0].Sequence >= burst.Groups[0].Sequence {
		t.Fatalf("history revision did not invalidate the checkpoint: %+v", replaced)
	}
	for range len(reads.recent) {
		list("", nil, 1)
	}
	evicted := list(replaced.Checkpoint, nil, 20)
	if evicted.Delta || len(evicted.Groups) != 3 {
		t.Fatalf("evicted checkpoint changed inventory: %+v", evicted)
	}
}

func TestConversationAssetsCheckpointMutableDependencies(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var reads ConversationAssetReads
	attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Worktree: session.Worktree, Name: "source.txt", MediaType: "text/plain", Size: 1, Revision: strings.Repeat("a", 64)}}
	var digest string
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := InsertAttachment(ctx, db, node, attachment); err != nil {
			return err
		}
		if err := BindTurnAttachments(ctx, db, turn, []protocol.AttachmentID{attachment.ID}); err != nil {
			return err
		}
		var err error
		digest, err = PutBlob(ctx, db, []byte("detail"))
		if err != nil {
			return err
		}
		payload := canonical.Payload{Value: json.RawMessage(`{"detail":null,"sailry_content":{"version":1,"blocks":[{"kind":"text","path":"/detail"},{"kind":"file","path":"report.pdf","mime":"application/pdf"}]}}`), Assets: []canonical.BinarySlot{{Pointer: "/detail", Encoding: canonical.Base64, Asset: canonical.AssetRef{Digest: digest, Mime: "application/octet-stream", Size: 6}}}}
		response := &canonical.ToolResponse{ID: "report", Name: "fixture_output", Status: canonical.ToolSuccess, Revision: strings.Repeat("a", 64), Result: payload}
		message := canonical.NewMessage(canonical.User, canonical.Block{Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: "sailry-attachment://" + attachment.ID.String(), Mime: &attachment.Spec.MediaType}}}, canonical.Block{Index: 1, Kind: "tool_response", Data: response})
		record := canonical.NewRecord(turn, "message", &message, now)
		record.Provenance.Author = "user"
		_, err = insertConversationAssetFixture(ctx, db, session.ID, record)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	var token string
	var page protocol.ConversationAssetsPage
	read := func() error {
		return storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			page, err = reads.List(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20, Checkpoint: &token})
			if err == nil {
				token = page.Checkpoint
			}
			return err
		})
	}
	if err := read(); err != nil || len(page.Groups) != 1 || len(page.Groups[0].Items) != 2 || token == "" {
		t.Fatalf("cold metadata: %+v, %v", page, err)
	}
	if err := read(); err != nil || !page.Delta || len(page.Groups) != 0 {
		t.Fatalf("warm metadata: %+v, %v", page, err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		attachment.Spec.Name = "edited.txt"
		body, err := encode(attachment)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "UPDATE attachments SET body=?2 WHERE id=?1", attachment.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := read(); err != nil || page.Delta || len(page.Groups) != 1 || page.Groups[0].Items[0].Data.(*protocol.Attachment).Spec.Name != "edited.txt" {
		t.Fatalf("attachment changed: %+v, %v", page, err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "DELETE FROM agent_blobs WHERE digest=?1", digest)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := read(); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
		t.Fatalf("warm checkpoint masked a missing canonical block: %v", err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := PutBlob(ctx, db, []byte("detail")); err != nil {
			return err
		}
		_, err := db.ExecContext(ctx, "DELETE FROM turn_attachments WHERE turn=?1", turn.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := read(); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
		t.Fatalf("warm checkpoint masked an unbound attachment: %v", err)
	}
}

func TestConversationAssetCheckpointDependencyBound(t *testing.T) {
	t.Parallel()
	var reads ConversationAssetReads
	entry := assetCheckpoint{deps: newAssetDependencies()}
	for index := range assetDependencyLimit + 1 {
		entry.deps.blobs[fmt.Sprintf("%064x", index)] = true
	}
	if token := reads.remember(entry); token != "" {
		t.Fatal("retained an oversized checkpoint")
	}
}
