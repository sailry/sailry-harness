package conversation

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// A Markdown file reference is navigation, not a publication. Explicit file
// and artifact blocks own file cards; image markup owns thumbnails and their
// file fallback. Ordinary links retain Markdown's inline click behavior.

// embed builds paragraph i of d as its cards and reports whether it did.
func (s *State) embed(c *ui.Context, session string, d *parser.Document, i int, actions Actions) bool {
	if d.Block(i).Kind != parser.Paragraph {
		return false
	}
	inlines := d.Inlines(i)
	var images []parser.Inline
	for at := 0; at < len(inlines); {
		in := inlines[at]
		switch {
		case in.Kind == parser.Image && !in.Pending:
			images = append(images, in)
		case in.Kind == parser.SoftBreak, in.Kind == parser.LineBreak, in.Kind == parser.Text && strings.TrimSpace(in.Text) == "":
		default:
			return false
		}
		at = max(in.Next, at+1)
	}
	if len(images) == 0 {
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
				// The place too: a paragraph may embed a target twice.
				s.thumbnails(c, fmt.Sprintf("embed-images-%d-%s", n, pictures[0].ID), pictures, false)
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
				ui.Text(c.Key(n), i18n.T(c, "chat_image_missing")).FontSize(textSize(c)).TextColor(p.MutedForeground)
			case e.File != nil:
				fileCard(c, fmt.Sprintf("embed-file-%d-%s", n, e.File.ID), *e.File, actions)
			default:
				ui.ButtonBase(c.Key(n)).AlignSelf(ui.Start).MaxWidthPercent(100).Cursor(ui.CursorPointer).Children(func() {
					ui.Text(c, dest).FontSize(textSize(c)).SingleLine().TextColor(p.Primary)
				}).OnClick(func() { s.follow(c, session, dest) })
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
