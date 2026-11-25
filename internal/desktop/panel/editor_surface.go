package panel

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/richtext"
)

type DocumentEditorState struct {
	Position struct {
		Line   int `json:"line"`
		Column int `json:"column"`
	} `json:"position"`
	Lines       int                   `json:"lines"`
	Language    string                `json:"language"`
	Mode        string                `json:"mode"`
	Dirty       bool                  `json:"dirty"`
	Saving      bool                  `json:"saving"`
	Markdown    bool                  `json:"markdown"`
	Notebook    bool                  `json:"notebook"`
	Copy        bool                  `json:"can_copy"`
	Wrap        bool                  `json:"wrap"`
	Editable    bool                  `json:"can_edit"`
	Readonly    bool                  `json:"readonly"`
	CanCutPaste bool                  `json:"can_cut_paste"`
	CanSave     bool                  `json:"can_save"`
	Format      *documentEditorFormat `json:"format"`
}

type documentEditorFormat struct {
	Block  *string `json:"block"`
	Bold   bool    `json:"bold"`
	Italic bool    `json:"italic"`
	Strike bool    `json:"strike"`
	Code   bool    `json:"code"`
	Link   bool    `json:"link"`
}

func (pr *Project) OpenDocumentEditor(path string, line int) {
	if !slices.Contains(pr.Files.open, path) {
		pr.Files.open = append(pr.Files.open, path)
	}
	if pr.Files.passing == path {
		pr.Files.passing = ""
	}
	if line > 0 {
		item := pr.Files.item(path)
		item.source, item.pendingLine = 1, line
	}
}

func (pr *Project) DocumentEditor(path string, writable bool, lines int) (DocumentEditorState, bool) {
	item := pr.Files.items[path]
	if item == nil || !item.loaded {
		return DocumentEditorState{}, false
	}
	pr.syncDocument(path, item)
	state := item.documentState()
	result := DocumentEditorState{Lines: lines, Mode: "source", Markdown: markdownFile(path), Notebook: strings.HasSuffix(strings.ToLower(path), ".ipynb"), Dirty: item.dirty, Saving: state.saving, Wrap: state.wrap}
	result.Position.Line, result.Position.Column = 1, 1
	if language := editor.LanguageFor(path); language != nil {
		result.Language = language.Name
	}
	if item.source == 0 && (result.Markdown || result.Notebook) {
		result.Mode = "document"
	}
	if result.Markdown && item.source == 0 && state.richSafe && state.rich != nil {
		anchor, caret := state.rich.Selection()
		result.Copy = anchor != caret
	} else if item.editor != nil {
		position := item.editor.CursorPos()
		result.Position.Line, result.Position.Column = position.Line+1, position.Col+1
		result.Lines = item.editor.Buffer.LineCount()
		selection := item.editor.Selection()
		result.Copy = selection.Anchor != selection.Head
	}
	result.Editable = writable && pr.fileWritable(item) && !state.unavailable && !state.partial
	result.Readonly = !result.Editable
	result.CanCutPaste = result.Editable && !(result.Notebook && item.source == 0) && !(result.Markdown && item.source == 0 && !state.richSafe)
	result.CanSave = result.Editable && item.dirty && !state.saving
	if result.Markdown && item.source == 0 && state.richSafe && state.rich != nil {
		value := currentEditorFormat(item)
		result.Format = &documentEditorFormat{Bold: value.bold, Italic: value.italic, Strike: value.strike, Code: value.code, Link: value.link}
		if value.block != "" {
			result.Format.Block = &value.block
		}
	}
	return result, true
}

func (pr *Project) RenderDocumentEditor(c *ui.Context, path string, writable func() bool, save func()) {
	pr.renderFileEditor(c, path, true, writable, save)
	pr.ActionDialog(c)
}

func (pr *Project) DocumentEditorAction(c *ui.Context, path, action string, line int) {
	item := pr.Files.item(path)
	state := item.documentState()
	pr.syncDocument(path, item)
	switch action {
	case "focus":
		state.focusFor(true).Focus(c)
	case "source":
		item.source = 1
	case "document":
		item.source = 0
	case "reveal":
		item.source, item.pendingLine = 1, line
	case "copy", "cut", "paste":
		if markdownFile(path) && item.source == 0 && state.richSafe && state.rich != nil {
			if action != "paste" {
				anchor, caret := state.rich.Selection()
				if start, end := min(anchor, caret), max(anchor, caret); start != end {
					c.WriteClipboard(state.rich.Text().Slice(start, end).String())
					if action == "cut" {
						state.rich.Insert(richtext.Text{})
					}
				}
			} else if text := c.ReadClipboard(); text != "" {
				state.rich.Insert(richtext.ParseMarkdown(text))
			}
		} else if item.editor != nil && (item.source != 0 || !markdownFile(path) && !strings.HasSuffix(strings.ToLower(path), ".ipynb")) {
			switch action {
			case "copy":
				item.editor.Run(editor.Copy)
			case "cut":
				item.editor.Run(editor.Cut)
			case "paste":
				item.editor.Run(editor.Paste)
			}
		}
	default:
		pr.runEditorAction(c, path, action)
	}
	pr.syncDocument(path, item)
}
