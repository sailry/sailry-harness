package conversation

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// A deliverable shows as what it is: a link standing alone in its paragraph
// to a produced document, page, export or media file is a file card, and to a
// picture its thumbnail. Image markup shows pictures and falls back to a
// file card or clickable link. Source references and links within text remain
// links; the resolver decides which files are deliverables.

type embedEntry struct {
	dest         string
	fallbackDest string
	label        string
	link         bool
}

type embedRun struct {
	text  string
	entry *embedEntry
	embed Embed
}

// wrappedImage reports a link whose content is a single complete image.
func wrappedImage(inlines []parser.Inline) (dest, label string, ok bool) {
	found := false
	for at := 0; at < len(inlines); {
		in := inlines[at]
		switch {
		case in.Kind == parser.Image && !in.Pending:
			if found {
				return "", "", false
			}
			found = true
			dest = in.Dest
			label = strings.TrimSpace(parser.PlainText(inlines[at+1 : in.Next]))
		case in.Kind == parser.SoftBreak, in.Kind == parser.LineBreak, in.Kind == parser.Text && strings.TrimSpace(in.Text) == "":
		default:
			return "", "", false
		}
		at = max(in.Next, at+1)
	}
	return dest, label, found
}

// embed builds paragraph i of d as its cards and reports whether it did.
func (s *State) embed(c *ui.Context, session string, d *parser.Document, i int, actions Actions, resolve func(string, string, string, bool) (Embed, bool)) bool {
	if d.Block(i).Kind != parser.Paragraph {
		return false
	}
	inlines := d.Inlines(i)
	parentKind := d.Block(d.Block(i).Parent).Kind
	var entries []embedEntry
	links := 0
	standalone := true
	for at := 0; at < len(inlines); {
		in := inlines[at]
		switch {
		case in.Kind == parser.Image && !in.Pending:
			entries = append(entries, embedEntry{
				dest:         in.Dest,
				fallbackDest: in.Dest,
				label:        strings.TrimSpace(parser.PlainText(inlines[at+1 : in.Next])),
			})
		case in.Kind == parser.Link && !in.Pending && !in.Auto:
			if imgDest, imgLabel, ok := wrappedImage(inlines[at+1 : in.Next]); ok {
				entries = append(entries, embedEntry{
					dest:         imgDest,
					fallbackDest: in.Dest,
					label:        imgLabel,
				})
			} else if parentKind == parser.Root {
				// Only a top-level paragraph delivers; a list or quote refers.
				links++
				entries = append(entries, embedEntry{
					dest:         in.Dest,
					fallbackDest: in.Dest,
					label:        strings.TrimSpace(parser.PlainText(inlines[at+1 : in.Next])),
					link:         true,
				})
			} else {
				standalone = false
			}
		case in.Kind == parser.SoftBreak, in.Kind == parser.LineBreak, in.Kind == parser.Text && strings.TrimSpace(in.Text) == "":
		default:
			standalone = false
		}
		if !standalone {
			break
		}
		at = max(in.Next, at+1)
	}
	if standalone && len(entries) > 0 && (links == 0 || len(entries) == 1) {
		embeds := make([]Embed, len(entries))
		for n, entry := range entries {
			e, ok := resolve(session, entry.dest, entry.label, !entry.link)
			if links > 0 && !ok || !ok && webLink(entry.dest) {
				return false
			}
			embeds[n] = e
		}
		ui.Column(c.Key("embed")).FillWidth().Gap(12).Children(func() {
			s.renderEmbedEntries(c, session, 0, entries, embeds, actions)
		})
		return true
	}
	return s.embedMixed(c, session, d, i, inlines, parentKind == parser.Root, actions, resolve)
}

