package panel

import (
	"strings"

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
	buttons := []string{i18n.T(c, "files_keep_editing")}
	actions := []func(*ui.Context){nil}
	var destructive []int
	if request.reload != nil {
		destructive = append(destructive, len(buttons))
		buttons = append(buttons, i18n.T(c, "files_reload"))
		actions = append(actions, request.reload)
	}
	if request.overwrite != nil {
		destructive = append(destructive, len(buttons))
		buttons = append(buttons, i18n.T(c, "files_overwrite"))
		actions = append(actions, request.overwrite)
	}
	answer := kit.Confirm(c.Key("file-conflict").Key(request), &request.open, kit.Confirmation{
		Title:   i18n.T(c, "files_changed_on_disk"),
		Message: strings.Join([]string{request.path, i18n.T(c, "files_conflict_description")}, "\n\n"),
		Buttons: buttons, Default: len(buttons) - 1, Destructive: destructive,
	})
	if answer >= 0 && actions[answer] != nil {
		actions[answer](c)
	}
	if !request.open && pr.conflict == request {
		pr.conflict = nil
	}
}
