package live

import (
	"context"
	"errors"
	"io/fs"
	"math/bits"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const sshUploadMaximum = 1 << 30

type sshTransferEntry struct {
	local, remote string
	directory     bool
	size          uint64
}

// sshTransferPlan is the resumable progress of one upload. pending retains the
// original durable publication request while its outcome is unconfirmed.
type sshTransferPlan struct {
	entries   []sshTransferEntry
	index     int
	pending   *protocol.Request
	copied    uint64
	total     uint64
	overwrite bool
}

func sshTransferKey(message string) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

func sshTransferCancelled() *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "file transfer was cancelled"}
}

func sshTransferUnknown(message string) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: message}
}

func sshTransferFailure(fault *protocol.Fault) sshTransferStatus {
	return sshTransferStatus{stage: sshTransferFailed, fault: fault}
}

// sshTransferFault keeps Node faults and reports any other error as unavailable.
func sshTransferFault(err error) *protocol.Fault {
	var transported *link.FaultError
	if errors.As(err, &transported) {
		fault := transported.Fault
		return &fault
	}
	var pointer *protocol.Fault
	if errors.As(err, &pointer) {
		fault := *pointer
		return &fault
	}
	if errors.Is(err, context.Canceled) {
		return sshTransferCancelled()
	}
	return sshTransferKey(err.Error())
}

func sshTransferLocalName(name string) bool {
	return name != "" && name != "." && name != ".." && !strings.ContainsAny(name, "/\\\x00")
}

func sshTransferPublished(output protocol.Output) bool {
	outcome, ok := detail[protocol.SshOutcome](output.Data)
	return ok && output.Kind == "ssh_outcome" && (outcome.Kind == "transferred" || outcome.Kind == "files_changed")
}

func sshTransferDirectory(output protocol.Output) (protocol.SshDirectory, bool) {
	outcome, ok := detail[protocol.SshOutcome](output.Data)
	if !ok || output.Kind != "ssh_outcome" || outcome.Kind != "directory" {
		return protocol.SshDirectory{}, false
	}
	return detail[protocol.SshDirectory](outcome.Data)
}

func (access sshTransferAccess) release(stream protocol.StreamID) {
	access.owner.cancelAttachmentStream(stream)
}

// sshTransferProgress limits progress publications to one per 75 ms.
func sshTransferProgress(update func(sshTransferStatus)) func(copied, size uint64, final bool) {
	last := time.Time{}
	return func(copied, size uint64, final bool) {
		now := time.Now()
		if !final && !last.IsZero() && now.Sub(last) < 75*time.Millisecond {
			return
		}
		last = now
		update(sshTransferStatus{stage: sshTransferTransferring, copied: copied, size: size})
	}
}

func (access sshTransferAccess) upload(ctx context.Context, source, remote string, overwrite bool, plan sshTransferPlan, check bool, update func(sshTransferStatus)) (sshTransferPlan, sshTransferStatus) {
	plan.overwrite = plan.overwrite || overwrite
	fault := access.uploadEntries(ctx, source, remote, &plan, check, update)
	switch {
	case fault == nil:
		return plan, sshTransferStatus{stage: sshTransferDone}
	case plan.pending != nil:
		return plan, sshTransferFailure(sshTransferUnknown(fault.Message))
	case fault.Code == protocol.ErrorCodeConflict:
		return plan, sshTransferStatus{stage: sshTransferExists}
	}
	return plan, sshTransferFailure(fault)
}

