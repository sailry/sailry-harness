package live

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"path"
	"path/filepath"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (host *legacyFileHost) selectUpload(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	directory, err := pluginNativeArgument[string](args, 0)
	if err != nil || directory != "" && directory != "." && !legacyFilePath(directory) {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid upload directory")
	}
	if _, err := host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) { return nil, host.check(true) }); err != nil {
		return nil, err
	}
	window := windowViewHost{runtime: host.state.runtime}
	selection, err := window.pickFiles(ctx)
	if err != nil || len(selection) == 0 {
		return json.RawMessage("null"), err
	}
	if len(selection) != 1 {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "select one file to upload")
	}
	name := filepath.Base(selection[0])
	destination := path.Join(directory, name)
	if !legacyFilePath(destination) {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid upload file name")
	}
	return host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(true); err != nil {
			return nil, err
		}
		transfer, err := host.newTransfer("upload", name, destination, selection[0])
		if err != nil {
			return nil, err
		}
		return pluginNativeJSON(transfer.value)
	})
}

func (host *legacyFileHost) startDownload(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	name, err := pluginNativeArgument[string](args, 0)
	if err != nil || !legacyFilePath(name) {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid download source")
	}
	if _, err := host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) { return nil, host.check(false) }); err != nil {
		return nil, err
	}
	window := windowViewHost{runtime: host.state.runtime}
	local, err := window.pickDestination(ctx, path.Base(name))
	if err != nil || local == "" {
		return json.RawMessage("null"), err
	}
	return host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		transfer, err := host.newTransfer("download", name, name, local)
		if err != nil {
			return nil, err
		}
		if err := host.runDownload(transfer); err != nil {
			return nil, err
		}
		return pluginNativeJSON(transfer.value)
	})
}

func (host *legacyFileHost) openSystemFile(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	name, err := pluginNativeArgument[string](args, 0)
	if err != nil || !legacyFilePath(name) {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid file path")
	}
	r := host.state.runtime
	if _, err := host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		if r.attachments == nil || r.attachments.native.OpenLocal == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "native file opening is unavailable")
		}
		return nil, nil
	}); err != nil {
		return nil, err
	}
	directory, release, err := r.ownedDownloadDirectory()
	if err != nil {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "temporary download directory is unavailable")
	}
	keep := false
	defer func() {
		if !keep {
			release()
		}
	}()
	local := filepath.Join(directory, path.Base(name))
	if err := host.downloadTo(ctx, name, local, nil); err != nil {
		return nil, err
	}
	value, err := host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		r.attachments.native.OpenLocal(local)
		return json.RawMessage("null"), nil
	})
	keep = err == nil
	return value, err
}

func (host *legacyFileHost) downloadSource(ctx context.Context, name string) (protocol.FileDownload, error) {
	out, err := host.state.execute(ctx, protocol.Command{Kind: "download_file", Data: protocol.CommandDownloadFilePayload{Worktree: host.worktree, Path: name}})
	if err != nil {
		return protocol.FileDownload{}, err
	}
	value, valid := detail[protocol.FileDownload](out.Data)
	if !valid || out.Kind != "file_download" || value.Worktree != host.worktree || value.Path != name || value.Thumbnail != nil {
		if valid {
			host.state.runtime.cancelAttachmentStream(value.Stream)
		}
		return protocol.FileDownload{}, pluginNativeFault(protocol.ErrorCodeInternal, "unexpected file download response")
	}
	return value, nil
}

func (host *legacyFileHost) downloadTo(ctx context.Context, source, destination string, progress func(uint64, uint64)) error {
	r := host.state.runtime
	window := windowViewHost{runtime: r}
	download, err := host.downloadSource(ctx, source)
	if err != nil {
		return err
	}
	target, err := window.openDestination(destination)
	if err != nil {
		r.cancelAttachmentStream(download.Stream)
		return legacyLocalIOFault(err, "download destination is unavailable")
	}
	defer target.Close()
	if err := window.download(ctx, download.Stream, download.Size, download.Revision, target.Writer, func(copied uint64) {
		if progress != nil {
			progress(copied, download.Size)
		}
	}); err != nil {
		return err
	}
	return legacyLocalIOFault(target.Publish(ctx), "could not publish downloaded file")
}

func (host *legacyFileHost) runDownload(job *legacyTransfer) error {
	r := host.state.runtime
	source, destination := job.value.SourcePath, job.local
	ctx, cancel := context.WithCancel(r.ctx)
	job.cancel = cancel
	job.stage("preparing", nil)
	if !r.async(func() {
		defer cancel()
		err := host.downloadTo(ctx, source, destination, host.transferProgress(job))
		r.enqueue(func(*ui.Context) { host.transferIOResult(job, err, nil) })
	}) {
		cancel()
		host.transferIOResult(job, resourceOfflineFault(), nil)
		return resourceOfflineFault()
	}
	return nil
}

