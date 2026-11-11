package kit

import (
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// RingSize is the progress ring's diameter.
const RingSize = 15

// Ring draws a progress ring 15 across in a tone's color: a track at 20% of
// it and, over it, the arc of percent (0 to 100) clockwise from the top, or
// while loading an arc that sweeps round once a second (a still quarter
// under Reduce Motion). It is a picture, not a control, named label. Custom
// drawing: pinned MyGo has a progress bar and a spinner of spokes, no
// progress circle.
func Ring(c *ui.Context, percent float64, loading bool, tone Tone, label string) ui.Element {
	const stroke = RingSize * 0.15
	color := tone.color(theme.Of(c))
	still := c.Preferences().ReduceMotion
	return ui.Box(c).Size(RingSize, RingSize).Shrink(0).Label(label).Draw(func(pt *ui.Painter, r ui.Rect) {
		cx, cy, rad := r.X+r.W/2, r.Y+r.H/2, (min(r.W, r.H)-stroke)/2
		var track ui.Path
		track.Circle(cx, cy, rad)
		pt.StrokePath(&track, stroke, color.Alpha(0.2))
		from, to := 0.0, min(max(percent, 0), 100)/100
		if loading {
			// The end eases round in a second, the start follows through
			// its second half.
			from, to = 0, 0.25
			if !still {
				t := float64(pt.Now().UnixMilli()%1000) / 1000
				from, to = ringEase(min(max((t-0.5)/0.5, 0), 1)), ringEase(t)
				pt.AnimationFrameRate(FPS)
			}
		}
		if to <= from {
			return
		}
		var arc ui.Path
		steps := max(2, int(math.Ceil(64*(to-from))))
		for i := 0; i <= steps; i++ {
			a := 2*math.Pi*(from+(to-from)*float64(i)/float64(steps)) - math.Pi/2
			x, y := cx+rad*float32(math.Cos(a)), cy+rad*float32(math.Sin(a))
			if i == 0 {
				arc.MoveTo(x, y)
			} else {
				arc.LineTo(x, y)
			}
		}
		pt.StrokePath(&arc, stroke, color)
	})
}

// ringEase is the quadratic ease in and out of the loading sweep.
func ringEase(t float64) float64 {
	if t < 0.5 {
		return 2 * t * t
	}
	return 1 - 2*(1-t)*(1-t)
}
