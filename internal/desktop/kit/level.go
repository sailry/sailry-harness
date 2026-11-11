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
// height. A RichText whose line has the theme's size as its strut takes
// that size, whatever its spans'.
func Level(c *ui.Context, e ui.Element, size float32) ui.Element {
	m := metricsOf(c.Theme().Font, size)
	dy := capBaseline(size) - (m.Ascent-m.Descent)/2
	return e.Margin(dy, 0, -dy, 0)
}

// LevelTop is Level for a text that may wrap, in a row aligned to its top
// whose icons are height/2 from it: the first line is where a centred row
// of height has its line, and a text of one line makes the row that high.
func LevelTop(c *ui.Context, e ui.Element, size, height float32) ui.Element {
	m := metricsOf(c.Theme().Font, size)
	top := height/2 + capBaseline(size) - m.Ascent
	return e.Margin(top, 0, max(height-top-m.Ascent-m.Descent, 0), 0)
}
