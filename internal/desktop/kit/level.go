package kit

import (
	"math"
	"sync"

	"github.com/egoist/mygo/ui"
)

// capHeight is the height of a UI font's capitals over its size. MyGo's
// font metrics give the ascent and the descent only; the capitals of the
// system fonts stand about 0.7 em, and the body of a CJK ideograph has its
// middle close to theirs.
const capHeight = 0.7

type fontKey struct {
	family string
	size   float32
}

var fontMetrics sync.Map // fontKey -> ui.FontMetrics

func metricsOf(family string, size float32) ui.FontMetrics {
	key := fontKey{family, size}
	if m, ok := fontMetrics.Load(key); ok {
		return m.(ui.FontMetrics)
	}
	m := ui.Font{Family: family, Size: size}.Metrics()
	fontMetrics.Store(key, m)
	return m
}

// capBaseline is how far below a row's middle the baseline of a text of
// size is for its capitals to stand in the middle: half their height, to a
// whole DIP, so that no platform's snapping of the baseline to a device
// pixel moves it at any whole scale.
func capBaseline(size float32) float32 {
	return float32(math.Round(float64(capHeight * size / 2)))
}

// Level puts a single line of text of size in the theme's font level with
// the icons of the row that centres it, and returns it. MyGo centres the
// text's line box, the ascent and the descent, whose middle is not the
// capitals' (the system font's ascent is taller than its capitals by more
// than the descent), and then snaps the baseline to a device pixel (down
// with Core Text): the capitals come out most of a DIP below the icons at
// scale 1 and half of one at scale 2. Level moves the box by the difference
// between that baseline and the capitals' (capBaseline), with a margin
// above and its opposite below, which keeps the box's size and the row's
// height. The line of a ui.RichText is not its text's: LevelSpan levels
// one.
func Level(c *ui.Context, e ui.Element, size float32) ui.Element {
	m := metricsOf(c.Theme().Font, size)
	return levelled(e, size, m.Ascent, m.Descent)
}

// LevelTop is Level for a text that may wrap, in a row aligned to its top
// whose icons are height/2 from it: the first line is where a centred row
// of height has its line, and a text of one line makes the row that high.
func LevelTop(c *ui.Context, e ui.Element, size, height float32) ui.Element {
	m := metricsOf(c.Theme().Font, size)
	return levelledTop(e, size, height, m.Ascent, m.Descent)
}

// LevelSpan is Level for a single-line ui.RichText of span in a row whose
// text is of size, as Sweep.Text takes its texts: the span's baseline is
// where Level puts that text's, so the two read as one line whatever the
// span's own font and size (a mono detail a DIP smaller). The line is not
// the span's: spanLine.
func LevelSpan(c *ui.Context, e ui.Element, span ui.Span, size float32) ui.Element {
	ascent, descent := spanLine(c, span)
	return levelled(e, size, ascent, descent)
}

// LevelSpanTop is LevelTop for a ui.RichText of span (LevelSpan).
func LevelSpanTop(c *ui.Context, e ui.Element, span ui.Span, size, height float32) ui.Element {
	ascent, descent := spanLine(c, span)
	return levelledTop(e, size, height, ascent, descent)
}

// spanLine is the ascent and the descent of the line of a ui.RichText of
// span. MyGo starts a RichText's line from the theme's font at the theme's
// size, its strut, and grows it to what a span needs: a span smaller than
// the theme's size stands on the theme's line, and a larger one, or one of
// a taller font, on its own. The element must take the theme's font size,
// as it does where no ancestor sets another.
func spanLine(c *ui.Context, span ui.Span) (ascent, descent float32) {
	t := c.Theme()
	family, size := span.Font, span.Size
	if family == "" {
		family = t.Font
	}
	if size == 0 {
		size = t.FontSize
	}
	strut, own := metricsOf(t.Font, t.FontSize), metricsOf(family, size)
	return max(strut.Ascent, own.Ascent), max(strut.Descent, own.Descent)
}

// levelled moves e, whose line has that ascent and descent, for the
// capitals of a text of size to stand in the middle of the row.
func levelled(e ui.Element, size, ascent, descent float32) ui.Element {
	dy := capBaseline(size) - (ascent-descent)/2
	return e.Margin(dy, 0, -dy, 0)
}

// levelledTop is levelled in a row of height aligned to its top.
func levelledTop(e ui.Element, size, height, ascent, descent float32) ui.Element {
	top := height/2 + capBaseline(size) - ascent
	return e.Margin(top, 0, max(height-top-ascent-descent, 0), 0)
}
