package panel

import (
	"fmt"
	"runtime"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type editorFormat struct {
	block                            string
	bold, italic, strike, code, link bool
}
type editorControl struct {
	action, label            string
	icon                     *ui.SVG
	selected, disabled, menu bool
}

func currentEditorFormat(item *fileItem) editorFormat {
	state := item.documentState()
	format := editorFormat{block: "paragraph"}
	if item.source == 0 && state.richSafe && state.rich != nil {
		value := state.rich.Formatting()
		format.bold, format.italic, format.strike, format.code, format.link = value.Bold == richtext.FormatOn, value.Italic == richtext.FormatOn, value.Strikethrough == richtext.FormatOn, value.Code == richtext.FormatOn, value.Link != ""
		paragraph := value.Paragraph
		switch {
		case value.ParagraphsMixed:
			format.block = ""
		case paragraph.CodeBlock:
			format.block = "code"
		case paragraph.Heading > 0:
			format.block = fmt.Sprintf("heading%d", paragraph.Heading)
		case paragraph.List == richtext.Bullets:
			format.block = "bullet"
		case paragraph.List == richtext.Numbers:
			format.block = "ordered"
		case paragraph.List == richtext.Tasks:
			format.block = "task"
		case paragraph.Quote:
			format.block = "quote"
		}
		return format
	}
	if item.editor == nil {
		return format
	}
	source := item.editor.Buffer.String()
	selection := item.editor.Selection()
	start, end := selection.Start(), selection.End()
	selected := source[start:end]
	marked := func(mark string) bool {
		return len(selected) >= 2*len(mark) && strings.HasPrefix(selected, mark) && strings.HasSuffix(selected, mark) || start >= len(mark) && end+len(mark) <= len(source) && source[start-len(mark):start] == mark && source[end:end+len(mark)] == mark
	}
	format.bold = marked("**") || marked("__")
	format.italic = marked("*") && !marked("**") || marked("***") || marked("_") && !marked("__") || marked("___")
	format.strike, format.code = marked("~~"), marked("`")
	line := source[strings.LastIndexByte(source[:start], '\n')+1:]
	line = strings.SplitN(line, "\n", 2)[0]
	for level := 1; level <= 6; level++ {
		if strings.HasPrefix(line, strings.Repeat("#", level)+" ") {
			format.block = fmt.Sprintf("heading%d", level)
			return format
		}
	}
	switch {
	case strings.HasPrefix(line, "- [ ") || strings.HasPrefix(line, "- [x") || strings.HasPrefix(line, "- [X"):
		format.block = "task"
	case strings.HasPrefix(line, "- ") || strings.HasPrefix(line, "* ") || strings.HasPrefix(line, "+ "):
		format.block = "bullet"
	case strings.HasPrefix(line, "> [!"):
		format.block = "callout"
	case strings.HasPrefix(line, "> "):
		format.block = "quote"
	case strings.HasPrefix(line, "1. "):
		format.block = "ordered"
	}
	return format
}
func editorBlockLabel(c *ui.Context, block string) string {
	if block == "" {
		return i18n.T(c, "files_block")
	}
	if level, ok := strings.CutPrefix(block, "heading"); ok {
		return i18n.T(c, "files_block_heading", "level", level)
	}
	return i18n.T(c, "files_block_"+block)
}

// fitEditorControls mirrors the source's whole-control overflow calculation.
func fitEditorControls(groups [][]editorControl, room float32) ([][]editorControl, [][]editorControl) {
	type entry struct {
		control editorControl
		group   int
		width   float32
	}
	entries := []entry{}
	var used float32
	for group, controls := range groups {
		for index, control := range controls {
			width := float32(32)
			if index == 0 {
				width += 10
			}
			entries = append(entries, entry{control, group, width})
			used += width
		}
	}
	count := len(entries)
	if room >= 0 && used > room {
		used += 32
		for count > 0 && used > room {
			count--
			used -= entries[count].width
		}
	}
	split := func(items []entry) [][]editorControl {
		result := [][]editorControl{}
		previous := -1
		for _, item := range items {
			if item.group != previous {
				result = append(result, nil)
				previous = item.group
			}
			last := len(result) - 1
			result[last] = append(result[last], item.control)
		}
		return result
	}
	return split(entries[:count]), split(entries[count:])
}
func (pr *Project) editorToolbar(c *ui.Context, path string, markdown, notebook bool) {
	item := pr.Files.item(path)
	state := item.documentState()
	// Only a Markdown file's controls show the format at the caret; reading it
	// from a source editor copies the whole buffer.
	format := editorFormat{block: "paragraph"}
	if markdown {
		format = currentEditorFormat(item)
	}
	preview := notebook && item.source == 0
	control := func(action, label string, icon *ui.SVG, selected bool) editorControl {
		return editorControl{action: action, label: label, icon: icon, selected: selected, disabled: !pr.fileWritable(item)}
	}
	find := control("find", "files_find", icons.Search, false)
	find.disabled = false
	wrap := control("wrap", "files_wrap", icons.EditorWrap, state.wrap)
	wrap.disabled = false
	groups := [][]editorControl{{find, control("replace", "files_replace", icons.Replace, false)}, {wrap}}
	if preview {
		groups = [][]editorControl{{find}}
	} else if markdown {
		insert := control("insert", "files_insert", icons.Plus, false)
		insert.menu = true
		groups = [][]editorControl{
			{control("italic", "files_italic", icons.EditorItalic, format.italic), control("strike", "files_strike", icons.EditorStrike, format.strike), control("inline_code", "files_inline_code", icons.Code, format.code), control("link", "files_link", icons.EditorLink, format.link)},
			{control("block:bullet", "files_block_bullet", icons.EditorBullet, format.block == "bullet"), control("block:ordered", "files_block_ordered", icons.EditorOrdered, format.block == "ordered"), control("block:task", "files_block_task", icons.EditorTask, format.block == "task"), control("block:quote", "files_block_quote", icons.Quote, format.block == "quote"), control("insert:table", "files_insert_table", icons.EditorTable, false), insert},
			{find},
		}
		if item.source == 1 {
			groups = append(groups, []editorControl{wrap})
		}
	}
	palette := theme.Of(c)
	bar := ui.Row(c.Key("toolbar")).Height(barHeight).Shrink(0).PaddingX(8).Gap(4).BorderWidth(0, 0, 1, 0).BorderColor(palette.Border)
	reserved := float32(24 + 32)
	if !preview {
		reserved += 10 + 2*32
	}
	if markdown {
		reserved += 10 + 132 + 32
	}
	if markdown || notebook {
		reserved += 10 + 74
	}
	room := bar.LayoutBounds().W - reserved
	if bar.LayoutBounds().W <= 0 {
		room = -1
	}
	visible, hidden := fitEditorControls(groups, room)
	bar.Children(func() {
		kit.IconButtonHint(c.Key("save"), icons.Save, i18n.T(c, "files_save"), editorShortcut("save"), 28, false).KeepFocus().Disabled(!pr.fileWritable(item) || !item.dirty || state.saving).OnClick(func() { pr.saveEditor(c, path) })
		if !preview {
			separator(c)
			undo, redo := item.editor != nil && item.editor.Buffer.CanUndo(), item.editor != nil && item.editor.Buffer.CanRedo()
			if markdown && item.source == 0 && state.richSafe && state.rich != nil {
				undo, redo = state.rich.CanUndo(), state.rich.CanRedo()
			}
			kit.IconButtonHint(c.Key("undo"), icons.Undo, i18n.T(c, "files_undo"), editorShortcut("undo"), 28, false).KeepFocus().Disabled(!pr.fileWritable(item) || !undo).OnClick(func() { pr.runEditorAction(c, path, "undo") })
			kit.IconButtonHint(c.Key("redo"), icons.Redo, i18n.T(c, "files_redo"), editorShortcut("redo"), 28, false).KeepFocus().Disabled(!pr.fileWritable(item) || !redo).OnClick(func() { pr.runEditorAction(c, path, "redo") })
		}
		if markdown {
			separator(c)
			kit.MenuButton(c.Key("block"), "file-block-menu", func(button ui.Element) {
				kit.GhostLook(c, button, false).Row().Height(28).MinWidth(100).Gap(4).PaddingX(8).Disabled(!pr.fileWritable(item)).Children(func() {
					ui.Text(c, editorBlockLabel(c, format.block)).FontSize(12)
					ui.Icon(c, icons.ChevronDown).FontSize(12)
				})
			}, func(menu *kit.Menu) {
				for _, block := range []string{"paragraph", "heading1", "heading2", "heading3", "heading4", "heading5", "heading6", "code", "callout"} {
					if block == "code" {
						menu.Separator()
					}
					choice := menu.Item(editorBlockLabel(c, block)).Shortcut(editorShortcut("block:" + block)).Checked(format.block == block).Disabled(!pr.fileWritable(item))
					if choice.Chosen() {
						pr.runEditorAction(c, path, "block:"+block)
					}
				}
			})
			pr.editorControl(c, path, control("bold", "files_bold", icons.EditorBold, format.bold))
		}
		for gi, group := range visible {
			if !markdown || gi > 0 {
				separator(c)
			}
			if markdown && gi == 2 {
				ui.Spacer(c)
			}
			for _, item := range group {
				pr.editorControl(c, path, item)
			}
		}
		ui.Spacer(c)
		if len(hidden) > 0 {
			kit.MenuButton(c.Key("more"), "file-more-menu", func(button ui.Element) {
				kit.IconLook(c, button, icons.Ellipsis, i18n.T(c, "files_more"), "", 28, false)
			}, func(menu *kit.Menu) {
				for gi, group := range hidden {
					if gi > 0 {
						menu.Separator()
					}
					for _, item := range group {
						if item.menu {
							menu.Submenu(i18n.T(c, item.label), func(sub *kit.Menu) { pr.editorInsertMenu(c, path, sub, item.disabled) }).Disabled(item.disabled)
						} else if menu.Item(i18n.T(c, item.label)).Icon(item.icon).Shortcut(editorShortcut(item.action)).Checked(item.selected).Disabled(item.disabled).Chosen() {
							pr.runEditorAction(c, path, item.action)
						}
					}
				}
			})
		}
		if markdown || notebook {
			separator(c)
			label := "content_document"
			if notebook {
				label = "content_preview"
			}
			kit.Segmented(c, "view", &item.source, []string{i18n.T(c, label), i18n.T(c, "content_source")}, kit.SegmentedProps{})
		}
	})
}
func (pr *Project) editorControl(c *ui.Context, path string, item editorControl) {
	if item.menu {
		kit.MenuButton(c.Key(item.action), "file-insert-menu", func(button ui.Element) {
			kit.IconLook(c, button, item.icon, i18n.T(c, item.label), "", 28, false)
			button.Disabled(item.disabled)
		}, func(menu *kit.Menu) { pr.editorInsertMenu(c, path, menu, item.disabled) })
		return
	}
	kit.IconButtonHint(c.Key(item.action), item.icon, i18n.T(c, item.label), editorShortcut(item.action), 28, item.selected).KeepFocus().Disabled(item.disabled).OnClick(func() { pr.runEditorAction(c, path, item.action) })
}
func editorShortcut(action string) string {
	primary, shift, alt := "Ctrl+", "Shift+", "Alt+"
	if runtime.GOOS == "darwin" {
		primary, shift, alt = "⌘", "⇧", "⌥"
	}
	key := map[string]string{"save": "S", "undo": "Z", "redo": shift + "Z", "bold": "B", "italic": "I", "strike": shift + "X", "inline_code": "E", "link": "K", "find": "F", "block:paragraph": alt + "0"}[action]
	if level, ok := strings.CutPrefix(action, "block:heading"); ok {
		key = alt + level
	}
	if key == "" {
		return ""
	}
	return primary + key
}
func (pr *Project) editorInsertMenu(c *ui.Context, path string, menu *kit.Menu, disabled bool) {
	for _, kind := range []struct {
		name string
		icon *ui.SVG
	}{{"image", icons.Image}, {"rule", icons.EditorRule}, {"code", icons.EditorCode}, {"mermaid", icons.EditorMermaid}, {"math", icons.EditorMath}, {"chart", icons.EditorChart}, {"html", icons.FileCode}} {
		if kind.name == "mermaid" || kind.name == "html" {
			menu.Separator()
		}
		if menu.Item(i18n.T(c, "files_insert_"+kind.name)).Icon(kind.icon).Disabled(disabled).Chosen() {
			pr.runEditorAction(c, path, "insert:"+kind.name)
		}
	}
}
