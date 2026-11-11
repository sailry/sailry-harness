package conversation

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Cards of a reply's Markdown (live/messages.rs on_file, on_image): a link
// to a workspace file standing alone in its paragraph is a file card; a
// paragraph of images shows the conversation's and the workspace's pictures
// as thumbnails, a picture that is gone as a line of text, another workspace
// file as a file card and anything else as a link. Web images stay
// Markdown's.

// embed builds paragraph i of d as its cards and reports whether it did.
func (s *State) embed(c *ui.Context, session string, d *parser.Document, i int, actions Actions) bool {
	if d.Block(i).Kind != parser.Paragraph {
		return false
	}
	inlines := d.Inlines(i)
	var links, images []parser.Inline
	label := ""
	for at := 0; at < len(inlines); {
		in := inlines[at]
		switch {
		case in.Kind == parser.Link && !in.Pending && !in.Auto:
			links = append(links, in)
			for _, child := range inlines[at+1 : in.Next] {
				label += child.Text
			}
		case in.Kind == parser.Image && !in.Pending:
			images = append(images, in)
		case in.Kind == parser.SoftBreak, in.Kind == parser.LineBreak, in.Kind == parser.Text && strings.TrimSpace(in.Text) == "":
		default:
			return false
		}
		at = max(in.Next, at+1)
	}
	switch {
	case len(links) == 1 && len(images) == 0:
		e, ok := s.Embed(session, links[0].Dest, label, false)
		if !ok || e.File == nil {
			return false
		}
		fileCard(c, "embed-file-"+e.File.ID, *e.File, actions)
		return true
	case len(links) > 0 || len(images) == 0:
		return false
	}
	embeds := make([]Embed, len(images))
	for n, in := range images {
		e, ok := s.Embed(session, in.Dest, "", true)
		if !ok && webLink(in.Dest) {
			return false
		}
		embeds[n] = e
	}
	p := theme.Of(c)
	ui.Column(c.Key("embed")).FillWidth().Gap(12).Children(func() {
		var pictures []Image
		flush := func(n int) {
			if len(pictures) > 0 {
				s.thumbnails(c, "embed-images-"+pictures[0].ID, pictures, false)
				pictures = nil
			}
		}
		for n, e := range embeds {
			switch {
			case e.Image != nil:
				pictures = append(pictures, *e.Image)
				continue
			}
			flush(n)
			switch dest := images[n].Dest; {
			case e.Missing:
				ui.Text(c.Key(n), i18n.T(c, "chat_image_missing")).FontSize(14).TextColor(p.MutedForeground)
			case e.File != nil:
				fileCard(c, "embed-file-"+e.File.ID, *e.File, actions)
			default:
				ui.ButtonBase(c.Key(n)).AlignSelf(ui.Start).MaxWidthPercent(100).Cursor(ui.CursorPointer).Children(func() {
					ui.Text(c, dest).FontSize(14).SingleLine().TextColor(p.Primary)
				}).OnClick(func() {
					if s.md.OnLink != nil {
						s.md.OnLink(dest)
					}
				})
			}
		}
		flush(len(embeds))
	})
	return true
}

// sessionIcon gives a link that opens a session the agent's icon before
// its text, in the link's color.
func sessionIcon(c *ui.Context, session func(string) bool) func(string, float32) (markdown.InlineObject, bool) {
	color := theme.Of(c).Primary
	return func(dest string, size float32) (markdown.InlineObject, bool) {
		if !session(dest) {
			return markdown.InlineObject{}, false
		}
		return markdown.InlineObject{Width: size + 4, Height: size * 0.85, Depth: size * 0.15, Draw: func(p *ui.Painter, r ui.Rect) {
			p.Icon(icons.Bot, ui.Rect{X: r.X, Y: r.Y, W: size, H: size}, color)
		}}, true
	}
}
