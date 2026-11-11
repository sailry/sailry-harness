package conversation

import (
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/firede/limu"
	limudiagram "github.com/firede/limu/diagram"
	"github.com/firede/limu/layout"
	"github.com/firede/limu/limumygo"
	"github.com/firede/limu/scene"
	"github.com/firede/limu/style"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Visuals of a reply: a Mermaid source, in a fenced block of the text or
// as a VisualBlock, drawn in place with no card around it. A data chart
// is drawn natively (chart.go); any other diagram is limu's picture,
// scaled down to the reply. A source limu cannot read stays a code block.

const (
	// maxDiagramSource is the longest source drawn as a diagram: a longer
	// one stays a code block, as layout runs on the UI goroutine.
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
	v := visualOf(src)
	if v.err != nil {
		return false
	}
	width := box.LayoutBounds().W
	if d := v.chart; d != nil {
		if title == "" {
			title = clip(d.title, labelRunes)
		}
		name := title
		if name == "" {
			name = string(v.d.Type())
		}
		box.Gap(visualGap).Children(func() {
			caption(c, title)
			d.build(c, name, width)
		})
		return true
	}
	sc, err := v.scene(width)
	if err != nil {
		return false
	}
	// As large as it is laid out, down to what fits the reply's width
	// and diagramHeight; past diagramScale it scrolls instead.
	k := 1.0
	if width > 0 && sc.Width > float64(width) {
		k = float64(width) / sc.Width
	}
	if sc.Height*k > diagramHeight {
		k = diagramHeight / sc.Height
	}
	scrolls := k < diagramScale
	if scrolls {
		k = diagramScale
	}
	name := title
	if name == "" {
		name = sc.Title
	}
	if name == "" {
		name = sc.Type
	}
	look := &limumygo.Options{Look: style.Look{Theme: diagramTheme(theme.Of(c)), ThemeOnly: true}, Fonts: limumygo.DefaultFonts, Scale: k}
	box.Gap(visualGap).Children(func() {
		if !titled(sc) {
			caption(c, title)
		}
		view := func() { limumygo.View(c, sc, look).Label(name).Element().AlignSelf(ui.Start).Shrink(0) }
		if !scrolls {
			view()
			return
		}
		ui.ScrollBoth(c.Key("diagram")).FillWidth().Height(float32(min(sc.Height*k, diagramHeight))).Children(view)
	})
	return true
}

// caption is the one small muted line above a visual.
func caption(c *ui.Context, text string) {
	if text != "" {
		ui.Text(c.Key("caption"), text).FontSize(textSize(c) - 2).TextColor(theme.Of(c).MutedForeground)
	}
}

// titled reports whether a scene draws its own title.
func titled(sc *scene.Scene) bool {
	for i := range sc.Items {
		if it := &sc.Items[i]; it.Kind == scene.Text && string(it.Role) == sc.Type+".title" {
			return true
		}
	}
	return false
}

// laidOut is what limu made of a source at a width: its scene, or why it
// could not lay it out.
type laidOut struct {
	sc  *scene.Scene
	err error
}

// parsedVisual is what a Mermaid source is: the diagram limu read, or why
// it could not; its chart when it is one the painters draw; and the
// scenes laid out so far, by width.
type parsedVisual struct {
	d      *limu.Diagram
	err    error
	chart  *dataChart
	scenes map[int]laidOut
}

// visualLimit bounds the sources kept and widthLimit the layouts of one:
// a long session, or a window resized across many widths, starts over
// rather than keeping each.
const (
	visualLimit = 256
	widthLimit  = 32
)

// visuals keeps the sources parsed and their scenes laid out, as layout
// measures every label, and the sources limu could not read, so they are
// not parsed every frame.
var visuals = struct {
	sync.Mutex
	m map[string]*parsedVisual
}{m: map[string]*parsedVisual{}}

// visualOf is src parsed, once.
func visualOf(src string) *parsedVisual {
	visuals.Lock()
	defer visuals.Unlock()
	if v, ok := visuals.m[src]; ok {
		return v
	}
	v := &parsedVisual{scenes: map[int]laidOut{}}
	if v.d, v.err = limu.Parse(src, limudiagram.Options{}); v.err == nil {
		v.chart = chartOf(v.d)
	}
	if len(visuals.m) >= visualLimit {
		clear(visuals.m)
	}
	visuals.m[src] = v
	return v
}

// scene is the diagram laid out for a width (a frame's whole DIPs; none
// before the first layout), in the app's fonts and 14 DIP text.
func (v *parsedVisual) scene(width float32) (*scene.Scene, error) {
	k := max(int(width), 0)
	visuals.Lock()
	defer visuals.Unlock()
	if d, ok := v.scenes[k]; ok {
		return d.sc, d.err
	}
	sc, err := v.d.Layout(&layout.Options{Measurer: limumygo.DefaultFonts, FontSize: 14, MaxWidth: float64(k), Snap: 0.5})
	if len(v.scenes) >= widthLimit {
		clear(v.scenes)
	}
	v.scenes[k] = laidOut{sc, err}
	return sc, err
}

// diagramTheme is limu's theme in the palette's colors: no fill behind
// the diagram, the text, lines and outlines of the transcript, and the
// series in the primary and the theme's accents.
func diagramTheme(p *theme.Palette) *style.Theme {
	col := func(c ui.Color) style.Color { return style.Color{R: c.R, G: c.G, B: c.B, A: c.A} }
	return &style.Theme{
		Bg: col(p.Surface.Alpha(0)), Fg: col(p.Foreground), Muted: col(p.MutedForeground), Line: col(p.MutedForeground),
		Surface: col(p.Muted), SurfaceAlt: col(p.Secondary), Border: col(p.Border), Accent: col(p.Primary), Critical: col(p.Danger),
		Series:      [8]style.Color{col(p.Primary), col(p.Orange), col(p.Success), col(p.Danger), col(p.Blue), col(p.Green), col(p.Yellow), col(p.Magenta)},
		Radius:      float64(p.Radius) / 2,
		StrokeWidth: 1,
	}
}
