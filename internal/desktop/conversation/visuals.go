package conversation

import (
	"strings"
	"sync"

	"github.com/ELY-Open-Source/CrychicUI/diagram"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	limudiagram "github.com/firede/limu/diagram"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/kit/crychic"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Visuals of a reply: a Mermaid source, in a fenced block of the text or
// as a VisualBlock, drawn in place with no card around it. A data chart
// is drawn natively (chart.go); any other diagram is CrychicUI's picture
// (diagram.Mermaid, on pele-go), scaled down to the reply. A source it
// cannot draw stays a code block.

const (
	// maxDiagramSource is the longest source drawn as a diagram: a longer
	// one stays a code block, as layout runs on the UI goroutine. Pele's
	// own limit is 50,000 characters.
	maxDiagramSource = 64 * 1024
	// diagramHeight is the tallest a diagram is and diagramScale the
	// smallest it is scaled to fit the reply: one still larger scrolls.
	diagramHeight = 420
	diagramScale  = 0.7
	// visualGap is between a visual and its caption.
	visualGap = 6
)

// mermaidBlocks draws the Markdown code blocks tagged mermaid.
var mermaidBlocks = map[string]func(*ui.Context, *markdown.CodeBlockView){"mermaid": mermaidBlock}

// bindVisuals makes the transcript's Markdown draw diagrams and math.
func (s *State) bindVisuals() {
	if s.md.Code == nil {
		s.md.Code = mermaidBlocks
	}
	bindMath(&s.md)
}

// mermaidBlock is a mermaid code block: the code while the reply is still
// writing it, then its chart or diagram, and the code again when it
// cannot be drawn.
func mermaidBlock(c *ui.Context, b *markdown.CodeBlockView) {
	src := b.Code()
	if b.Open || len(src) > maxDiagramSource || strings.TrimSpace(src) == "" {
		kit.CodeBlock(c, b)
		return
	}
	box := ui.Column(c.Key("mermaid")).AlignSelf(ui.Stretch).MinWidth(0)
	if !showVisual(c, box, src, "") {
		box.Children(func() { kit.CodeBlock(c, b) })
	}
}

// showVisual fills box, as wide as the reply, with the chart or diagram
// of a Mermaid source, and reports whether it could. title is the block's
// own, shown as one small muted line above; a chart without one shows the
// source's title there instead, and a diagram that draws its title shows
// only that. The visual is named for assistive technology by the title
// shown, else by its kind.
func showVisual(c *ui.Context, box ui.Element, src, title string) bool {
	if len(src) > maxDiagramSource || strings.TrimSpace(src) == "" {
		return false
	}
	width := box.LayoutBounds().W
	if v := visualOf(src); v.chart != nil {
		d := v.chart
		if title == "" {
			title = clip(d.title, labelRunes)
		}
		name := title
		if name == "" {
			name = v.kind
		}
		box.Gap(visualGap).Children(func() {
			caption(c, title)
			d.build(c, name, width)
		})
		return true
	}
	pic, ok := crychic.MermaidOf(c, src)
	if !ok {
		return false
	}
	// As large as it is drawn, down to what fits the reply's width and
	// diagramHeight; past diagramScale it scrolls instead.
	k := float32(1)
	if width > 0 && pic.Width > width {
		k = width / pic.Width
	}
	if pic.Height*k > diagramHeight {
		k = diagramHeight / pic.Height
	}
	scrolls := k < diagramScale
	if scrolls {
		k = diagramScale
	}
	w := pic.Width * k
	if k < 1 {
		// Whole DIPs, so that rounding never takes the height past the cap.
		w = float32(int(w))
	}
	name := title
	if name == "" {
		name = pic.Title
	}
	if name == "" {
		name = pic.Kind
	}
	// The picture is drawn in the theme it is built under: the palette's
	// inset fill in its nodes, so they stand off the page in a light theme
	// too, its second tone and its outline, with half the corner.
	p := theme.Of(c)
	look := *c.Theme()
	look.Surface, look.SurfaceHover, look.Border, look.Radius = p.Muted, p.Secondary, p.Border, p.Radius/2
	box.Gap(visualGap).Children(func() {
		if !pic.Titled {
			caption(c, title)
		}
		// The picture fills the width it is given, at its own proportions.
		view := func() {
			ui.Themed(c.Key("picture"), &look).Background(ui.Transparent).Width(w).Shrink(0).AlignSelf(ui.Start).Children(func() {
				crychic.Mermaid(c, src, diagram.MermaidOptions{Label: name, Failed: i18n.T(c, "artifact_preview_failed")})
			})
		}
		if !scrolls {
			view()
			return
		}
		ui.ScrollBoth(c.Key("diagram")).FillWidth().Height(min(pic.Height*k, diagramHeight)).Children(view)
	})
	return true
}

// caption is the one small muted line above a visual.
func caption(c *ui.Context, text string) {
	if text != "" {
		ui.Text(c.Key("caption"), text).FontSize(textSize(c) - 2).TextColor(theme.Of(c).MutedForeground)
	}
}

// parsedVisual is what a Mermaid source is to the painters: its kind, and
// its chart when it is a data chart they draw.
type parsedVisual struct {
	kind  string
	chart *dataChart
}

// visualLimit bounds the sources kept: a long session starts over rather
// than keeping each.
const visualLimit = 256

// visuals keeps the sources read, so a chart's data is not parsed every
// frame.
var visuals = struct {
	sync.Mutex
	m map[string]*parsedVisual
}{m: map[string]*parsedVisual{}}

// visualOf is src read, once: limu reads its front matter and kind, and
// the data of an xychart or pie. Every other kind is left to the picture.
func visualOf(src string) *parsedVisual {
	visuals.Lock()
	defer visuals.Unlock()
	if v, ok := visuals.m[src]; ok {
		return v
	}
	v := &parsedVisual{}
	if p, err := limudiagram.Prepare(src, limudiagram.Options{}); err == nil {
		v.kind, v.chart = string(p.Type), chartOf(p)
	}
	if len(visuals.m) >= visualLimit {
		clear(visuals.m)
	}
	visuals.m[src] = v
	return v
}
