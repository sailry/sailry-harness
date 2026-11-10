package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Artifact is a tab previewing a file of the conversation, a card's, by the
// file's ID.
const Artifact Kind = "artifact"

type ArtifactCallbacks struct {
	OpenExternal func(conversation.File)
	Image        func(kit.ImageAction, conversation.Image)
	Render       func(*ui.Context, conversation.File) bool
}

func (s *State) SetArtifactCallbacks(callbacks ArtifactCallbacks) { s.artifactCallbacks = callbacks }
func (s *State) UpdateArtifact(file conversation.File) {
	if s.artifacts == nil {
		s.artifacts = map[string]conversation.File{}
	}
	s.artifacts[file.ID] = file
}

// ShowArtifact opens f's preview as a tab, or focuses it.
func (s *State) ShowArtifact(f conversation.File) {
	if s.artifacts == nil {
		s.artifacts = map[string]conversation.File{}
	}
	s.artifacts[f.ID] = f
	s.Show(Tab{Artifact, f.ID})
}

// artifact is a file's preview: the path bar, then the text in mono, the code
// highlighted, the Markdown rendered, the picture fitted, or the state of a
// preview that cannot show (loading, failed, too large, changed, unsupported)
// with Open in another app.
func (s *State) artifact(c *ui.Context, id string) {
	p := theme.Of(c)
	f, ok := s.artifacts[id]
	if !ok {
		return
	}
	open := func() {
		if s.artifactCallbacks.OpenExternal != nil {
			s.artifactCallbacks.OpenExternal(f)
		} else {
			notice(c)
		}
	}
	toolbar(c, "artifact-bar", func() {
		ui.Icon(c, conversation.FileIcon(f)).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
		ui.Text(c, f.Path).FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		if f.Kind == conversation.CodeFile || f.Kind == conversation.TextFile || f.Kind == conversation.MarkdownFile {
			kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "content_copy"), 28, false).OnClick(func() {
				c.WriteClipboard(f.Text)
				kit.Message(c, i18n.T(c, "content_copied"))
			})
		}
		kit.IconButton(c.Key("open"), icons.ExternalLink, i18n.T(c, "files_open_system"), 28, false).OnClick(open)
	})
	state := func(key string) {
		ui.Column(c.Key("state")).Grow(1).Padding(16).Gap(12).Children(func() {
			ui.Text(c, i18n.T(c, key)).FontSize(14).TextColor(p.MutedForeground)
			if key != "artifact_preview_loading" {
				kit.Secondary(c.Key("system"), i18n.T(c, "files_open_system")).AlignSelf(ui.Start).OnClick(open)
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
	if s.artifactCallbacks.Render != nil && s.artifactCallbacks.Render(c, f) {
		return
	}
	kit.ImageViewer(c, &s.view, func(im conversation.Image) kit.ViewerItem {
		return kit.ViewerItem{Source: im.Source, Name: im.Name}
	}, func(action kit.ImageAction, image conversation.Image) {
		if s.artifactCallbacks.Image != nil {
			s.artifactCallbacks.Image(action, image)
		} else {
			notice(c)
		}
	})
	ui.Scroll(c.Key("artifact-" + id)).Grow(1).MinHeight(0).Children(func() {
		switch f.Kind {
		case conversation.TextFile:
			kit.Code(c, ui.Column(c.Key(f.ID)).PaddingY(12), f.Text, kit.CodeOptions{Numbers: true})
		case conversation.CodeFile:
			kit.Code(c, ui.Column(c.Key(f.ID)).PaddingY(12), f.Text, kit.CodeOptions{Language: editor.LanguageFor(f.Path), Numbers: true})
		case conversation.MarkdownFile:
			ui.Column(c).Padding(16, 20).Children(func() { kit.Markdown(c, f.Text, kit.MarkdownProps{}) })
		case conversation.ImageFile:
			ui.Box(c).FillWidth().Padding(16).Center().Children(func() {
				if f.Image == nil || f.Image.Source == nil {
					kit.Image(c, nil, kit.ImageProps{Height: 240, Alt: i18n.T(c, "chat_image_loading")})
					return
				}
				im := *f.Image
				w, h := float32(im.Width), float32(im.Height)
				img := kit.Image(c, im.Source, kit.ImageProps{Fill: true, Contain: true, Variant: kit.ImageBorderless, Alt: f.Path,
					OnOpen: func() { s.view.Show([]conversation.Image{im}, 0) }}).FillWidth()
				if w > 0 && h > 0 {
					img.AspectRatio(w / h).MaxWidth(w)
				}
			})
		}
	})
}

// Subagent is a tab holding a subagent's session.
const Subagent Kind = "subagent"

// ShowChild opens a subagent's session as a tab titled title, or focuses it.
func (s *State) ShowChild(id, title string) {
	if s.children == nil {
		s.children = map[string]string{}
	}
	s.children[id] = title
	s.Show(Tab{Subagent, id})
}
