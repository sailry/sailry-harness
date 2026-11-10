package live

import (
	"context"
	"errors"
	"path"
	"path/filepath"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceUploads struct {
	ctx     context.Context
	cancel  context.CancelFunc
	items   map[protocol.RequestID]*workspaceUpload
	current *workspaceUpload
	picking bool
}

type workspaceUpload struct {
	owner       *workspaceUploads
	project     *workspaceProject
	id          protocol.RequestID
	worktree    protocol.WorktreeID
	directory   string
	paths       []string
	index       int
	view        transfers.EntryTransfer
	cancel      context.CancelFunc
	stop        bool
	existing    *string
	publication *protocol.Request
	spec        protocol.FileUploadSpec
}

func (r *Runtime) workspaceUploadOpen() bool {
	return r.workspace != nil && r.workspace.uploads != nil && r.workspace.uploads.current != nil && r.workspace.uploads.current.view.Open
}

func (r *Runtime) closeWorkspaceUploads() {
	if r.workspace != nil && r.workspace.uploads != nil {
		r.workspace.uploads.cancel()
	}
}

// UploadWorkspaceFiles captures the selected execution worktree before opening
// the controller's native picker. Selected local paths are never Node paths.
func (r *Runtime) UploadWorkspaceFiles(worktree protocol.WorktreeID, directory string) {
	if r.workspace == nil || r.attachments == nil || r.attachments.native.PickFiles == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "native file picker is unavailable"))
		return
	}
	project := r.workspace.projects[worktree]
	if project == nil {
		r.Report(fileActionFault(protocol.ErrorCodeNotFound, "upload worktree is unavailable"))
		return
	}
	if r.transferDialogElsewhere() || r.workspaceTransferOpen() || r.workspaceUploadOpen() || r.attachments.download != nil && r.attachments.download.Open {
		r.Report(fileActionFault(protocol.ErrorCodeBusy, "another file transfer dialog is open"))
		return
	}
	if r.workspace.uploads == nil {
		ctx, cancel := context.WithCancel(r.ctx)
		r.workspace.uploads = &workspaceUploads{ctx: ctx, cancel: cancel, items: map[protocol.RequestID]*workspaceUpload{}}
	}
	state := r.workspace.uploads
	if state.picking {
		return
	}
	state.picking = true
	r.attachments.native.PickFiles(func(paths []string) {
		selected := slices.Clone(paths)
		r.enqueue(func(c *ui.Context) {
			state.picking = false
			if len(selected) == 0 {
				return
			}
			if r.transferDialogElsewhere() || r.workspaceTransferOpen() || r.workspaceUploadOpen() || r.attachments.download != nil && r.attachments.download.Open {
				project.workspaceError(c, protocol.ErrorCodeBusy, "another file transfer dialog is open")
				return
			}
			upload := &workspaceUpload{owner: state, project: project, id: protocol.NewRequestID(), worktree: worktree, directory: directory, paths: selected}
			state.items[upload.id], state.current = upload, upload
			upload.prepare(true)
			upload.start(false)
		})
	})
}

func (r *Runtime) workspaceUploadDialog(c *ui.Context) {
	if !r.workspaceUploadOpen() {
		return
	}
	upload := r.workspace.uploads.current
	transfers.EntryTransferDialog(c, &upload.view, transfers.EntryTransferActions{
		Start: func(string) { upload.start(false) }, Replace: func() { upload.start(true) },
		Check: upload.check, Cancel: upload.close, Close: upload.close,
	})
}

func (u *workspaceUpload) prepare(open bool) {
	name := filepath.Base(u.paths[u.index])
	remaining := make([]string, 0, len(u.paths)-u.index-1)
	for _, source := range u.paths[u.index+1:] {
		remaining = append(remaining, filepath.Base(source))
	}
	u.view = transfers.EntryTransfer{Open: open, Upload: true, SourcePath: name, Destination: path.Join(u.directory, name), Remaining: remaining, Status: transfers.EntryReady}
	u.existing, u.publication = nil, nil
	u.spec = protocol.FileUploadSpec{}
}

