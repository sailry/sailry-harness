package kit

import (
	"maps"
	"strings"

	"github.com/ELY-Open-Source/CrychicUI/diagram"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit/crychic/mermaid"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

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

// MermaidChart optionally draws a surface's own chart for a Mermaid source.
// Returning false leaves the source to the standard diagram renderer.
type MermaidChart func(c *ui.Context, box ui.Element, source, title string) bool

// BindMermaid adds the fence renderer to a Kit Markdown view's retained options
// without replacing a caller's language handlers or modifying a shared map.
func BindMermaid(o *markdown.Options, chart MermaidChart) {
	if o.Code["mermaid"] != nil {
		return
	}
	code := maps.Clone(o.Code)
	if code == nil {
		code = make(map[string]func(*ui.Context, *markdown.CodeBlockView))
	}
	code["mermaid"] = func(c *ui.Context, b *markdown.CodeBlockView) {
		if b.Open {
			o.CodeBlock(c, b)
			return
		}
		box := ui.Column(c.Key("mermaid")).AlignSelf(ui.Stretch).MinWidth(0)
		if !showMermaid(c, box, b.Code(), "", o.Theme.FontSize, chart) {
			box.Children(func() { o.CodeBlock(c, b) })
		}
	}
	o.Code = code
}

// Mermaid shows a standalone visual block, or its original source when it cannot be drawn.
func Mermaid(c *ui.Context, box ui.Element, src, title string, size float32, chart MermaidChart) {
	if !showMermaid(c, box, src, title, size, chart) {
		box.Gap(visualGap).Children(func() {
			caption(c, title, size)
			Code(c, ui.Column(c.Key("source")), src, CodeOptions{Tint: true})
		})
	}
}

func showMermaid(c *ui.Context, box ui.Element, src, title string, size float32, chart MermaidChart) bool {
	if len(src) > maxDiagramSource || strings.TrimSpace(src) == "" {
		return false
	}
	if chart != nil && chart(c, box, src, title) {
		return true
	}
	pic, ok := mermaid.PictureOf(c, src)
	if !ok {
		return false
	}
	// As large as it is drawn, down to what fits the available width and
	// diagramHeight; past diagramScale it scrolls instead.
	k := float32(1)
	width := box.LayoutBounds().W
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
			caption(c, title, size)
		}
		// The picture fills the width it is given, at its own proportions.
		view := func() {
			ui.Themed(c.Key("picture"), &look).Background(ui.Transparent).Width(w).Shrink(0).AlignSelf(ui.Start).Children(func() {
				mermaid.Draw(c, src, diagram.MermaidOptions{Label: name, Failed: i18n.T(c, "artifact_preview_failed")})
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
func caption(c *ui.Context, text string, size float32) {
	if text != "" {
		ui.Text(c.Key("caption"), text).FontSize(size - 2).TextColor(theme.Of(c).MutedForeground)
	}
}
