// Markdown is adapted from Lobe MyGo's markdown.Markdown
// (github.com/lobehub/lobe-mygo pkg/markdown/markdown.go at 5b3ecde):
//
// MIT License
//
// Copyright (c) 2026 LobeHub
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Changes: the caller's MyGo markdown.Options pass through whole, Kit
// filling only the Theme, Reveal and CodeBlock left unset; the variants'
// themes from the palette (Document: MyGo's DefaultTheme of the app
// theme; Chat: the reference's reply look, crates/content
// markdown/style.rs) rather than Lobe's type scale; code blocks as
// CodeBlock rather than Highlighter and Snippet; no wrapping column, the
// view being MyGo's own; text shown as it comes rather than paced (see
// streamReveal).

package kit

import (
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-go/internal/theme"
)

// MarkdownVariant picks the look Markdown gives a view whose Options
// leave Theme unset.
type MarkdownVariant int

const (
	// Document is for documents and details: MyGo's DefaultTheme of the
	// app theme.
	Document MarkdownVariant = iota
	// Chat is for replies: 14 DIP text on 24 DIP lines, emphasis in
	// primary, code blocks on a 3.5% foreground tint.
	Chat
)

// MarkdownProps are how Markdown shows its source.
type MarkdownProps struct {
	// Variant is the look when Options.Theme is unset.
	Variant MarkdownVariant
	// Animated tells a source still being written, as a reply streaming
	// in: what is half written shows as it will most likely end. Clear it
	// once the source is complete, in the same place.
	Animated bool
	// Options are MyGo's, every one of them the caller's. Markdown fills
	// Theme (the Variant's, following the app theme while it stays
	// Kit's), Reveal and CodeBlock (CodeBlock) in place when they are
	// unset, so pass the same Options each frame: they also keep the
	// view's images. nil uses Kit's shared Options of the Variant.
	Options *markdown.Options
}

// shared are the Options of views given none, by variant.
var shared [2]markdown.Options

// streamReveal shows streamed text as it comes: MyGo's paced reveal asks
// for frames at the display's rate (Context.AnimationFrame has no rate),
// past the FPS cap, so the source's own arrival paces it.
var streamReveal = markdown.Reveal{Unit: markdown.AtOnce, Fade: -1}

// Markdown shows src, MyGo's markdown.View, or markdown.Stream while
// Animated: CommonMark with MyGo's extensions, selectable and copying as
// text, HTML or Markdown, links opening as Options say.
func Markdown(c *ui.Context, src string, p MarkdownProps) ui.Element {
	o := p.Options
	if o == nil {
		o = &shared[p.Variant]
	}
	if o.Theme == nil || isMarkdownTheme(o.Theme) {
		o.Theme = MarkdownTheme(c, p.Variant)
	}
	if o.Reveal == nil {
		o.Reveal = &streamReveal
	}
	if o.CodeBlock == nil {
		o.CodeBlock = CodeBlock
	}
	if p.Animated {
		return markdown.Stream(c, src, o)
	}
	return markdown.View(c, src, o)
}

type markdownThemeKey struct {
	p *theme.Palette
	v MarkdownVariant
}

var (
	markdownThemes sync.Map // markdownThemeKey -> *markdown.Theme
	markdownOwn    sync.Map // *markdown.Theme -> struct{}: Kit's themes
)

func isMarkdownTheme(th *markdown.Theme) bool {
	_, ok := markdownOwn.Load(th)
	return ok
}

// MarkdownTheme is the look of a Variant in the app theme.
func MarkdownTheme(c *ui.Context, v MarkdownVariant) *markdown.Theme {
	p := theme.Of(c)
	key := markdownThemeKey{p, v}
	if th, ok := markdownThemes.Load(key); ok {
		return th.(*markdown.Theme)
	}
	th := markdown.DefaultTheme(c.Theme())
	if v == Chat {
		th = chatTheme(p)
	}
	markdownThemes.Store(key, th)
	markdownOwn.Store(th, struct{}{})
	return th
}

// chatTheme is the reply's look (crates/content markdown/style.rs): 14
// DIP text on 24 DIP lines, code blocks on a 3.5% foreground tint.
func chatTheme(p *theme.Palette) *markdown.Theme {
	return &markdown.Theme{
		FontSize:            14,
		LineHeight:          24.0 / 14,
		CodeSize:            13.0 / 14, // ems: Kit's 13 DIP mono size
		CodeLineHeight:      18 / 12.5,
		Text:                assistant(p),
		Muted:               p.MutedForeground,
		Link:                p.Link,
		Strong:              p.Primary, // answers emphasize in primary
		Code:                p.Foreground,
		CodeBackground:      p.Muted,
		CodeBlockBackground: p.Foreground.Alpha(0.035),
		CodeBlockBorder:     p.Border,
		CodeBlockRadius:     p.Radius,
		Mark:                p.Warning.Alpha(0.35),
		CodeBlockPadding:    12.0 / 14, // ems; the reference pads 12 x 10
		Rule:                p.Border,
		TableBorder:         p.Border,
		QuoteBar:            p.Border,
		QuoteText:           p.MutedForeground,
	}
}

// assistant is the reply's text color: the package's assistant message
// foreground, else the theme's.
func assistant(p *theme.Palette) ui.Color {
	if p.Variant != nil {
		return p.Variant.Messages["assistant"].Foreground.Or(p.Foreground)
	}
	return p.Foreground
}