// embedMixed splits a paragraph containing both text and complete image markup
// into Markdown text runs and embedded image cards.
func (s *State) embedMixed(c *ui.Context, session string, d *parser.Document, _ int, inlines []parser.Inline, topLevel bool, actions Actions, resolve func(string, string, string, bool) (Embed, bool)) bool {
	src := d.Source()
	hasImage := false
	for at := 0; at < len(inlines); at++ {
		in := inlines[at]
		switch in.Kind {
		case parser.FootnoteRef:
			return false
		case parser.Image:
			if !in.Pending {
				hasImage = true
			}
		case parser.Link:
			if !in.Auto && in.Start >= 0 && in.End <= len(src) && in.Start < in.End && !strings.Contains(src[in.Start:in.End], "](") {
				// Preserve reference-style links inside the complete document view.
				return false
			}
		}
	}
	if !hasImage {
		return false
	}
	var runs []embedRun
	textStart, textEnd := -1, -1
	flushText := func() bool {
		if textStart < 0 || textEnd <= textStart || textEnd > len(src) {
			textStart, textEnd = -1, -1
			return true
		}
		text := strings.TrimSpace(src[textStart:textEnd])
		textStart, textEnd = -1, -1
		if text == "" {
			return true
		}
		if !topLevel && strings.Contains(text, "\n") {
			// Multiline slices inside blockquotes or lists can carry container prefixes.
			return false
		}
		text, ok := inlineRun(text, d.Extensions())
		if !ok {
			return false
		}
		runs = append(runs, embedRun{text: text})
		return true
	}
	resolved := 0
	for at := 0; at < len(inlines); {
		in := inlines[at]
		var entry *embedEntry
		switch {
		case in.Kind == parser.Image && !in.Pending:
			entry = &embedEntry{
				dest:         in.Dest,
				fallbackDest: in.Dest,
				label:        strings.TrimSpace(parser.PlainText(inlines[at+1 : in.Next])),
			}
		case in.Kind == parser.Link && !in.Pending && !in.Auto:
			if imgDest, imgLabel, ok := wrappedImage(inlines[at+1 : in.Next]); ok {
				entry = &embedEntry{
					dest:         imgDest,
					fallbackDest: in.Dest,
					label:        imgLabel,
				}
			}
		}
		if entry != nil {
			e, ok := resolve(session, entry.dest, entry.label, true)
			if !ok {
				return false
			}
			if !flushText() {
				return false
			}
			resolved++
			runs = append(runs, embedRun{entry: entry, embed: e})
			at = max(in.Next, at+1)
			continue
		}
		if in.Kind == parser.SoftBreak || in.Kind == parser.LineBreak || in.Kind == parser.Text && strings.TrimSpace(in.Text) == "" {
			at = max(in.Next, at+1)
			continue
		}
		if in.Start < 0 || in.End < in.Start || in.End > len(src) {
			return false
		}
		if textStart < 0 {
			textStart = in.Start
		}
		if in.End > textEnd {
			textEnd = in.End
		}
		at = max(in.Next, at+1)
	}
	if resolved == 0 || !flushText() {
		return false
	}
	s.embedMD.Theme, s.embedMD.Extensions, s.embedMD.CodeBlock = s.md.Theme, s.md.Extensions, s.md.CodeBlock
	s.embedMD.OnLink, s.embedMD.LinkMenu, s.embedMD.Preview = s.md.OnLink, s.md.LinkMenu, s.md.Preview
	s.embedMD.LinkIcon, s.embedMD.Resolve = s.md.LinkIcon, s.md.Resolve
	s.embedMD.Code, s.embedMD.WrapCode, s.embedMD.Math, s.embedMD.InlineMath = s.md.Code, s.md.WrapCode, s.md.Math, s.md.InlineMath
	s.embedMD.Block = nil
	s.embedMD.ShareSelection = true
	ui.Column(c.Key("embed")).FillWidth().Gap(8).Children(func() {
		var entries []embedEntry
		var embeds []Embed
		group := 0
		flushEmbeds := func() {
			if len(entries) > 0 {
				s.renderEmbedEntries(c, session, group, entries, embeds, actions)
				entries = nil
				embeds = nil
				group++
			}
		}
		for idx, run := range runs {
			if run.entry != nil {
				entries = append(entries, *run.entry)
				embeds = append(embeds, run.embed)
				continue
			}
			flushEmbeds()
			kit.Markdown(c.Key(fmt.Sprintf("embed-text-%d", idx)), run.text, kit.MarkdownProps{Variant: kit.Chat, Options: &s.embedMD})
		}
		flushEmbeds()
	})
	return true
}

