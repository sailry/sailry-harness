package panel

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/richtext"
)

func (pr *Project) runEditorAction(c *ui.Context, path, action string) {
	item := pr.Files.item(path)
	state := item.documentState()
	if action == "wrap" {
		state.wrap = !state.wrap
		if item.editor != nil {
			item.editor.SoftWrap = state.wrap
		}
		return
	}
	if action == "find" || action == "replace" {
		item.source = 1
		state.pending = func(native *editor.State) {
			if action == "replace" && item.writable {
				native.Run(editor.FindReplace)
			} else {
				native.Find("")
			}
		}
		return
	}
	if !item.writable {
		return
	}
	rich := markdownFile(path) && item.source == 0 && state.richSafe && state.rich != nil
	if action == "undo" || action == "redo" {
		if rich {
			if action == "undo" {
				state.rich.Undo()
			} else {
				state.rich.Redo()
			}
			pr.syncRichDraft(path)
		} else if item.editor != nil {
			changed := false
			if action == "undo" {
				changed = item.editor.Buffer.Undo()
			} else {
				changed = item.editor.Buffer.Redo()
			}
			if changed {
				pr.FileChanged(path, item.editor.Buffer.String())
			}
		}
		return
	}
	if action == "link" || action == "insert:image" {
		value := ""
		if rich && action == "link" {
			value = state.rich.Formatting().Link
		}
		label := "files_link"
		if action == "insert:image" {
			label = "files_insert_image"
		}
		pr.Prompt(label, []string{"mcp_url"}, []string{value}, func(values []string, finish func(bool)) {
			if rich && action == "link" {
				state.rich.SetLink(values[0])
				pr.syncRichDraft(path)
			} else {
				pr.editorSourceAction(c, path, action, values[0])
			}
			finish(true)
		})
		return
	}
	if !rich || strings.HasPrefix(action, "insert:") || action == "block:callout" {
		pr.editorSourceAction(c, path, action, "")
		return
	}
	switch action {
	case "bold":
		state.rich.ToggleBold()
	case "italic":
		state.rich.ToggleItalic()
	case "strike":
		state.rich.ToggleStrikethrough()
	case "inline_code":
		state.rich.ToggleCode()
	case "block:paragraph":
		state.rich.FormatParagraphs(func(paragraph *richtext.ParagraphStyle) { *paragraph = richtext.ParagraphStyle{ID: paragraph.ID} })
	case "block:bullet":
		state.rich.ToggleList(richtext.Bullets)
	case "block:ordered":
		state.rich.ToggleList(richtext.Numbers)
	case "block:task":
		state.rich.ToggleList(richtext.Tasks)
	case "block:quote":
		state.rich.ToggleQuote()
	case "block:code":
		state.rich.ToggleCodeBlock()
	default:
		if heading, ok := strings.CutPrefix(action, "block:heading"); ok && len(heading) == 1 {
			state.rich.SetHeading(int(heading[0] - '0'))
		}
	}
	pr.syncRichDraft(path)
}
