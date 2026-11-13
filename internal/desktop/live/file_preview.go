package live

import (
	"bytes"
	"context"
	"errors"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// filePreview is the preview dialog of a file outside a conversation, read
// by whoever holds it: a package's remote files today, a host's file
// manager later. The window has one; a newer preview replaces it.
type filePreview struct {
	open       bool
	file       conversation.File
	view       fileview.View
	generation uint64
	cancel     context.CancelFunc
}

// errPreviewLarge is a read refused at fileview.Limit.
var errPreviewLarge = errors.New("file is past the preview limit")

// previewBuffer collects a read of at most fileview.Limit bytes.
type previewBuffer struct{ bytes.Buffer }

func (*previewBuffer) Close() error { return nil }

// openFilePreview shows path in the preview dialog, read by read. A file no
// preview can draw is not read: the dialog names it.
func (r *Runtime) openFilePreview(path string, read func(context.Context) ([]byte, error)) {
	root := r.localRuntime()
	if root.filePreview == nil {
		root.filePreview = &filePreview{}
	}
	preview := root.filePreview
	if preview.cancel != nil {
		preview.cancel()
	}
	preview.generation++
	generation := preview.generation
	file := conversation.File{ID: "file-preview", Path: path, Kind: attachmentFileKind(path, "")}
	preview.open, preview.view = true, fileview.View{}
	if file.Kind == conversation.OtherFile {
		preview.file, preview.cancel = file, nil
		return
	}
	file.State = conversation.PreviewLoading
	preview.file = file
	ctx, cancel := context.WithCancel(root.ctx)
	preview.cancel = cancel
	if !root.async(func() {
		data, err := read(ctx)
		loaded := file
		loaded.State = conversation.PreviewReady
		switch {
		case errors.Is(err, errPreviewLarge):
			loaded.State, err = conversation.PreviewLarge, nil
		case faultCode(err) == protocol.ErrorCodeRevisionConflict:
			loaded.State = conversation.PreviewChanged
		case err != nil:
			loaded.State = conversation.PreviewFailed
		case loaded.Kind == conversation.ImageFile:
			bitmap, decodeErr := decodeAttachmentBitmap(data)
			if decodeErr != nil {
				loaded.State, err = conversation.PreviewFailed, decodeErr
				break
			}
			width, height := bitmap.Size()
			loaded.Image = &conversation.Image{ID: loaded.ID, Name: loaded.Name(), Width: width, Height: height, Source: bitmap}
		case !utf8.Valid(data):
			// Named as text or code and holding something else: there is
			// no text to show.
			loaded.Kind = conversation.OtherFile
		default:
			loaded.Text = string(data)
		}
		root.enqueue(func(c *ui.Context) {
			if preview.generation != generation || !preview.open {
				return
			}
			if err != nil && ctx.Err() == nil {
				root.downloadOutcome(c, err)
			}
			preview.file = loaded
		})
	}) {
		cancel()
		preview.file.State = conversation.PreviewFailed
	}
}

// renderFilePreview draws the preview dialog while it is open; closing it
// stops a read still running.
func (r *Runtime) renderFilePreview(c *ui.Context) {
	preview := r.localRuntime().filePreview
	if preview == nil || !preview.open {
		return
	}
	fileview.Dialog(c, "file-preview", &preview.view, preview.file, &preview.open, fileview.Callbacks{})
	if !preview.open && preview.cancel != nil {
		preview.cancel()
		preview.cancel = nil
	}
}

// readPreview reads a remote file for the preview, refusing one past
// fileview.Limit before its bytes are sent.
func (access sshTransferAccess) readPreview(ctx context.Context, path string) ([]byte, error) {
	profile := access.profile
	output, err := access.execute(ctx, protocol.Command{Kind: "download_ssh_file", Data: protocol.CommandDownloadSshFilePayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Path: path}})
	if err != nil {
		return nil, err
	}
	outcome, ok := detail[protocol.SshOutcome](output.Data)
	if !ok || output.Kind != "ssh_outcome" || outcome.Kind != "download" {
		return nil, sshTransferUnavailable()
	}
	download, ok := detail[protocol.SshDownload](outcome.Data)
	if !ok {
		return nil, sshTransferUnavailable()
	}
	if download.Path != path || download.Size > fileview.Limit {
		access.release(download.Stream)
		if download.Path != path {
			return nil, sshTransferUnavailable()
		}
		return nil, errPreviewLarge
	}
	buffer := &previewBuffer{}
	if err := access.owner.controller.Download(ctx, download.Stream, download.Size, download.Revision, buffer, nil); err != nil {
		return nil, err
	}
	return buffer.Bytes(), nil
}
