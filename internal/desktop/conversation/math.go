package conversation

import (
	_ "embed"
	"log/slog"
	"sync"

	"github.com/caelyreth/untie"
	untiemygo "github.com/caelyreth/untie/contrib/mygo"
	"github.com/egoist/mygo/ui/markdown"
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

// bindMath has o typeset $…$, $$…$$, \(…\) and \[…\]. Formulas take the
// text size and the theme's text color at paint time, so a theme switch
// lays nothing out. A formula still being written is the Markdown
// stream's: it closes the open formula and each partial source is typeset
// as it stands; one Untie cannot read shows its source. With no usable
// font nothing is bound and formulas keep their source text.
func bindMath(o *markdown.Options) {
	if o.Math != nil {
		return
	}
	if r := mathRenderer(); r != nil {
		r.BindMarkdown(o)
	}
}
