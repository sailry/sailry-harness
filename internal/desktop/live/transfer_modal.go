package live

import (
	"slices"

	"github.com/egoist/mygo/ui"
)

func (r *Runtime) transferModalBusy(entry bool) bool {
	if r.transferDialogElsewhere() {
		return true
	}
	if r.workspaceUploadOpen() {
		return true
	}
	if entry {
		return r.attachments != nil && r.attachments.download != nil && r.attachments.download.Open
	}
	return r.workspaceTransferOpen()
}
func (r *Runtime) transferDialogs(c *ui.Context) {
	root := r.localRuntime()
	owner := root.transferDialogOwner
	if owner == nil || !owner.hasTransferDialog() {
		owner = nil
		if root.hasTransferDialog() {
			owner = root
		} else {
			ids := make([]string, 0, len(root.bindings))
			for id := range root.bindings {
				ids = append(ids, id)
			}
			slices.Sort(ids)
			for _, id := range ids {
				if candidate := root.bindings[id]; candidate.hasTransferDialog() {
					owner = candidate
					break
				}
			}
		}
		root.transferDialogOwner = owner
	}
	if owner != nil {
		owner.ownedTransferDialogs(c)
	}
}
func (r *Runtime) hasTransferDialog() bool {
	return r.workspaceUploadOpen() || r.workspaceTransferOpen() || r.attachmentDownloadOpen()
}
func (r *Runtime) transferDialogElsewhere() bool {
	root := r.localRuntime()
	if root != r && root.hasTransferDialog() {
		return true
	}
	for _, bound := range root.bindings {
		if bound != r && bound.hasTransferDialog() {
			return true
		}
	}
	return false
}
func (r *Runtime) ownedTransferDialogs(c *ui.Context) {
	if r.workspaceUploadOpen() {
		r.workspaceUploadDialog(c)
		return
	}
	if r.workspaceTransferOpen() {
		r.workspaceTransferDialog(c)
		return
	}
	r.attachmentDownloadDialog(c)
}