func (host *legacyFileHost) runUpload(job *legacyTransfer) error {
	r := host.state.runtime
	local, destination, expected := job.local, job.value.Path, job.expected
	ctx, cancel := context.WithCancel(r.ctx)
	job.cancel = cancel
	job.stage("preparing", nil)
	if !r.async(func() {
		finish := func(err error, observed *string) {
			cancel()
			r.enqueue(func(*ui.Context) { host.transferIOResult(job, err, observed) })
		}
		window := windowViewHost{runtime: r}
		source, err := window.openSource(ctx, local, math.MaxInt64)
		if err != nil {
			finish(legacyLocalIOFault(err, "upload source is unavailable"), nil)
			return
		}
		defer source.Reader.Close()
		spec := protocol.FileUploadSpec{Worktree: host.worktree, Path: destination, Size: source.Size, Revision: source.Revision, ExpectedRevision: expected}
		out, err := host.state.execute(ctx, protocol.Command{Kind: "upload_file", Data: spec})
		if err != nil {
			var observed *string
			if faultCode(err) == protocol.ErrorCodeRevisionConflict {
				if target, statErr := host.downloadSource(ctx, destination); statErr == nil {
					r.cancelAttachmentStream(target.Stream)
					observed = &target.Revision
				}
			}
			finish(err, observed)
			return
		}
		upload, valid := detail[protocol.FileUpload](out.Data)
		if !valid || out.Kind != "file_upload" || !sameWorkspaceUploadSpec(upload.Spec, spec) {
			if valid {
				r.cancelAttachmentStream(upload.Stream)
			}
			finish(pluginNativeFault(protocol.ErrorCodeInternal, "unexpected file upload response"), nil)
			return
		}
		progress := host.transferProgress(job)
		if err := window.upload(ctx, upload.Stream, spec.Size, spec.Revision, source.Reader, func(copied uint64) { progress(copied, spec.Size) }); err != nil {
			finish(err, nil)
			return
		}
		request := host.request(protocol.Command{Kind: "finish_file_upload", Data: protocol.CommandFinishFileUploadPayload{Worktree: host.worktree, Path: destination, Stream: upload.Stream}})
		r.enqueue(func(*ui.Context) {
			err := ctx.Err()
			if err == nil {
				for _, dirty := range host.project.panel.Unsaved() {
					if legacyPathWithin(dirty, destination) {
						err = pluginNativeFault(protocol.ErrorCodeConflict, "upload destination has unsaved changes")
						break
					}
				}
			}
			cancel()
			job.cancel = nil
			if err != nil {
				r.async(func() { r.cancelAttachmentStream(upload.Stream) })
				host.transferIOResult(job, err, nil)
				return
			}
			_ = host.publishTransfer(job, request)
		})
	}) {
		cancel()
		host.transferIOResult(job, resourceOfflineFault(), nil)
		return resourceOfflineFault()
	}
	return nil
}

func (host *legacyFileHost) transferIOResult(job *legacyTransfer, err error, observed *string) {
	job.cancel, job.expected = nil, observed
	switch {
	case err == nil:
		job.stage("done", nil)
	case errors.Is(err, context.Canceled) || faultCode(err) == protocol.ErrorCodeCancelled:
		job.stage("cancelled", nil)
	case observed != nil:
		job.stage("exists", err)
	default:
		job.stage("failed", err)
	}
	if observed == nil {
		job.local = ""
	}
	host.transferChanged()
}

func (host *legacyFileHost) transferProgress(job *legacyTransfer) func(uint64, uint64) {
	var last time.Time
	return func(copied, size uint64) {
		now := time.Now()
		if copied != 0 && copied != size && now.Sub(last) < 75*time.Millisecond {
			return
		}
		last = now
		host.state.runtime.enqueue(func(*ui.Context) {
			if job.cancel == nil {
				return
			}
			canCancel := job.value.CanCancel
			job.stage("transferring", nil)
			job.value.CanCancel = canCancel
			job.value.Progress = &legacyTransferProgress{Copied: copied, Size: size}
			host.transferChanged()
		})
	}
}

func legacyLocalIOFault(err error, message string) error {
	if err == nil || errors.Is(err, context.Canceled) {
		return err
	}
	code := faultCode(err)
	if code == "" {
		code = protocol.ErrorCodeUnavailable
	}
	return pluginNativeFault(code, message)
}
