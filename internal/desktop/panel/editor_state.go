package panel

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type documentEditor struct {
	wrap, saving, partial       bool
	rich                        *richtext.Document
	richSource                  string
	richOriginal, richCanonical string
	richVersion                 uint64
	richSafe                    bool
	pending                     func(*editor.State)
	notebook                    *viewmodel.EditorNotebook
	notebookRequested           bool
	notebookDraft               string
	focus                       [2]ui.Handle
	unavailable                 bool
	// changed is the file changing on disk while its draft is open.
	changed bool
}
type editorCloseRequest struct {
	path      string
	open      bool
	saving    bool
	confirmed func()
}

func (item *fileItem) documentState() *documentEditor {
	if item.document == nil {
		// Long lines wrap until the toolbar's toggle turns it off.
		item.document = &documentEditor{wrap: true}
	}
	return item.document
}
func (state *documentEditor) focusFor(own bool) *ui.Handle {
	if own {
		return &state.focus[1]
	}
	return &state.focus[0]
}
func (pr *Project) SetFileStatus(path string, saving, partial bool) {
	state := pr.Files.item(path).documentState()
	state.saving, state.partial = saving, partial
}
func (pr *Project) SetFileUnavailable(path string, unavailable bool) {
	if item := pr.Files.items[path]; item != nil {
		item.documentState().unavailable = unavailable
		if unavailable {
			item.writable = false
		}
	}
}
func (pr *Project) fileTabLabel(path, label string) string {
	if item := pr.Files.items[path]; item != nil && item.dirty {
		return label + " •"
	}
	return label
}
func (pr *Project) requestEditorClose(path string, confirmed func()) bool {
	item := pr.Files.items[path]
	if pr.Actions == nil || item == nil || !item.dirty && !item.documentState().saving {
		return false
	}
	if pr.closing == nil {
		pr.closing = &editorCloseRequest{path: path, open: true, saving: item.documentState().saving, confirmed: confirmed}
	}
	return true
}
func (pr *Project) finishEditorClose(request *editorCloseRequest) {
	if pr.closing != request {
		return
	}
	pr.closing = nil
	if request.confirmed != nil {
		request.confirmed()
	}
}
func (pr *Project) SetEditorNotebook(path string, notebook *viewmodel.EditorNotebook) {
	item := pr.Files.items[path]
	if item == nil {
		return
	}
	state := item.documentState()
	state.notebook = notebook
	state.notebookRequested = true
}
func (pr *Project) EditorNotebook(path string) *viewmodel.EditorNotebook {
	if item := pr.Files.items[path]; item != nil {
		return item.documentState().notebook
	}
	return nil
}
func (pr *Project) ResetEditorNotebook(path string) {
	if item := pr.Files.items[path]; item != nil {
		state := item.documentState()
		state.notebook, state.notebookRequested, state.notebookDraft = nil, false, ""
	}
}

// CanSaveFocused follows the focused editor's native menu ownership, including
// clean documents whose Save action is currently a no-op.
func (pr *Project) CanSaveFocused(c *ui.Context) bool {
	_, item := pr.focusedEditor(c)
	return item != nil && item.writable && item.loaded && !item.documentState().partial && !item.documentState().saving
}
func (pr *Project) SaveFocused(c *ui.Context) bool {
	path, item := pr.focusedEditor(c)
	if item == nil {
		return false
	}
	pr.saveEditor(c, path)
	return true
}
func (pr *Project) focusedEditor(c *ui.Context) (string, *fileItem) {
	if pr.Actions == nil {
		return "", nil
	}
	for path, item := range pr.Files.items {
		if item.document != nil && (item.document.focus[0].FocusWithin(c) || item.document.focus[1].FocusWithin(c)) {
			return path, item
		}
	}
	return "", nil
}
func (pr *Project) editorSourceAction(c *ui.Context, path, action, value string) {
	item := pr.Files.item(path)
	if !item.writable {
		return
	}
	state := item.documentState()
	source := item.draft
	var selected *editor.Selection
	if item.source == 0 && state.richSafe && state.rich != nil {
		mapped, selection, ok := richSourceSelection(source, state.rich)
		if !ok {
			item.source = 1
			kit.Toast(c, kit.Failed, i18n.T(c, "workspace_action_failed"))
			return
		}
		source, selected = mapped, &selection
	}
	expected := item.draft
	item.source = 1
	state.pending = func(native *editor.State) {
		current, selection := native.Buffer.String(), native.Selection()
		if selected != nil {
			if current != expected {
				kit.Toast(c, kit.Conflict, i18n.T(c, "workspace_changed"))
				return
			}
			current, selection = source, *selected
		}
		text, selection := editMarkdownSource(current, selection, action, value)
		native.Select(0, native.Buffer.Len())
		native.Insert(text)
		native.SetSelections(selection)
		pr.FileChanged(path, text)
		state.richSource = ""
	}
}
func markdownFile(path string) bool {
	switch strings.ToLower(path[strings.LastIndexByte(path, '.')+1:]) {
	case "md", "markdown", "mdown":
		return true
	}
	return false
}