func workspaceUploadPathsOverlap(left, right string) bool {
	return left == right || strings.HasPrefix(left, right+"/") || strings.HasPrefix(right, left+"/")
}

func (u *workspaceUpload) targetAvailable() bool {
	if _, dirty := u.project.panel.FileDraft(u.view.Destination); dirty {
		return false
	}
	for _, candidate := range u.owner.items {
		if candidate != u && candidate.worktree == u.worktree && (candidate.view.Status == transfers.EntryRunning || candidate.publication != nil) && workspaceUploadPathsOverlap(candidate.view.Destination, u.view.Destination) {
			return false
		}
	}
	for _, command := range u.project.uncertainOperations {
		if target := workspaceOperationPath(command); target != "" && workspaceUploadPathsOverlap(target, u.view.Destination) {
			return false
		}
		if command.Kind == "rename_entry" {
			if rename, ok := detail[protocol.CommandRenameEntryPayload](command.Data); ok && workspaceUploadPathsOverlap(rename.To, u.view.Destination) {
				return false
			}
		}
	}
	for _, transfer := range u.project.runtime.workspace.transfers {
		if transfer.view.Status != transfers.EntryRunning && transfer.view.Status != transfers.EntryUncertain {
			continue
		}
		source, from, _, move := workspaceTransferEntry(transfer.commands[transfer.index])
		if transfer.project.owner.ID == u.worktree && workspaceUploadPathsOverlap(transfer.view.Destination, u.view.Destination) || move && source == u.worktree && workspaceUploadPathsOverlap(from, u.view.Destination) {
			return false
		}
	}
	return true
}

func (p *workspaceProject) pendingWorkspaceUploads() []panel.PendingOperation {
	if p.runtime.workspace == nil || p.runtime.workspace.uploads == nil {
		return nil
	}
	var result []panel.PendingOperation
	for _, upload := range p.runtime.workspace.uploads.items {
		if upload.worktree != p.owner.ID || upload.view.Status == transfers.EntryDone || upload.view.Status == transfers.EntryCancelled {
			continue
		}
		result = append(result, panel.PendingOperation{Label: upload.view.Destination, Run: func(c *ui.Context) {
			r := p.runtime
			if r.transferDialogElsewhere() || r.workspaceTransferOpen() || r.attachments != nil && r.attachments.download != nil && r.attachments.download.Open || r.workspaceUploadOpen() && r.workspace.uploads.current != upload {
				p.workspaceError(c, protocol.ErrorCodeBusy, "another file transfer dialog is open")
				return
			}
			upload.owner.current, upload.view.Open = upload, true
		}})
	}
	return result
}

func (u *workspaceUpload) close() {
	u.stop, u.view.Open = true, false
	if u.view.Status == transfers.EntryRunning {
		if u.view.Cancelable && u.cancel != nil {
			u.view.Canceling = true
			u.cancel()
		}
		return
	}
	if u.view.Status == transfers.EntryReady {
		u.view.Status = transfers.EntryCancelled
	}
	if u.view.Status == transfers.EntryDone || u.view.Status == transfers.EntryCancelled {
		delete(u.owner.items, u.id)
	}
}

func (u *workspaceUpload) report(c *ui.Context, err error) {
	key, kind := "files_upload_failed", kit.Failed
	var sourceError *attachmentSourceError
	if errors.As(err, &sourceError) {
		key = sourceError.key
	}
	switch {
	case u.view.Status == transfers.EntryExists:
		key, kind = "files_upload_exists", kit.Conflict
	case u.view.Status == transfers.EntryUncertain:
		key, kind = "files_upload_unknown", kit.Uncertain
	case u.view.Status == transfers.EntryCancelled:
		key, kind = "files_upload_cancelled", kit.Done
	default:
		switch faultCode(err) {
		case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeConflict:
			key, kind = "files_upload_conflict", kit.Conflict
		case protocol.ErrorCodePermissionDenied:
			key = "files_upload_denied"
		case protocol.ErrorCodeBusy:
			key = "files_operation_busy"
		}
	}
	kit.Toast(c, kind, i18n.T(c, key))
}
