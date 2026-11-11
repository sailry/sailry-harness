package crychic

import (
	"context"
	"encoding/xml"
	"strconv"
	"strings"
	"sync"

	pele "github.com/Broderick-Westrope/pele-go"
	"github.com/ELY-Open-Source/CrychicUI/diagram"
	"github.com/egoist/mygo/ui"
)

// MermaidPicture is what the library draws of a Mermaid source: its size
// before it is fitted to a width, in the theme's font and text size; the
// kind of diagram; the title the source gives assistive technology
// (accTitle); and whether the picture draws a title of its own.
type MermaidPicture struct {
	Width, Height float32
	Kind, Title   string
	Titled        bool
}

// pictureKey is what a picture is made from, as the library's own inputs.
type pictureKey struct {
	source, font string
	size         float32
}

// pictureLimit bounds the sources kept: a long session starts over rather
// than keeping each.
const pictureLimit = 256

// pictures keeps what Pele made of each source, so a source is laid out
// once here however often its diagram is built, and one Pele cannot read
// is not read again every frame.
var pictures = struct {
	sync.Mutex
	m map[pictureKey]*MermaidPicture
}{m: map[pictureKey]*MermaidPicture{}}

// measurer measures labels as the library's does, in the fonts that draw
// them, so the size found here is the size of what it draws.
type measurer struct{ font string }

func (m measurer) Width(text string, size float64, st pele.Style) float64 {
	f := ui.Font{Family: m.font, Size: float32(size), Italic: st&pele.Italic != 0}
	if st&pele.Mono != 0 {
		f.Family = "monospace"
	}
	if st&(pele.Sub|pele.Sup) != 0 {
		f.Size /= 1.2
	}
	if st&pele.Bold != 0 {
		f.Weight = 700
	}
	w, _ := ui.ShapeText(text, f, 0).Size()
	return float64(w)
}

// MermaidOf is the picture diagram.Mermaid draws of source, and false for a
// source it would answer with its error box: one Pele cannot read, and one
// that names a custom property of Pele's own (reserved). The library gives
// no result to ask, so the source is rendered here first, with its options.
func MermaidOf(c *ui.Context, source string) (MermaidPicture, bool) {
	t := c.Theme()
	key := pictureKey{source, t.Font, t.Rem(1)}
	pictures.Lock()
	defer pictures.Unlock()
	p, seen := pictures.m[key]
	if !seen {
		p = picture(key)
		if len(pictures.m) >= pictureLimit {
			clear(pictures.m)
		}
		pictures.m[key] = p
	}
	if p == nil {
		return MermaidPicture{}, false
	}
	return *p, true
}

// picture renders a source as the library does; nil is one it cannot draw.
func picture(key pictureKey) *MermaidPicture {
	if reserved(key.source) {
		return nil
	}
	off := false
	res, err := pele.Render(context.Background(), key.source, pele.Options{
		Measurer: measurer{key.font}, FontSize: pele.Ptr(float64(key.size)), Links: &off, Images: &off})
	if err != nil {
		return nil
	}
	w, h, title := drawing(res.SVG)
	if w <= 0 || h <= 0 {
		return nil
	}
	return &MermaidPicture{Width: w, Height: h, Kind: string(res.Type), Title: title,
		Titled: strings.Contains(res.SVG, `class="pele-title"`)}
}

// reserved reports a source that reads a custom property in Pele's own
// namespaces through var(). Pele passes a style's value on as written, and
// the library fails the whole drawing on a name there it does not know.
func reserved(source string) bool {
	s := strings.ToLower(source)
	for {
		at := strings.Index(s, "var(")
		if at < 0 {
			return false
		}
		s = strings.TrimLeft(s[at+4:], " \t\r\n\f")
		if strings.HasPrefix(s, "--_") || strings.HasPrefix(s, "--pele-") {
			return true
		}
	}
}

// drawing reads what the library takes from the root of Pele's drawing:
// the size it draws at, which is the root's and may be rounded from the
// result's, and the title for assistive technology, the root's first child.
func drawing(svg string) (w, h float32, title string) {
	dec := xml.NewDecoder(strings.NewReader(svg))
	token, _ := dec.Token()
	root, ok := token.(xml.StartElement)
	if !ok {
		return 0, 0, ""
	}
	for _, a := range root.Attr {
		v, err := strconv.ParseFloat(strings.TrimSuffix(strings.TrimSpace(a.Value), "px"), 32)
		if err != nil {
			continue
		}
		switch a.Name.Local {
		case "width":
			w = float32(v)
		case "height":
			h = float32(v)
		}
	}
	if token, _ = dec.Token(); token == nil {
		return w, h, ""
	} else if first, ok := token.(xml.StartElement); !ok || first.Name.Local != "title" {
		return w, h, ""
	}
	token, _ = dec.Token()
	chars, _ := token.(xml.CharData)
	return w, h, strings.TrimSpace(string(chars))
}

// Mermaid is diagram.Mermaid for a source a model wrote. It builds nothing
// for a source the library would answer with its error box (MermaidOf), so
// the caller shows the source its own way. A diagram without a label is
// named by its accTitle, else by its kind, never by the library's English.
// The library's arrival is held still: a virtual list would replay it each
// time the diagram scrolls back in.
func Mermaid(c *ui.Context, source string, o diagram.MermaidOptions) (ui.Element, bool) {
	p, ok := MermaidOf(c, source)
	if !ok {
		return ui.Element{}, false
	}
	if o.Label == "" {
		o.Label = p.Title
	}
	if o.Label == "" {
		o.Label = p.Kind
	}
	return diagram.Mermaid(c, source, o).Translate(0, 0).Scale(1, 1).Opacity(1), true
}
