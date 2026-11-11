package panel

import (
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
	kit.Dialog(c.Key("file-close"), &request.open, func() {
		ui.Text(c, i18n.T(c, "files_discard_title")).FontSize(16).FontWeight(600)
		ui.Text(c, request.path).FontSize(14)
		ui.Text(c, i18n.T(c, "files_discard_description")).FontSize(14)
		ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(state.saving).OnClick(func() { request.open = false })
			kit.Secondary(c.Key("discard"), i18n.T(c, "files_discard")).Disabled(state.saving).OnClick(func() {
				item.dirty = false
				pr.finishEditorClose(request)
			})
			kit.PrimaryBusy(c.Key("save"), i18n.T(c, "files_save"), !item.writable || pr.Actions.SaveFile == nil, state.saving).OnClick(func() {
				request.saving = true
				pr.Actions.SaveFile(c, request.path, item.draft+item.suffix)
			})
		})
	}).Width(440)
	if !request.open && !state.saving && pr.closing == request {
		pr.closing = nil
	}
}
