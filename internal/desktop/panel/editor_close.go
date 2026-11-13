package panel

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

func (pr *Project) editorCloseDialog(c *ui.Context) {
	request := pr.closing
	if request == nil {
		return
	}
	item := pr.Files.items[request.path]
	if item == nil {
		pr.finishEditorClose(request)
		return
	}
	state := item.documentState()
	if request.saving && !state.saving {
		request.saving = false
		if !item.dirty {
			pr.finishEditorClose(request)
			return
		}
	}
	if state.saving {
		return
	}
	buttons := []string{i18n.T(c, "settings_cancel"), i18n.T(c, "files_discard")}
	defaultButton := 0
	if item.writable && pr.Actions.SaveFile != nil {
		buttons = append(buttons, i18n.T(c, "files_save"))
		defaultButton = 2
	}
	open := request.open
	switch kit.Confirm(c.Key("file-close").Key(request), &open, kit.Confirmation{
		Title:   i18n.T(c, "files_discard_title"),
		Message: strings.Join([]string{request.path, i18n.T(c, "files_discard_description")}, "\n\n"),
		Buttons: buttons, Default: defaultButton, Destructive: []int{1},
	}) {
	case 0:
		request.open = false
	case 1:
		item.dirty = false
		pr.finishEditorClose(request)
	case 2:
		request.saving = true
		pr.Actions.SaveFile(c, request.path, item.draft+item.suffix)
	}
	if !request.open && !state.saving && pr.closing == request {
		pr.closing = nil
	}
}