func (access sshTransferAccess) uploadEntries(ctx context.Context, source, remote string, plan *sshTransferPlan, check bool, update func(sshTransferStatus)) *protocol.Fault {
	controller := access.owner.controller
	if check && plan.pending == nil {
		return sshTransferUnknown("original upload receipt is unavailable")
	}
	if plan.pending != nil {
		if !check {
			return sshTransferUnknown("upload publication is unconfirmed")
		}
		resolution, err := controller.Resolve(ctx, *plan.pending)
		if err != nil {
			return sshTransferFault(err)
		}
		switch resolution.State {
		case client.RequestFailed, client.RequestUnknown:
			// A stored failure, or an outcome the Node can never tell: the
			// request is over either way. The job reports it once and may
			// be started again, which publishes under a new request.
			plan.pending = nil
			fault := *resolution.Fault
			return &fault
		case client.RequestApplied:
			plan.pending = nil
			if !sshTransferPublished(resolution.Output) {
				return sshTransferKey("files_upload_failed")
			}
			plan.copied += plan.entries[plan.index].size
			plan.index++
		case client.RequestAbsent:
			if finish, ok := detail[protocol.CommandFinishSshUploadPayload](plan.pending.Command.Data); ok && plan.pending.Command.Kind == "finish_ssh_upload" {
				access.release(finish.Stream)
			}
			plan.pending = nil
			return sshTransferKey("upload publication was not admitted")
		default:
			return sshTransferUnknown("upload publication is unconfirmed")
		}
	}
	if len(plan.entries) == 0 {
		entries, fault := sshTransferScan(ctx, source, remote)
		if fault != nil {
			return fault
		}
		plan.entries, plan.total = entries, 0
		for _, entry := range entries {
			plan.total += entry.size
		}
	}
	profile := access.profile
	progress := sshTransferProgress(update)
	for plan.index < len(plan.entries) {
		entry := plan.entries[plan.index]
		if ctx.Err() != nil {
			return sshTransferCancelled()
		}
		var request protocol.Request
		if entry.directory {
			request = access.request(protocol.Command{Kind: "modify_ssh_file", Data: protocol.CommandModifySshFilePayload{
				Profile: profile.ID, ExpectedRevision: profile.Revision, Path: entry.remote,
				Action: protocol.SshFileAction{Kind: "create", Data: protocol.SshFileActionCreatePayload{Directory: true}},
			}})
		} else {
			local, err := localSource(ctx, entry.local, nil, "", sshUploadMaximum)
			if err != nil {
				return sshUploadSourceFault(ctx, err)
			}
			spec := protocol.SshUploadSpec{Profile: profile.ID, ExpectedRevision: profile.Revision, Path: entry.remote, Size: local.size, Revision: local.revision, Overwrite: plan.overwrite}
			output, err := access.execute(ctx, protocol.Command{Kind: "stage_ssh_upload", Data: spec})
			if err != nil {
				local.reader.Close()
				return sshTransferFault(err)
			}
			staged, ok := detail[protocol.SshUpload](output.Data)
			if !ok || output.Kind != "ssh_upload" {
				local.reader.Close()
				return sshTransferKey("files_upload_failed")
			}
			if staged.Spec != spec {
				local.reader.Close()
				access.release(staged.Stream)
				return sshTransferKey("files_upload_failed")
			}
			copied, total := plan.copied, plan.total
			err = controller.Upload(ctx, staged.Stream, local.size, local.revision, local.reader, func(sent uint64) {
				progress(copied+sent, total, sent == 0 || sent == local.size)
			})
			local.reader.Close()
			if err != nil {
				if ctx.Err() != nil {
					return sshTransferCancelled()
				}
				return sshTransferFault(err)
			}
			if ctx.Err() != nil {
				access.release(staged.Stream)
				return sshTransferCancelled()
			}
			request = access.request(protocol.Command{Kind: "finish_ssh_upload", Data: protocol.CommandFinishSshUploadPayload{
				Profile: profile.ID, ExpectedRevision: profile.Revision, Path: spec.Path, Stream: staged.Stream,
			}})
		}
		update(sshTransferStatus{stage: sshTransferPublishing})
		plan.pending = &request
		// Publication is durable and not cancellable: its outcome is always observed.
		output, err := controller.Execute(access.owner.ctx, request)
		outcome, _ := detail[protocol.SshOutcome](output.Data)
		switch {
		case err == nil && sshTransferPublished(output):
		case err == nil && output.Kind == "ssh_outcome" && outcome.Kind == "host_key_required":
			plan.pending = nil
			return sshTransferKey("ssh_key_changed")
		case err == nil:
			plan.pending = nil
			return sshTransferKey("files_upload_failed")
		case faultCode(err) == protocol.ErrorCodeConflict && entry.directory && plan.overwrite:
			plan.pending = nil
			listed, err := access.execute(ctx, protocol.Command{Kind: "browse_ssh_directory", Data: protocol.CommandBrowseSshDirectoryPayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Path: entry.remote}})
			if err != nil {
				return sshTransferFault(err)
			}
			if _, ok := sshTransferDirectory(listed); !ok {
				return sshTransferKey("files_upload_failed")
			}
		case faultCode(err) == protocol.ErrorCodeOutcomeUnknown || faultCode(err) == protocol.ErrorCodeUnavailable || faultCode(err) == "":
			return sshTransferUnknown(sshTransferFault(err).Message)
		default:
			plan.pending = nil
			return sshTransferFault(err)
		}
		plan.pending = nil
		plan.copied += entry.size
		plan.index++
	}
	return nil
}

