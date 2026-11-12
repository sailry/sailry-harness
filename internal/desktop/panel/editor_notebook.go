package panel

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func (pr *Project) notebookEditorPreview(c *ui.Context, path string, own bool) {
	item := pr.Files.item(path)
	state := item.documentState()
	draft := item.draft + item.suffix
	if pr.Actions.ReadNotebook != nil && (!state.notebookRequested || state.notebookDraft != draft) {
		state.notebookRequested, state.notebookDraft = true, draft
		pr.Actions.ReadNotebook(path, 0)
	}
	page := state.notebook
	if page != nil && !page.Loading && len(page.Cells) == 0 && !page.Failed {
		// A notebook without cells is an empty document: the state fills
		// the area, which a scroll's content does not.
		empty(c, icons.File, "notebook_empty")
		return
	}
	ui.Scroll(c.Key("notebook")).Bind(state.focusFor(own)).TrackScroll(scroll(&item.text, own)).Grow(1).MinHeight(0).Padding(12).Gap(12).Children(func() {
		if page == nil || page.Loading && len(page.Cells) == 0 {
			kit.Skeleton(c, kit.SkeletonProps{Height: 48})
			return
		}
		palette := theme.Of(c)
		for _, cell := range page.Cells {
			label := i18n.T(c, "notebook_cell", "number", cell.Index+1)
			ui.Column(c.Key(cell.Index)).Gap(8).Padding(12).Border(1, palette.Border).Radius(palette.Radius).Children(func() {
				ui.Row(c).Gap(8).Children(func() {
					ui.Text(c, label).FontWeight(500)
					ui.Text(c, cell.Kind).FontSize(12).TextColor(palette.MutedForeground)
				})
				if cell.Kind == "markdown" {
					kit.Markdown(c, cell.Source, kit.MarkdownProps{})
				} else {
					notebookEditorCode(c.Key("source"), cell.Source, page.Language, cell.Kind == "code")
				}
				for index, output := range cell.Outputs {
					notebookEditorCode(c.Key(index), output, "", false)
				}
				for _, image := range cell.Images {
					width, height := float32(image.Width), float32(image.Height)
					if width > 0 && height > 0 {
						kit.Image(c.Key("image-"+image.ID), image.Source, kit.ImageProps{Contain: true, Variant: kit.ImageBorderless, Alt: label}).MaxWidth(width).MaxHeight(512).AspectRatio(width / height)
					}
				}
				if cell.Truncated {
					ui.Text(c, i18n.T(c, "files_content_partial")).FontSize(12).TextColor(palette.MutedForeground)
				}
			})
		}
	})
	if page != nil && (page.Failed || page.Next != nil) {
		label := "notebook_more"
		if page.Failed {
			label = "chat_retry"
		}
		kit.Ghost(c.Key("notebook-more"), i18n.T(c, label), false).Height(32).FillWidth().Disabled(page.Loading || pr.Actions.ReadNotebook == nil).Children(func() { ui.Text(c, i18n.T(c, label)).FontSize(12) }).OnClick(func() {
			offset := uint64(0)
			if page.Next != nil {
				offset = *page.Next
			}
			pr.Actions.ReadNotebook(path, offset)
		})
	}
}

func notebookEditorCode(c *ui.Context, source, language string, code bool) {
	height := float32(min(max(strings.Count(source, "\n")+1, 1), 24)*20 + 20)
	var syntax *editor.Language
	if code {
		syntax = editor.LanguageNamed(language)
	}
	kit.Code(c, ui.Column(c).Height(height).Clip(), source, kit.CodeOptions{Language: syntax, Numbers: true, Bind: func(native *editor.State) {
		native.ReadOnly, native.FitContent, native.NoScrollbars = true, false, false
	}})
}

// A failed draft preview keeps its bytes and returns to their source editor.
func (pr *Project) EditorNotebookFailed(path string, page *viewmodel.EditorNotebook, draft bool) {
	if pr.EditorNotebook(path) != page {
		return
	}
	page.Loading, page.Failed = false, true
	if draft {
		pr.Files.items[path].source = 1
	}
}
