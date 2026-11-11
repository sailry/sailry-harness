package live

import (
	"bytes"
	"context"
	"encoding/hex"
	"errors"
	"image"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestAttachmentLocalIO(t *testing.T) {
	// The first subtests draw with a ui.Tester, whose font atlas every
	// tester shares: they run before this test joins the parallel ones.
	t.Run("remote transfer dialog retains its cancel owner", func(t *testing.T) {
		local := &Runtime{}
		cancelled := 0
		remote := &Runtime{parent: local, attachments: &attachmentState{
			download:       &transfers.Download{Open: true, Name: "remote-result.bin", Status: transfers.Receiving, Size: 10},
			downloadCancel: func() { cancelled++ },
		}}
		local.bindings = map[string]*Runtime{"remote": remote}
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			local.transferDialogs(c)
		}, 640, 480)
		tester.Frame()
		if !tester.HasText("remote-result.bin") || !local.transferModalBusy(true) {
			t.Fatal("remote transfer dialog was not presented by its window")
		}
		tester.Key(0, ui.KeyEscape)
		tester.Frame()
		if cancelled != 1 || !remote.attachments.download.Canceled {
			t.Fatal("remote transfer cancellation was routed to another owner")
		}
	})
	t.Run("workspace uploads", testWorkspaceUploads)
	t.Run("ssh transfers", testSshTransferBridge)
	t.Parallel()
	t.Run("source evidence and limits", func(t *testing.T) {
		root := t.TempDir()
		path := filepath.Join(root, "input.txt")
		data := []byte("attachment content")
		if err := os.WriteFile(path, data, 0o600); err != nil {
			t.Fatal(err)
		}
		source, err := localSource(t.Context(), path, nil, "")
		if err != nil {
			t.Fatal(err)
		}
		got, err := io.ReadAll(source.reader)
		_ = source.reader.Close()
		sum := blake3.Sum256(data)
		if err != nil || !bytes.Equal(got, data) || source.size != uint64(len(data)) || source.revision != hex.EncodeToString(sum[:]) || source.mime != "text/plain" {
			t.Fatalf("source lost content evidence: %+v, %q, %v", source, got, err)
		}
		if _, err = localSource(t.Context(), "relative.txt", nil, ""); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("relative source accepted: %v", err)
		}
		large, err := os.Create(filepath.Join(root, "large.bin"))
		if err != nil {
			t.Fatal(err)
		}
		if err = large.Truncate(attachmentMaximum + 1); err != nil {
			t.Fatal(err)
		}
		_ = large.Close()
		_, err = localSource(t.Context(), large.Name(), nil, "")
		var sourceError *attachmentSourceError
		if !errors.As(err, &sourceError) || sourceError.key != "files_upload_too_large" {
			t.Fatalf("oversize source did not retain its UI outcome: %v", err)
		}
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		if _, err = localSource(ctx, "", data, "image/png"); !errors.Is(err, context.Canceled) {
			t.Fatalf("cancelled source accepted: %v", err)
		}
	})
	for _, mode := range []string{"new", "replace", "changed", "appeared", "cancelled"} {
		t.Run("destination "+mode, func(t *testing.T) {
			root := t.TempDir()
			path := filepath.Join(root, "saved.txt")
			if mode == "replace" || mode == "changed" || mode == "cancelled" {
				if err := os.WriteFile(path, []byte("original"), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			target, err := prepareDownloadDestination(path)
			if err != nil {
				t.Fatal(err)
			}
			defer target.close()
			if _, err = target.file.WriteString("verified bytes"); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			want := "verified bytes"
			if mode == "changed" || mode == "appeared" {
				want = "external content"
				if err = os.WriteFile(path, []byte(want), 0o600); err != nil {
					t.Fatal(err)
				}
			} else if mode == "cancelled" {
				want = "original"
				cancel()
			}
			err = target.publish(ctx)
			if mode == "changed" || mode == "appeared" {
				if faultCode(err) != protocol.ErrorCodeRevisionConflict {
					t.Fatalf("destination conflict was lost: %v", err)
				}
			} else if mode == "cancelled" {
				if !errors.Is(err, context.Canceled) {
					t.Fatalf("cancelled destination was published: %v", err)
				}
			} else if err != nil || !target.published {
				t.Fatalf("verified publication failed: %v", err)
			}
			target.close()
			got, err := os.ReadFile(path)
			if err != nil || string(got) != want {
				t.Fatalf("destination changed unexpectedly: %q, %v", got, err)
			}
			if _, err = os.Lstat(filepath.Join(root, target.temporary)); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("owned staging remained: %v", err)
			}
		})
	}
}

