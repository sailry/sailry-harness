package conversation

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/firede/limu/limumygo"
	"github.com/firede/limu/style"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Visual blocks inside a reply's Markdown (markdown/visuals): a fenced
// ```mermaid block is drawn in place by limu, with no WebView, and math is
// typeset by bindMath when the build has a typesetter.

// maxDiagramSource is the largest Mermaid source drawn (visuals
// MAX_SOURCE_BYTES); a longer one shows as code.
const maxDiagramSource = 64 * 1024

// mermaidBlocks are the Markdown code hooks of the replies, by language.
var mermaidBlocks = map[string]func(*ui.Context, *markdown.CodeBlockView){"mermaid": mermaidBlock}

// bindVisuals has the replies' Markdown draw its Mermaid blocks and math.
func (s *State) bindVisuals() {
	if s.md.Code == nil {
		s.md.Code = mermaidBlocks
	}
	bindMath(&s.md)
}

// mermaidBlock is a ```mermaid block of a reply: the diagram at its own
// size, centered, scrolling sideways when it is wider than the reply, laid
// out again only when the reply's width changes and painted in the
// palette, so a theme switch lays nothing out. A block still being
// written, an empty or oversized one, and a source limu cannot read show
// as the code block they are.
func mermaidBlock(c *ui.Context, b *markdown.CodeBlockView) {
	src := b.Code()
	if b.Open || len(src) > maxDiagramSource || strings.TrimSpace(src) == "" {
		kit.CodeBlock(c, b)
		return
	}
	box := ui.Column(c.Key("mermaid")).AlignSelf(ui.Stretch).MinWidth(0)
	sc, err := diagram(src, box.LayoutBounds().W)
	if err != nil {
		box.Children(func() { kit.CodeBlock(c, b) })
		return
	}
	look := &limumygo.Options{Look: style.Look{Theme: diagramTheme(theme.Of(c)), ThemeOnly: true}, Fonts: limumygo.DefaultFonts}
	box.Children(func() {
		ui.ScrollHorizontal(c.Key("diagram")).AlignSelf(ui.Stretch).Children(func() {
			w := limumygo.View(c, sc, look)
			if sc.Title == "" {
				w.Label(sc.Type)
			}
			w.Element().Margin(0, ui.Auto).Shrink(0)
		})
	})
}