func sshUploadSourceFault(ctx context.Context, err error) *protocol.Fault {
	if ctx.Err() != nil {
		return sshTransferCancelled()
	}
	if faultCode(err) == protocol.ErrorCodeRevisionConflict {
		return sshTransferKey("files_upload_changed")
	}
	var source *attachmentSourceError
	if errors.As(err, &source) {
		return sshTransferKey(source.key)
	}
	return sshTransferKey("files_upload_source")
}

// sshTransferScan lists a chosen file or directory tree with every directory
// before its content. Only ordinary files and directories are transferable.
func sshTransferScan(ctx context.Context, source, remote string) ([]sshTransferEntry, *protocol.Fault) {
	type item struct{ local, remote string }
	pending := []item{{source, remote}}
	var result []sshTransferEntry
	for len(pending) > 0 {
		current := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if ctx.Err() != nil {
			return nil, sshTransferCancelled()
		}
		info, err := os.Lstat(current.local)
		if err != nil {
			return nil, sshTransferKey("files_upload_source")
		}
		entry := sshTransferEntry{local: current.local, remote: current.remote, directory: info.IsDir()}
		switch {
		case info.IsDir():
			children, err := os.ReadDir(current.local)
			if err != nil {
				return nil, sshTransferKey("files_upload_source")
			}
			for _, child := range children {
				if !utf8.ValidString(child.Name()) {
					return nil, sshTransferKey("files_upload_name")
				}
				pending = append(pending, item{filepath.Join(current.local, child.Name()), sshTransferJoin(current.remote, child.Name())})
			}
		case info.Mode().IsRegular():
			entry.size = uint64(info.Size())
		default:
			return nil, sshTransferKey("files_upload_source")
		}
		result = append(result, entry)
	}
	return result, nil
}

func (access sshTransferAccess) download(ctx context.Context, remote, local string, directory bool, update func(sshTransferStatus)) sshTransferStatus {
	if fault := access.downloadEntries(ctx, remote, local, directory, update); fault != nil {
		return sshTransferFailure(fault)
	}
	return sshTransferStatus{stage: sshTransferDone}
}