func (s *State) renderEmbedEntries(c *ui.Context, session string, group int, entries []embedEntry, embeds []Embed, actions Actions) {
	p := theme.Of(c)
	var pictures []Image
	flush := func(n int) {
		if len(pictures) > 0 {
			// The place too: a paragraph may embed a target twice.
			s.thumbnails(c, fmt.Sprintf("embed-images-%d-%d-%s", group, n, pictures[0].ID), pictures, false)
			pictures = nil
		}
	}
	for n, e := range embeds {
		if e.Image != nil {
			pictures = append(pictures, *e.Image)
			if detail := entries[n].fallbackDest; detail != "" && detail != entries[n].dest {
				// A linked picture shows itself and keeps the page it links to.
				flush(n + 1)
				s.embedLink(c, session, fmt.Sprintf("embed-detail-%d-%d", group, n), detail)
			}
			continue
		}
		flush(n)
		dest := entries[n].fallbackDest
		if dest == "" {
			dest = entries[n].dest
		}
		switch {
		case e.Missing:
			ui.Text(c.Key(fmt.Sprintf("embed-missing-%d-%d", group, n)), i18n.T(c, "chat_image_missing")).FontSize(textSize(c)).TextColor(p.MutedForeground)
		case e.File != nil:
			s.fileCard(c, fmt.Sprintf("embed-file-%d-%d-%s", group, n, e.File.ID), *e.File, actions)
		default:
			s.embedLink(c, session, fmt.Sprintf("embed-link-%d-%d", group, n), dest)
		}
	}
	flush(len(embeds))
}

func (s *State) embedLink(c *ui.Context, session, key, dest string) {
	p := theme.Of(c)
	ui.ButtonBase(c.Key(key)).AlignSelf(ui.Start).MaxWidthPercent(100).Cursor(ui.CursorPointer).Children(func() {
		ui.Text(c, dest).FontSize(textSize(c)).SingleLine().TextColor(p.Primary)
	}).OnClick(func() { s.follow(c, session, dest) })
}

// inlineRun keeps a text run cut from a paragraph one paragraph when it
// renders alone: a line that would start a heading, a thematic break, a list,
// a quote, a fence or HTML has its marker escaped. It reports false when the
// run still cannot stay a paragraph.
func inlineRun(text string, ext parser.Extensions) (string, bool) {
	paragraph := func(src string) bool {
		d := parser.New(ext)
		d.Set(src)
		n, kind := 0, parser.Root
		for child := range d.Children(0) {
			n++
			kind = d.Block(child).Kind
		}
		return n == 1 && kind == parser.Paragraph
	}
	lines := strings.Split(text, "\n")
	for n, line := range lines {
		line = strings.TrimLeft(line, " \t")
		if line != "" && (!paragraph(line) || n > 0 && strings.Trim(line, "= \t") == "") {
			at := 0
			for at < len(line) && line[at] >= '0' && line[at] <= '9' {
				at++
			}
			if at == 0 || at == len(line) || line[at] != '.' && line[at] != ')' {
				at = 0
			}
			if r := line[at]; r > '~' || r < '!' || r >= '0' && r <= '9' || r >= 'A' && r <= 'Z' || r >= 'a' && r <= 'z' {
				return "", false
			}
			line = line[:at] + `\` + line[at:]
		}
		lines[n] = line
	}
	text = strings.Join(lines, "\n")
	return text, paragraph(text)
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