func TestAttachmentSourceProjection(t *testing.T) {
	t.Run("notebook projection", testEditorNotebookProjection)
	t.Run("retained remote artifact renders through its captured owner", func(t *testing.T) {
		file := conversation.File{ID: "preview/remote/session/artifact"}
		artifact := protocol.ArtifactSummary{ID: protocol.NewArtifactID(), Revision: 3}
		local := &Runtime{attachments: &attachmentState{previews: map[string]attachmentDescriptor{}}}
		rendered := 0
		remote := &Runtime{parent: local, attachments: &attachmentState{
			previews:    map[string]attachmentDescriptor{file.ID: {artifact: &artifact}},
			previewHTML: map[string]string{file.ID: "<p>Remote artifact</p>"},
			native: NativeFileActions{DocumentView: func(_ *ui.Context, source protocol.ArtifactSummary, html string) bool {
				if source != artifact || html != "<p>Remote artifact</p>" {
					t.Error("artifact rendering lost the captured revision or content")
				}
				rendered++
				return true
			}},
		}}
		local.bindings = map[string]*Runtime{"remote": remote}
		if !local.renderArtifactPreview(nil, file) || rendered != 1 {
			t.Fatal("local navigation replaced the remote artifact owner")
		}
	})
	t.Parallel()
	r := &Runtime{ctx: t.Context()}
	r.initAttachments()
	defer r.closeAttachments()
	session, worktree := protocol.NewSessionID(), protocol.NewWorktreeID()
	attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Worktree: worktree, Name: "chart.png", MediaType: "image/png", Revision: "captured"}}
	imageSource := protocol.ConversationImage{Entry: "canonical-entry", Part: 3, Index: 2, Attachment: attachment}
	page := protocol.DefaultConversationPage()
	page.Entries = []protocol.ConversationEntry{{Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: &protocol.ConversationPartToolResultPayload{Images: []protocol.ConversationImage{imageSource}}}}}}
	page.Queue.Items = []protocol.ConversationPending{{Attachments: []protocol.Attachment{attachment}}}
	r.bindAttachmentSources(session.String(), page)
	descriptor, ok := r.attachmentDescriptor(session.String(), attachment.ID.String())
	if !ok || descriptor.image == nil || *descriptor.image != imageSource || descriptor.worktree != worktree || descriptor.session != session {
		t.Fatalf("canonical image provenance lost to queue metadata: %+v", descriptor)
	}
	revision := "observed"
	r.registerWorkspaceFile(session.String(), "file-card", worktree, "docs/readme.md", &revision)
	revision = "mutated"
	descriptor, ok = r.attachmentDescriptor(session.String(), "file-card")
	if !ok || descriptor.revision == nil || *descriptor.revision != "observed" {
		t.Fatalf("observed revision was not captured: %+v", descriptor)
	}
	turn := conversation.Turn{Files: []conversation.File{{ID: "image-input", Path: "capture", Mime: "image/png"}, {ID: "text-input", Path: "notes.txt", Mime: "text/plain"}}}
	sentAttachmentImages(&turn)
	if len(turn.Images) != 1 || turn.Images[0].ID != "image-input" || len(turn.Files) != 1 || turn.Files[0].ID != "text-input" {
		t.Fatalf("sent MIME image did not retain thumbnail ownership: %+v", turn)
	}
	observed := "published"
	ref := protocol.ToolDetailReference{Session: session, Turn: protocol.NewTurnID(), Entry: "captured-response", Index: 2}
	artifact := protocol.ArtifactSummary{ID: protocol.NewArtifactID(), Session: protocol.NewSessionID(), Turn: ref.Turn, Title: "Report", Revision: 3}
	published := projectPublishedItems(ref, worktree, protocol.ToolDetail{Content: &protocol.ToolContent{Blocks: []protocol.ToolContentBlock{
		{Kind: "file", Data: &protocol.ToolContentFile{Path: "output/chart.png", Mime: "image/png", Revision: &observed}},
		{Kind: "artifact", Data: &protocol.ToolContentBlockArtifactPayload{Artifact: artifact}},
	}}})
	observed = "changed"
	if len(published) != 2 || !published[0].image || published[0].descriptor.worktree != worktree || *published[0].descriptor.revision != "published" || published[1].descriptor.session != session || *published[1].descriptor.artifact != artifact {
		t.Fatalf("published cards lost captured worktree, revision or fork scope: %+v", published)
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, image.NewNRGBA(image.Rect(0, 0, 3, 2))); err != nil {
		t.Fatal(err)
	}
	bitmap, err := decodeAttachmentBitmap(encoded.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	if width, height := bitmap.Size(); width != 3 || height != 2 {
		t.Fatalf("decoded image size changed: %dx%d", width, height)
	}
}
