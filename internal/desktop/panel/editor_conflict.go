package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// editorConflict is the save of a draft whose file changed on disk, waiting
// for the user's choice.
type editorConflict struct {
	path              string
	open              bool
	overwrite, reload func(*ui.Context)
}

// SetFileChanged records whether the file at path changed on disk while its
// draft is open. The editor's footer shows it.
func (pr *Project) SetFileChanged(path string, changed bool) {
	if item := pr.Files.items[path]; item != nil {
		item.documentState().changed = changed
	}
}

// FileConflict asks what to do with the draft of path, whose file changed on
// disk: overwrite the file with the draft, reload the file and drop the
// draft, or keep editing. A nil overwrite is a file that cannot be written.
// A close that was waiting for this save is withdrawn; the draft is not
// settled until the user chooses.
func (pr *Project) FileConflict(path string, overwrite, reload func(*ui.Context)) {
	if pr.closing != nil && pr.closing.path == path {
		pr.closing.open = false
	}
	pr.conflict = &editorConflict{path: path, open: true, overwrite: overwrite, reload: reload}
}

// DropFileDraft returns the editor of path to the text it loaded last.
func (pr *Project) DropFileDraft(path string) {
	if item := pr.Files.items[path]; item != nil {
		item.dirty = false
		pr.SetFile(path, item.base)
	}
}

func (pr *Project) editorConflictDialog(c *ui.Context) {
	request := pr.conflict
	if request == nil {
		return
	}
	choose := func(run func(*ui.Context)) func() {
		return func() {
			request.open = false
			if run != nil {
				run(c)
			}
		}
	}
	kit.Dialog(c.Key("file-conflict"), &request.open, func() {
		ui.Text(c, i18n.T(c, "files_changed_on_disk")).FontSize(16).FontWeight(600)
		ui.Text(c, request.path).FontSize(14)
		ui.Text(c, i18n.T(c, "files_conflict_description")).FontSize(14)
		ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("keep"), i18n.T(c, "files_keep_editing")).OnClick(choose(nil))
			kit.Secondary(c.Key("reload"), i18n.T(c, "files_reload")).OnClick(choose(request.reload))
			kit.Primary(c.Key("overwrite"), i18n.T(c, "files_overwrite"), request.overwrite == nil).OnClick(choose(request.overwrite))
		})
	}).Width(440)
	if !request.open && pr.conflict == request {
		pr.conflict = nil
	}
}
