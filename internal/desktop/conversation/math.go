package conversation

import (
	_ "embed"
	"log/slog"
	"sync"

	"github.com/caelyreth/untie"
	untiemygo "github.com/caelyreth/untie/contrib/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"
)

// Math in replies is typeset natively by Untie (no WebView).

// mathFont is Latin Modern Math 1.959, the OpenType MATH font formulas are
// set in, under the GUST Font License (LICENSE-LATIN-MODERN-MATH). Untie
// rejects STIX Two Math 2.13: it reads that font's MathKern table as truncated.
//
//go:embed math.otf
var mathFont []byte

// mathCache is how many laid out formulas Untie keeps; a formula being
// streamed adds one for each partial source that reads.
const mathCache = 512

// mathRenderer is the one renderer of the process, made on first use: its layouts
// carry no color, so light and dark and every conversation share them.
var mathRenderer = sync.OnceValue(func() *untiemygo.Renderer {
	fonts, err := untie.LoadMathFont(mathFont)
	if err != nil {
		slog.Error("math font unreadable", "error", err)
		return nil
	}
	r, err := untiemygo.New(fonts, untiemygo.Options{CacheEntries: mathCache})
	if err != nil {
		slog.Error("math renderer unavailable", "error", err)
		return nil
	}
	return r
})

// Register the Markdown syntax without loading the math font until a formula
// actually enters the layout. Most coding transcripts never need the font.
func bindMath(o *markdown.Options) {
	if o.Math != nil {
		return
	}
	if o.Extensions == 0 {
		o.Extensions = parser.Default
	}
	o.Extensions |= parser.ExtMath | parser.ExtDollarMath
	o.Math = func(c *ui.Context, source string) {
		if renderer := mathRenderer(); renderer != nil {
			renderer.DisplayMath(c, source)
		} else {
			ui.Text(c, source)
		}
	}
	o.InlineMath = func(source string, size float32) (markdown.InlineObject, bool) {
		return mathRenderer().InlineMath(source, size)
	}
}