func (access sshTransferAccess) downloadEntries(ctx context.Context, remote, local string, directory bool, update func(sshTransferStatus)) *protocol.Fault {
	type item struct {
		remote, local string
		directory     bool
	}
	profile := access.profile
	pending := []item{{remote, local, directory}}
	var files []item
	for len(pending) > 0 {
		current := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if ctx.Err() != nil {
			return sshTransferCancelled()
		}
		if !current.directory {
			files = append(files, current)
			continue
		}
		info, err := os.Lstat(current.local)
		switch {
		case err == nil && info.IsDir():
		case errors.Is(err, fs.ErrNotExist):
			if os.Mkdir(current.local, 0o755) != nil {
				return sshTransferKey("files_download_destination")
			}
		default:
			return sshTransferKey("files_download_destination")
		}
		var after *protocol.SshCursor
		for {
			output, err := access.execute(ctx, protocol.Command{Kind: "browse_ssh_directory", Data: protocol.CommandBrowseSshDirectoryPayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Path: current.remote, After: after}})
			if err != nil {
				if ctx.Err() != nil {
					return sshTransferCancelled()
				}
				return sshTransferFault(err)
			}
			list, ok := sshTransferDirectory(output)
			if !ok {
				return sshTransferKey("files_download_failed")
			}
			for _, entry := range list.Entries {
				if !sshTransferLocalName(entry.Name) {
					return sshTransferKey("files_download_failed")
				}
				pending = append(pending, item{sshTransferJoin(current.remote, entry.Name), filepath.Join(current.local, entry.Name), entry.Kind == protocol.EntryKindDirectory})
			}
			if after = list.Next; after == nil {
				break
			}
		}
	}
	count := uint64(max(len(files), 1))
	progress := sshTransferProgress(update)
	for index, file := range files {
		key := access.downloadFile(ctx, file.remote, file.local, func(copied, size uint64) {
			permille := uint64(1000)
			if size != 0 {
				high, low := bits.Mul64(min(copied, size), 1000)
				permille, _ = bits.Div64(high, low, size)
			}
			progress(uint64(index)*1000+permille, count*1000, copied == 0 || copied == size)
		}, update)
		if key != "" {
			return sshTransferKey(key)
		}
	}
	return nil
}

// downloadFile receives one remote file into staging and publishes it
// atomically. It returns the failure's message key, or "" when published.
func (access sshTransferAccess) downloadFile(ctx context.Context, remote, local string, progress func(copied, size uint64), update func(sshTransferStatus)) string {
	cancelled := func(err error) bool {
		return ctx.Err() != nil || errors.Is(err, context.Canceled) || faultCode(err) == protocol.ErrorCodeCancelled
	}
	if ctx.Err() != nil {
		return "files_download_cancelled"
	}
	target, err := prepareDownloadDestination(local)
	if err != nil {
		return "files_download_destination"
	}
	defer target.close()
	profile := access.profile
	output, err := access.execute(ctx, protocol.Command{Kind: "download_ssh_file", Data: protocol.CommandDownloadSshFilePayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Path: remote}})
	if err != nil {
		if cancelled(err) {
			return "files_download_cancelled"
		}
		return "files_download_failed"
	}
	outcome, ok := detail[protocol.SshOutcome](output.Data)
	if !ok || output.Kind != "ssh_outcome" || outcome.Kind != "download" {
		return "files_download_failed"
	}
	download, ok := detail[protocol.SshDownload](outcome.Data)
	if !ok {
		return "files_download_failed"
	}
	if download.Path != remote {
		access.release(download.Stream)
		return "files_download_failed"
	}
	err = access.owner.controller.Download(ctx, download.Stream, download.Size, download.Revision, target.file, func(copied uint64) {
		progress(copied, download.Size)
	})
	if err != nil {
		switch {
		case cancelled(err):
			return "files_download_cancelled"
		case faultCode(err) == protocol.ErrorCodeRevisionConflict:
			return "files_download_changed"
		}
		return "files_download_failed"
	}
	update(sshTransferStatus{stage: sshTransferPublishing})
	if err := target.publish(ctx); err != nil {
		switch {
		case cancelled(err):
			return "files_download_cancelled"
		case faultCode(err) == protocol.ErrorCodeRevisionConflict || faultCode(err) == protocol.ErrorCodeConflict:
			return "files_download_conflict"
		}
		return "files_download_destination"
	}
	return ""
}
