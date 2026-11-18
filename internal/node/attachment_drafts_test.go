package node

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestDraftAttachmentFirstMessage(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{
		MCP: fixtureMCPOptions(), Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) {
			return nil, errors.New("fixture only exercises turn admission")
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	owner, other := protocol.NodeID{1}, protocol.NodeID{2}
	trustedCallerFixture(t, node, owner)
	trustedCallerFixture(t, node, other)
	config := sessionConfigFixture(t, node)
	before := snapshotFixture(t, node)
	const payload = "private draft bytes"
	spec := protocol.AttachmentSpec{Name: "draft.txt", MediaType: "text/plain", Size: uint64(len(payload)), Revision: fmt.Sprintf("%x", blake3.Sum256([]byte(payload)))}
	request := func(kind string, data any) protocol.Request {
		return protocol.NewRequest(node.ID(), protocol.Command{Kind: kind, Data: data})
	}
	require := func(result link.Response, kind string) any {
		t.Helper()
		if result.Fault != nil || result.Output.Kind != kind {
			t.Fatalf("expected %s: %+v", kind, result)
		}
		return result.Output.Data
	}
	reject := func(result link.Response, code protocol.ErrorCode) {
		t.Helper()
		if result.Fault == nil || result.Fault.Code != code {
			t.Fatalf("expected %s: %+v", code, result)
		}
	}
	stage := func() (protocol.Attachment, protocol.Request) {
		t.Helper()
		upload := require(dispatchFixture(t, node, owner, request("upload_attachment", &spec)), "attachment_upload").(protocol.AttachmentUpload)
		if upload.Spec.Worktree != (protocol.WorktreeID{}) {
			t.Fatal("draft reservation acquired a worktree")
		}
		if stream, err := node.Open(ctx, other, upload.Stream); err == nil {
			_ = stream.Close()
			t.Fatal("another device with the same user opened the upload")
		}
		stream, err := node.Open(ctx, owner, upload.Stream)
		if err != nil {
			t.Fatal(err)
		}
		defer stream.Close()
		if _, err = io.WriteString(stream, payload); err != nil {
			t.Fatal(err)
		}
		if err = stream.CloseWrite(); err != nil {
			t.Fatal(err)
		}
		var ready [1]byte
		if _, err = io.ReadFull(stream, ready[:]); err != nil || ready[0] != protocol.FILE_UPLOAD_READY {
			t.Fatalf("upload readiness = %v, %v", ready, err)
		}
		finish := request("finish_attachment_upload", &protocol.CommandFinishAttachmentUploadPayload{Stream: upload.Stream})
		reject(dispatchFixture(t, node, other, finish), protocol.ErrorCodeNotFound)
		attachment := require(dispatchFixture(t, node, owner, finish), "attachment").(protocol.Attachment)
		if attachment.Spec != spec {
			t.Fatalf("staged attachment = %+v, want %+v", attachment.Spec, spec)
		}
		return attachment, finish
	}
	checkCount := func(added int) {
		t.Helper()
		after := snapshotFixture(t, node)
		if len(after.Sessions) != len(before.Sessions)+added || len(after.Worktrees) != len(before.Worktrees)+added {
			t.Fatalf("visible session/worktree counts = %d/%d, want %d/%d", len(after.Sessions), len(after.Worktrees), len(before.Sessions)+added, len(before.Worktrees)+added)
		}
	}
	checkDownload := func(id protocol.AttachmentID) {
		t.Helper()
		download := require(dispatchFixture(t, node, owner, request("download_attachment", &protocol.CommandDownloadAttachmentPayload{Attachment: id})), "attachment_download").(protocol.AttachmentDownload)
		stream, err := node.Open(ctx, owner, download.Stream)
		if err != nil {
			t.Fatal(err)
		}
		defer stream.Close()
		data, err := io.ReadAll(stream)
		if err != nil || string(data) != payload {
			t.Fatalf("downloaded draft = %q, %v", data, err)
		}
	}
	staged, finish := stage()
	checkCount(0)
	replay := require(dispatchFixture(t, node, owner, finish), "attachment").(*protocol.Attachment)
	if *replay != staged {
		t.Fatalf("finish replay = %+v, want %+v", replay, staged)
	}
	download := request("download_attachment", &protocol.CommandDownloadAttachmentPayload{Attachment: staged.ID})
	discard := request("discard_attachment", &protocol.CommandDiscardAttachmentPayload{Attachment: staged.ID})
	reject(dispatchFixture(t, node, other, download), protocol.ErrorCodeNotFound)
	reject(dispatchFixture(t, node, other, discard), protocol.ErrorCodeNotFound)
	plugin := download
	scope := protocol.DefaultPluginContext()
	scope.Package = protocol.PluginReference{Name: "fixture", Digest: "fixture"}
	plugin.Plugin = &scope
	reject(dispatchFixture(t, node, owner, plugin), protocol.ErrorCodePermissionDenied)
	for _, reassign := range []bool{true, false} {
		if err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			if err := store.SetLinkTrust(ctx, db, owner, false); err != nil {
				return err
			}
			if reassign {
				return store.PairLinkPeer(ctx, db, owner, link.DirectionController)
			}
			return store.SetLinkTrust(ctx, db, owner, true)
		}); err != nil {
			t.Fatal(err)
		}
		if reassign {
			reject(dispatchFixture(t, node, owner, download), protocol.ErrorCodeNotFound)
			reject(dispatchFixture(t, node, owner, finish), protocol.ErrorCodePermissionDenied)
		}
	}

	draft := protocol.DefaultConversationStart()
	draft.Config, draft.Execution, draft.UseExplicitConfig = &config, protocol.ConversationExecutionModel, true
	draft.Message = protocol.ConversationInput{Text: "Use the draft", Attachments: []protocol.AttachmentID{staged.ID}}
	reject(dispatchFixture(t, node, other, request("start_session", &draft)), protocol.ErrorCodeNotFound)
	invalid := draft
	invalid.Message.Attachments = []protocol.AttachmentID{staged.ID, protocol.NewAttachmentID()}
	reject(dispatchFixture(t, node, owner, request("start_session", &invalid)), protocol.ErrorCodeNotFound)
	checkCount(0)
	node.collectAttachments(ctx)
	checkDownload(staged.ID)

	start := request("start_session", &draft)
	turn := require(dispatchFixture(t, node, owner, start), "queued_turn").(protocol.QueuedTurn)
	if turn.Config.Provider != config.Provider || turn.Config.Model != config.Model || turn.Roles.Execution != draft.Execution {
		t.Fatalf("start altered the selected model or execution mode: %+v", turn)
	}
	checkCount(1)
	if err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		session, err := store.ReadSession(ctx, db, turn.Session)
		if err != nil {
			return err
		}
		bound, err := store.GetTurnAttachment(ctx, db, turn.ID, staged.ID)
		if err != nil {
			return err
		}
		want := staged
		want.Spec.Worktree = session.Worktree
		if session.Worktree == (protocol.WorktreeID{}) || bound != want {
			return fmt.Errorf("bound attachment = %+v, want %+v", bound, want)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	reject(dispatchFixture(t, node, owner, download), protocol.ErrorCodeNotFound)
	again := require(dispatchFixture(t, node, owner, start), "queued_turn").(*protocol.QueuedTurn)
	if again.ID != turn.ID || again.Session != turn.Session {
		t.Fatalf("start replay created another turn: %+v", again)
	}
	checkCount(1)

	removed, _ := stage()
	discard = request("discard_attachment", &protocol.CommandDiscardAttachmentPayload{Attachment: removed.ID})
	require(dispatchFixture(t, node, owner, discard), "attachment_discarded")
	require(dispatchFixture(t, node, owner, discard), "attachment_discarded")
	assertMissing := func(id protocol.AttachmentID) {
		t.Helper()
		_, err := os.Stat(filepath.Join(node.profile.Ownership.Path, "attachments", id.String()))
		if !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("discarded or expired attachment was retained: %v", err)
		}
	}
	assertMissing(removed.ID)
	expired, _ := stage()
	if err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "UPDATE draft_attachments SET created_ms=?1 WHERE id=?2", time.Now().Add(-admission.Retention-time.Hour).UnixMilli(), expired.ID.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	node.collectAttachments(ctx)
	assertMissing(expired.ID)
	checkCount(1)
}
