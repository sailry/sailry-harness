package conversation

import (
	"sync"

	"github.com/egoist/mygo/ui"
	limudiagram "github.com/firede/limu/diagram"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// drawMermaidChart gives a reply's xychart and pie their interactive painter.
// Every other Mermaid source stays with the shared diagram renderer.
func drawMermaidChart(c *ui.Context, box ui.Element, src, title string) bool {
	v := visualOf(src)
	d := v.chart
	if d == nil {
		return false
	}
	if title == "" {
		title = clip(d.title, labelRunes)
	}
	name := title
	if name == "" {
		name = v.kind
	}
	width := box.LayoutBounds().W
	box.Gap(6).Children(func() {
		if title != "" {
			ui.Text(c.Key("caption"), title).FontSize(textSize(c) - 2).TextColor(theme.Of(c).MutedForeground)
		}
		d.build(c, name, width)
	})
	return true
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
