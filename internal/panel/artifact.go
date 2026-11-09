package panel

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-go/internal/conversation"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Artifact is a tab previewing a file of the conversation, a card's
// (content/files/panel.rs, preview.rs), by the file's ID.
const Artifact Kind = "artifact"

// ShowArtifact opens f's preview as a tab, or focuses it.
func (s *State) ShowArtifact(f conversation.File) {
	if s.artifacts == nil {
		s.artifacts = map[string]conversation.File{}
	}
	s.artifacts[f.ID] = f
	s.Show(Tab{Artifact, f.ID})
}

// artifact is a file's preview (content/files/preview.rs): the path bar,
// then the text in mono, the code highlighted, the Markdown rendered, the
// picture fitted, or the state of a preview that cannot show (loading,
// failed, too large, changed, unsupported) with Open in another app.
func (s *State) artifact(c *ui.Context, id string) {
	p := theme.Of(c)
	f, ok := s.artifacts[id]
	if !ok {
		return
	}
	toolbar(c, "artifact-bar", func() {
		ui.Icon(c, conversation.FileIcon(f)).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
		ui.Text(c, f.Path).FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		if f.Kind == conversation.CodeFile || f.Kind == conversation.TextFile || f.Kind == conversation.MarkdownFile {
			kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "content_copy"), 28, false).OnClick(func() {
				c.WriteClipboard(f.Text)
				c.Toast(i18n.T(c, "content_copied"))
			})
		}
		kit.IconButton(c.Key("open"), icons.ExternalLink, i18n.T(c, "files_open_system"), 28, false).OnClick(func() { notice(c) })
	})
	state := func(key string) {
		ui.Column(c.Key("state")).Grow(1).Padding(16).Gap(12).Children(func() {
			ui.Text(c, i18n.T(c, key)).FontSize(14).TextColor(p.MutedForeground)
			if key != "artifact_preview_loading" {
				kit.Secondary(c.Key("system"), i18n.T(c, "files_open_system")).AlignSelf(ui.Start).OnClick(func() { notice(c) })
			}
		})
	}
	switch f.State {
	case conversation.PreviewLoading:
		state("artifact_preview_loading")
		return
	case conversation.PreviewFailed:
		state("artifact_preview_failed")
		return
	case conversation.PreviewLarge:
		state("artifact_preview_large")
		return
	case conversation.PreviewChanged:
		state("artifact_preview_changed")
		return
	}
	ui.Scroll(c.Key("artifact-" + id)).Grow(1).MinHeight(0).Children(func() {
		switch f.Kind {
		case conversation.TextFile:
			ui.Text(c, f.Text).Selectable().Font("monospace").FontSize(13).LineHeight(20.0 / 13).Padding(16)
		case conversation.CodeFile:
			ext := f.Path[strings.LastIndexByte(f.Path, '.')+1:]
			ui.Column(c).Padding(12).Children(func() {
				markdown.View(c, fmt.Sprintf("```%s\n%s\n```", ext, strings.TrimRight(f.Text, "\n")), &s.md)
			})
		case conversation.MarkdownFile:
			ui.Column(c).Padding(16, 20).Children(func() { markdown.View(c, f.Text, &s.md) })
		case conversation.ImageFile:
			ui.Box(c).FillWidth().Padding(16).Center().Children(func() {
				if f.Image == nil || f.Image.Source == nil {
					kit.Loader(c, 20)
					return
				}
				w, h := float32(f.Image.Width), float32(f.Image.Height)
				img := ui.Image(c, f.Image.Source).FillWidth().Fit(ui.Contain)
				if w > 0 && h > 0 {
					img.AspectRatio(w / h).MaxWidth(w)
				}
			})
		case conversation.PDFFile:
			// MyGo v0.8.3 has no PDF view (docs/conversation.md, Deviations):
			// the document's facts and Open in another app.
			kit.Empty(c, icons.FileText, f.Name(), fmt.Sprintf("PDF · %d", f.Pages), func() {
				kit.Secondary(c.Key("system"), i18n.T(c, "files_open_system")).OnClick(func() { notice(c) })
			})
		default:
			state("artifact_preview_unavailable")
		}
	})
}

// Subagent is a tab holding a subagent's session (subagents/panel.rs).
const Subagent Kind = "subagent"

// ShowChild opens a subagent's session as a tab titled title, or focuses it.
func (s *State) ShowChild(id, title string) {
	if s.children == nil {
		s.children = map[string]string{}
	}
	s.children[id] = title
	s.Show(Tab{Subagent, id})
}
