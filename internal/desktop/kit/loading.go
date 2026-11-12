package kit

import (
	"github.com/egoist/mygo/plugins/loading"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The app's loading marks are MyGo's (plugins/loading, after loading.dev),
// one for each thing that goes on:
//
//   - Loader, loading.Swirl: work that runs and content that loads.
//   - Busy, loading.Ring: the operation a control started.
//   - Indicator: what a running turn does (thinking.go).
//
// Each is a picture with the progress role, in a color of the theme given
// here, as the marks do not take the color of the text around them. They
// paint again only while they show: a mark out of view, or not built, asks
// for no frame, and with Reduce Motion each holds a still pose and asks for
// none. They move at the display's rate, which MyGo's marks do not let cap
// (Painter.AnimationFrame, not AnimationFrameRate(FPS)); only they paint
// again, nothing is rebuilt.

// BusySize is the busy mark's size, and the most a mark beside 14 DIP text
// in a control is: the size of the icons there.
const BusySize = 14

// Loader is the app's loader, size across, in the muted color: a bright cell
// chasing its trail around a square. It is named as running work; Label
// renames it.
func Loader(c *ui.Context, size float32) ui.Element {
	return LoaderIn(c, size, theme.Of(c).MutedForeground)
}

// LoaderIn is Loader in color, the color of the row's text it marks.
func LoaderIn(c *ui.Context, size float32, color ui.Color) ui.Element {
	return loading.Swirl(c, loading.Options{Size: size, Color: color}).Label(i18n.T(c, "activity_running"))
}

// Busy is the mark of the operation a control started, in the icon's place
// or before the label, BusySize across in the muted color: an arc turning
// in a faint circle, as Ring's loading arc. The control it is in is busy
// and names the operation.
func Busy(c *ui.Context) ui.Element {
	return BusyIn(c, theme.Of(c).MutedForeground)
}

// BusySized is Busy size across: the mark of a step in progress in a list
// whose icons are that size.
func BusySized(c *ui.Context, size float32) ui.Element {
	return loading.Ring(c, loading.Options{Size: size, Color: theme.Of(c).MutedForeground})
}

// BusyIn is Busy in color, the color of the label it stands before on a
// filled button.
func BusyIn(c *ui.Context, color ui.Color) ui.Element {
	return loading.Ring(c, loading.Options{Size: BusySize, Color: color})
}
