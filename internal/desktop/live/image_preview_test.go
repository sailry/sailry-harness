package live

import (
	"bytes"
	"context"
	"encoding/hex"
	"image"
	"image/png"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestOverviewImagesUseViewer(t *testing.T) {
	for _, test := range []struct {
		kind   string
		remote bool
	}{{kind: "attachment"}, {kind: "image"}, {kind: "file"}, {kind: "attachment", remote: true}} {
		name := test.kind
		if test.remote {
			name += " on remote owner"
		}
		t.Run(name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source, settle := readRecoveryFixture(t)
				owner := r
				if test.remote {
					ctx, cancel := context.WithCancel(r.ctx)
					owner = &Runtime{ctx: ctx, parent: r, App: r.App, controller: r.controller}
					owner.initAttachments()
					r.bindings = map[string]*Runtime{"remote": owner}
					t.Cleanup(func() {
						cancel()
						owner.closeAttachments()
						owner.workers.Wait()
					})
				}
				var encoded bytes.Buffer
				if err := png.Encode(&encoded, image.NewNRGBA(image.Rect(0, 0, 20, 30))); err != nil {
					t.Fatal(err)
				}
				source.image = encoded.Bytes()
				sum := blake3.Sum256(source.image)
				digest := hex.EncodeToString(sum[:])
				session, worktree := protocol.SessionID{1}, protocol.WorktreeID{1}
				attachment := protocol.Attachment{ID: protocol.AttachmentID{1}, Spec: protocol.AttachmentSpec{
					Worktree: worktree, Name: "picture.png", MediaType: "image/png", Size: uint64(encoded.Len()), Revision: digest,
				}}
				target := protocol.ConversationAssetsTarget{Kind: test.kind}
				switch test.kind {
				case "attachment":
					target.Data = attachment
				case "image":
					target.Data = protocol.ConversationImage{Entry: "image-entry", Attachment: attachment}
				case "file":
					target.Data = protocol.ConversationAssetsTargetFilePayload{Path: "picture.png"}
				}
				source.handle = func(request protocol.Request) (link.Response, bool) {
					switch request.Command.Kind {
					case "download_image", "download_attachment":
						return link.Response{Output: protocol.Output{Kind: "attachment_download", Data: protocol.AttachmentDownload{Stream: protocol.StreamID{1}, Attachment: attachment}}}, true
					case "download_file":
						return link.Response{Output: protocol.Output{Kind: "file_download", Data: protocol.FileDownload{Stream: protocol.StreamID{1}, Worktree: worktree, Path: "picture.png", Size: uint64(encoded.Len()), Revision: digest}}}, true
					}
					return link.Response{}, false
				}
				copied := false
				owner.attachments.native.CopyImage = func(data []byte, _ string) {
					copied = bytes.Equal(data, source.image)
				}
				rows := overviewAssets([]protocol.ConversationAssetsGroup{{Sequence: 1, Worktree: worktree, Kind: protocol.ConversationAssetsKindResource, Items: []protocol.ConversationAssetsTarget{target}}})
				if len(rows) != 1 {
					t.Fatalf("image inventory produced %d rows", len(rows))
				}
				before := r.App.Panel.Active
				owner.openOverviewAsset(protocol.Session{ID: session, Worktree: worktree}, rows[0])
				preview := r.filePreview
				if preview == nil || !preview.open || preview.file.Kind != conversation.ImageFile || r.App.PanelOpen || r.App.Panel.Active != before {
					t.Fatal("opening a source image changed the right panel instead of opening its viewer")
				}
				settle()
				if preview.file.State != conversation.PreviewReady || preview.file.Image == nil || preview.file.Image.Width != 20 || preview.file.Image.Height != 30 {
					t.Fatalf("image viewer did not receive the decoded picture: %+v", preview.file)
				}
				preview.callbacks.Image(kit.ImageCopy, *preview.file.Image)
				settle()
				if !copied {
					t.Fatal("image toolbar did not use the captured source descriptor")
				}
				preview.open = false
				r.renderFilePreview(nil)
				if r.filePreview != nil || preview.file.Image != nil || len(owner.attachments.previews) != 0 {
					t.Fatalf("closed image viewer retained window=%v, image=%v, descriptors=%d", r.filePreview != nil, preview.file.Image != nil, len(owner.attachments.previews))
				}
			})
		})
	}
}

func TestEscapeReleasesFilePreview(t *testing.T) {
	r := &Runtime{ctx: t.Context()}
	preview, ctx := r.beginFilePreview(conversation.File{ID: "preview", Path: "notes.txt", Text: "sample", Kind: conversation.TextFile, State: conversation.PreviewReady}, fileview.Callbacks{})
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		r.renderFilePreview(c)
	}, 800, 600)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	tester.Frame()
	if r.filePreview != nil || preview.file.Text != "" || ctx.Err() != context.Canceled {
		t.Fatalf("closed preview retained window=%v, text=%d, context=%v", r.filePreview != nil, len(preview.file.Text), ctx.Err())
	}
}

func TestFilePreviewReplacesPendingRead(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, settle := readRecoveryFixture(t)
		var firstContext context.Context
		r.openFilePreview("first.png", func(ctx context.Context, _ func(uint64, uint64)) ([]byte, error) {
			firstContext = ctx
			<-ctx.Done()
			return nil, ctx.Err()
		})
		synctest.Wait()
		first := r.filePreview
		second, _ := r.beginFilePreview(conversation.File{ID: "second", Path: "second.txt"}, fileview.Callbacks{})
		settle()
		if firstContext.Err() != context.Canceled || r.filePreview != second || second.file.Path != "second.txt" || first == second {
			t.Fatal("replacing the image viewer retained its read or accepted its late result")
		}
	})
}
