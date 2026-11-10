package live

import "github.com/egoist/mygo/ui"

func (r *Runtime) transferModalBusy(entry bool) bool {
	if r.workspaceUploadOpen() {
		return true
	}
	if entry {
		return r.attachments != nil && r.attachments.download != nil && r.attachments.download.Open
	}
	return r.workspaceTransferOpen()
}
func (r *Runtime) transferDialogs(c *ui.Context) {
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
