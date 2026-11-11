package panel

import (
	"runtime"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func (pr *Project) liveFileEditor(c *ui.Context, path string, own bool) {
	file := findFile(pr.Data.Files, path)
	if file == nil || path == "" {
		// No file is open: "" is the tree's root, not a file.
		empty(c, icons.File, "file_empty")
		return
	}
	item := pr.Files.item(path)
	if item.documentState().unavailable && !item.dirty {
		kit.Result(c, kit.ResultProps{Icon: icons.File, Title: i18n.T(c, "file_empty")})
		return
	}
	if !item.loaded {
		pr.SetFile(path, file.Text)
	}
	state := item.documentState()
	markdown, notebook := markdownFile(path), strings.HasSuffix(strings.ToLower(path), ".ipynb")
	if item.pendingLine > 0 {
		item.source = 1
	}
	if markdown && (state.rich == nil || state.richSource != item.draft) {
		state.richSafe = richMarkdownSafe(item.draft)
		state.rich = richtext.NewDocument(richtext.ParseMarkdown(item.draft))
		state.richSource = item.draft
		state.richOriginal, state.richCanonical = item.draft, state.rich.Text().Markdown()
		state.richVersion = state.rich.Version()
	}
	pr.editorToolbar(c, path, markdown, notebook)
	switch {
	case notebook && item.source == 0:
		pr.notebookEditorPreview(c, path, own)
	case markdown && item.source == 0 && state.richSafe:
		ui.Column(c.Key("document")).Bind(state.focusFor(own)).Grow(1).MinHeight(0).Padding(12, 16).Children(func() {
			options := richtext.EditorOptions{ReadOnly: !item.writable, OnLinkCommand: func() { pr.runEditorAction(c, path, "link") }, OnKey: func(key ui.Key, mods ui.Modifiers) bool { return pr.editorKey(c, path, key, mods) }}
			if richtext.EditorBase(c.Key(path), state.rich, options).Grow(1).MinHeight(0).Changed() {
				pr.syncRichDraft(path)
			}
		})
	case markdown && item.source == 0:
		ui.Scroll(c.Key("document")).Bind(state.focusFor(own)).TrackScroll(scroll(&item.text, own)).Grow(1).MinHeight(0).Padding(12, 16).Children(func() { kit.Markdown(c, item.draft+item.suffix, kit.MarkdownProps{}) })
	default:
		ui.Scroll(c.Key("text")).Bind(state.focusFor(own)).TrackScroll(scroll(&item.text, own)).Grow(1).MinHeight(0).Padding(12, 0).Children(func() {
			kit.Code(c, ui.Column(c.Key(path)), item.draft, kit.CodeOptions{Language: editor.LanguageFor(path), Numbers: true, Editable: item.writable, Changed: func(text string) {
				pr.FileChanged(path, text)
			}, Bind: func(native *editor.State) {
				if !own || item.editor != native {
					native.SoftWrap = state.wrap
				} else {
					state.wrap = native.SoftWrap
				}
				if own {
					pr.bindEditor(path, native)
				}
				native.ReadOnly = !item.writable
				native.OnInput = func(event ui.InputEvent) bool {
					return event.Kind == ui.InputKeyDown && pr.editorKey(c, path, event.Key, event.Mods)
				}
				if pending := state.pending; own && pending != nil {
					state.pending = nil
					pending(native)
				}
			}})
			if own && item.editor != nil {
				state.wrap = item.editor.SoftWrap
			}
		})
	}
	pr.editorFooter(c, path, markdown)
}
func editorPrimaryModifier() ui.Modifiers {
	if runtime.GOOS == "darwin" {
		return ui.Super
	}
	return ui.Ctrl
}
func (pr *Project) editorKey(c *ui.Context, path string, key ui.Key, mods ui.Modifiers) bool {
	primary := editorPrimaryModifier()
	if mods == primary && key == ui.KeyS {
		pr.saveEditor(c, path)
		return true
	}
	if !markdownFile(path) || !pr.Files.item(path).writable {
		return false
	}
	action := ""
	if mods == primary {
		action = map[ui.Key]string{ui.KeyB: "bold", ui.KeyI: "italic", ui.KeyE: "inline_code", ui.KeyK: "link"}[key]
	} else if mods == primary|ui.Shift && key == ui.KeyX {
		action = "strike"
	} else if mods == primary|ui.Alt && key >= ui.Key0 && key <= ui.Key6 {
		action = "block:paragraph"
		if key != ui.Key0 {
			action = "block:heading" + string(rune('0'+key-ui.Key0))
		}
	}
	if action == "" {
		return false
	}
	pr.runEditorAction(c, path, action)
	return true
}
func (pr *Project) saveEditor(c *ui.Context, path string) {
	item := pr.Files.item(path)
	if item.writable && item.loaded && item.dirty && !item.documentState().saving && !item.documentState().partial && pr.Actions != nil && pr.Actions.SaveFile != nil {
		pr.Actions.SaveFile(c, path, item.draft+item.suffix)
	}
}
func (pr *Project) syncRichDraft(path string) {
	item := pr.Files.item(path)
	state := item.documentState()
	if state.rich != nil && state.rich.Version() != state.richVersion {
		state.richVersion = state.rich.Version()
		text := state.rich.Text().Markdown()
		if text == state.richCanonical {
			text = state.richOriginal
		}
		state.richSource = text
		pr.FileChanged(path, text)
	}
}
func (pr *Project) editorFooter(c *ui.Context, path string, markdown bool) {
	item := pr.Files.item(path)
	state := item.documentState()
	line, column := 1, 1
	if markdown && item.source == 0 && state.richSafe && state.rich != nil {
		_, caret := state.rich.Selection()
		text := []rune(state.rich.Text().String())
		prefix := string(text[:min(caret, len(text))])
		line = strings.Count(prefix, "\n") + 1
		column = utf8.RuneCountInString(prefix[strings.LastIndexByte(prefix, '\n')+1:]) + 1
	} else if item.editor != nil {
		position := item.editor.CursorPos()
		line, column = position.Line+1, position.Col+1
	}
	language := ""
	if value := editor.LanguageFor(path); value != nil {
		language = value.Name
	}
	palette := theme.Of(c)
	ui.Row(c.Key("footer")).Height(footer).Shrink(0).PaddingX(12).Gap(12).BorderWidth(1, 0, 0, 0).BorderColor(palette.Border).TextColor(palette.MutedForeground).Children(func() {
		ui.Text(c, path).FontSize(12).SingleLine().Grow(1).Shrink(1).ContextMenu(func(menu *ui.Menu) {
			if menu.Item(i18n.T(c, "files_copy_path")).Chosen() {
				c.WriteClipboard(path)
			}
			if menu.Item(i18n.T(c, "files_open_system")).Chosen() {
				pr.fileAction(c, FileAction{Kind: "files_open_system", Paths: []string{path}})
			}
		})
		if state.saving {
			ui.Text(c, i18n.T(c, "files_saving")).FontSize(12).SingleLine()
		} else if state.changed {
			ui.Text(c, i18n.T(c, "files_changed_on_disk")).FontSize(12).SingleLine()
		} else if state.partial {
			ui.Text(c, i18n.T(c, "files_content_partial")).FontSize(12).SingleLine()
		}
		ui.Text(c, i18n.T(c, "files_cursor_position", "line", line, "column", column)).FontSize(12).SingleLine()
		ui.Text(c, "UTF-8").FontSize(12).SingleLine()
		if language != "" {
			ui.Text(c, language).FontSize(12).SingleLine()
		}
	})
}
