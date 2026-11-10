package live

import (
	"context"
	"errors"
	"math"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func sameWorkspaceUploadSpec(left, right protocol.FileUploadSpec) bool {
	return left.Worktree == right.Worktree && left.Path == right.Path && left.Size == right.Size && left.Revision == right.Revision && (left.ExpectedRevision == nil && right.ExpectedRevision == nil || left.ExpectedRevision != nil && right.ExpectedRevision != nil && *left.ExpectedRevision == *right.ExpectedRevision)
}

func (u *workspaceUpload) start(replace bool) {
	if u.view.Status == transfers.EntryRunning || u.publication != nil || u.view.Status == transfers.EntryDone || u.view.Status == transfers.EntryExists && !replace || replace && (u.view.Status != transfers.EntryExists || u.existing == nil) {
		return
	}
	r := u.project.runtime
	if !u.targetAvailable() {
		r.enqueue(func(c *ui.Context) {
			u.report(c, fileActionFault(protocol.ErrorCodeBusy, "file has an active operation or unsaved changes"))
		})
		return
	}
	ctx, cancel := context.WithCancel(u.owner.ctx)
	u.cancel, u.stop = cancel, false
	u.view.Status, u.view.Cancelable, u.view.Canceling, u.view.Phase = transfers.EntryRunning, true, false, "files_upload_preparing"
	u.view.Copied, u.view.Size = 0, 0
	sourcePath, destination := u.paths[u.index], u.view.Destination
	var expected *string
	if replace {
		revision := *u.existing
		expected = &revision
	}
	r.async(func() {
		source, err := localSource(ctx, sourcePath, nil, "", math.MaxInt64)
		if err != nil {
			u.finishPreparation(err, nil)
			return
		}
		defer source.reader.Close()
		spec := protocol.FileUploadSpec{Worktree: u.worktree, Path: destination, Size: source.size, Revision: source.revision, ExpectedRevision: expected}
		output, err := r.controller.Do(ctx, protocol.Command{Kind: "upload_file", Data: spec})
		if err != nil {
			var existing *string
			if faultCode(err) == protocol.ErrorCodeRevisionConflict {
				existing = u.destinationRevision(ctx, destination)
			}
			u.finishPreparation(err, existing)
			return
		}
		upload, ok := detail[protocol.FileUpload](output.Data)
		if output.Kind != "file_upload" || !ok || !sameWorkspaceUploadSpec(upload.Spec, spec) {
			if ok {
				r.cancelAttachmentStream(upload.Stream)
			}
			u.finishPreparation(fileActionFault(protocol.ErrorCodeInvalidRequest, "file upload descriptor changed"), nil)
			return
		}
		r.enqueue(func(*ui.Context) { u.view.Phase, u.view.Size = "files_upload_sending", spec.Size })
		last := time.Time{}
		err = r.controller.Upload(ctx, upload.Stream, spec.Size, spec.Revision, source.reader, func(copied uint64) {
			now := time.Now()
			if copied != 0 && copied != spec.Size && !last.IsZero() && now.Sub(last) < 75*time.Millisecond {
				return
			}
			last = now
			r.enqueue(func(*ui.Context) { u.view.Copied = copied })
		})
		_ = source.reader.Close()
		if err != nil {
			u.finishPreparation(err, nil)
			return
		}
		if err = ctx.Err(); err != nil {
			r.cancelAttachmentStream(upload.Stream)
			u.finishPreparation(err, nil)
			return
		}
		request := r.controller.Prepare(protocol.Command{Kind: "finish_file_upload", Data: protocol.CommandFinishFileUploadPayload{Worktree: spec.Worktree, Path: spec.Path, Stream: upload.Stream}})
		r.enqueue(func(c *ui.Context) {
			if ctx.Err() != nil || !u.targetAvailable() {
				r.async(func() { r.cancelAttachmentStream(upload.Stream) })
				err := ctx.Err()
				if err == nil {
					err = fileActionFault(protocol.ErrorCodeConflict, "upload destination has unsaved changes")
				}
				u.preparationResult(c, err, nil)
				return
			}
			u.publication, u.spec = &request, spec
			u.publish(false)
		})
	})
}

func (u *workspaceUpload) destinationRevision(ctx context.Context, destination string) *string {
	r := u.project.runtime
	output, err := r.controller.Do(ctx, protocol.Command{Kind: "download_file", Data: protocol.CommandDownloadFilePayload{Worktree: u.worktree, Path: destination}})
	if err != nil {
		return nil
	}
	download, ok := detail[protocol.FileDownload](output.Data)
	if !ok {
		return nil
	}
	r.cancelAttachmentStream(download.Stream)
	if output.Kind != "file_download" || download.Worktree != u.worktree || download.Path != destination {
		return nil
	}
	revision := download.Revision
	return &revision
}

func (u *workspaceUpload) finishPreparation(err error, existing *string) {
	u.project.runtime.enqueue(func(c *ui.Context) { u.preparationResult(c, err, existing) })
}

func (u *workspaceUpload) preparationResult(c *ui.Context, err error, existing *string) {
	if u.cancel != nil {
		u.cancel()
		u.cancel = nil
	}
	u.existing = existing
	u.view.Canceling, u.view.Cancelable, u.view.Status = false, false, transfers.EntryFailed
	if existing != nil {
		u.view.Status = transfers.EntryExists
	}
	if u.stop || errors.Is(err, context.Canceled) || faultCode(err) == protocol.ErrorCodeCancelled {
		u.view.Status = transfers.EntryCancelled
	}
	u.report(c, err)
}

func (u *workspaceUpload) check() {
	if u.view.Status == transfers.EntryUncertain && u.publication != nil {
		u.publish(true)
	}
}

func (u *workspaceUpload) publish(check bool) {
	if u.publication == nil {
		return
	}
	r := u.project.runtime
	request := *u.publication
	u.view.Status, u.view.Cancelable, u.view.Canceling, u.view.Phase = transfers.EntryRunning, false, false, "files_upload_publishing"
	r.async(func() {
		var output protocol.Output
		var err error
		if check {
			output, err = r.retryAttachmentRequest(u.owner.ctx, request)
		} else {
			output, err = r.controller.Execute(u.owner.ctx, request)
		}
		r.enqueue(func(c *ui.Context) { u.publicationResult(c, output, err) })
	})
}

func (u *workspaceUpload) publicationResult(c *ui.Context, output protocol.Output, err error) {
	if u.cancel != nil {
		u.cancel()
		u.cancel = nil
	}
	u.view.Canceling, u.view.Cancelable = false, false
	if err == nil {
		written, ok := detail[protocol.FileWritten](output.Data)
		if output.Kind != "file_written" || !ok || written.Path != u.spec.Path || written.Size != u.spec.Size || written.Revision != u.spec.Revision || written.Excerpt != nil {
			err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "file publication result changed")
		}
	}
	if err != nil {
		u.view.Status = transfers.EntryFailed
		if attachmentRequestUncertain(err) || faultCode(err) == protocol.ErrorCodeCancelled {
			u.view.Status = transfers.EntryUncertain
		} else {
			u.publication = nil
		}
		u.report(c, err)
		return
	}
	u.publication = nil
	u.project.pendingFiles, u.project.pendingGit = true, true
	u.index++
	if u.index == len(u.paths) {
		u.view.Status, u.view.Remaining = transfers.EntryDone, nil
		delete(u.owner.items, u.id)
		kit.Toast(c, kit.Done, i18n.T(c, "files_upload_done"))
		return
	}
	open := u.view.Open
	u.prepare(open)
	if u.stop {
		u.view.Status = transfers.EntryCancelled
		delete(u.owner.items, u.id)
		kit.Toast(c, kit.Done, i18n.T(c, "files_upload_cancelled"))
		return
	}
	u.start(false)
}
