package kit

import (
	"math"
	"time"

	"github.com/egoist/mygo/ui"
)

// Two of loading.dev's indicators that MyGo's plugins/loading does not
// have at the pinned version: Loop and Chevrons. They follow loading-dev's
// shapes, keyframes and timings (https://github.com/jakubkrehel/loading,
// MIT; its notice is in third_party_licenses), as that package's marks do,
// and behave as they do: a progress indicator of unknown length that moves
// as long as it shows, stands still in loading.dev's pose where the desktop
// asks for less motion, and asks for frames that only paint.

// markBox is the side of the viewBox loading-dev draws its marks in.
const markBox = 24

// markStroke is the width of their strokes in that viewBox.
const markStroke = 2.5

// markTimer is how long a mark has run, and the path it draws with, its
// storage kept from frame to frame.
type markTimer struct {
	start time.Time
	path  ui.Path
}

type markKey struct{}

// mark creates a mark size across whose draw paints it cycles into its
// motion, at rest when still.
func mark(c *ui.Context, size float32, cycle time.Duration, draw func(p *ui.Painter, r ui.Rect, path *ui.Path, cycles float64, still bool)) ui.Element {
	still := c.Preferences().ReduceMotion
	e := ui.Box(c).Size(size, size).Shrink(0).Role(ui.RoleProgress).Range(0, 1, -1)
	now := c.Now()
	k := ui.Local(e, markKey{}, func() markTimer { return markTimer{start: now} })
	e.Draw(func(p *ui.Painter, r ui.Rect) {
		cycles := 0.0
		if !still {
			cycles = float64(p.Now().Sub(k.start)) / float64(cycle)
			p.AnimationFrame()
		}
		draw(p, r, &k.path, cycles, still)
	})
	return e
}

// loopCurves are the cubic curves of Loop's path, a figure of eight on its
// side, in the viewBox: from the middle up and round the left circle, back
// through the middle and round the right one. Each is two control points
// and an end; the half circles of radius 4 are two quarter arcs each.
var loopCurves = [8][6]float32{
	{10, 9.3, 8.4, 8, 6.5, 8},
	{4.291, 8, 2.5, 9.791, 2.5, 12},
	{2.5, 14.209, 4.291, 16, 6.5, 16},
	{8.4, 16, 10, 14.7, 12, 12},
	{14, 9.3, 15.6, 8, 17.5, 8},
	{19.709, 8, 21.5, 9.791, 21.5, 12},
	{21.5, 14.209, 19.709, 16, 17.5, 16},
	{15.6, 16, 14, 14.7, 12, 12},
}

// loopLength is the length of Loop's path in the viewBox.
var loopLength = func() float64 {
	const steps = 64
	x, y, length := 12.0, 12.0, 0.0
	for _, k := range loopCurves {
		x0, y0 := x, y
		for i := 1; i <= steps; i++ {
			t := float64(i) / steps
			u := 1 - t
			nx := u*u*u*x0 + 3*u*u*t*float64(k[0]) + 3*u*t*t*float64(k[2]) + t*t*t*float64(k[4])
			ny := u*u*u*y0 + 3*u*u*t*float64(k[1]) + 3*u*t*t*float64(k[3]) + t*t*t*float64(k[5])
			length += math.Hypot(nx-x, ny-y)
			x, y = nx, ny
		}
	}
	return length
}()

// loopDash is the share of the path Loop's dash covers.
const loopDash = 0.3

// Loop creates loading.dev's Loop in color: a dash going round a figure of
// eight on its side, over the faint figure, once in 1.6 s.
func Loop(c *ui.Context, size float32, color ui.Color) ui.Element {
	return mark(c, size, 1600*time.Millisecond, func(p *ui.Painter, r ui.Rect, path *ui.Path, cycles float64, _ bool) {
		k := min(r.W, r.H) / markBox
		x, y := r.X+(r.W-k*markBox)/2, r.Y+(r.H-k*markBox)/2
		figure := func() {
			path.Reset().MoveTo(x+12*k, y+12*k)
			for _, q := range loopCurves {
				path.CubeTo(x+q[0]*k, y+q[1]*k, x+q[2]*k, y+q[3]*k, x+q[4]*k, y+q[5]*k)
			}
			path.Close()
		}
		figure()
		p.StrokePath(path, markStroke*k, color.Alpha(0.2))
		// The dash goes forward along the path: SVG's stroke-dashoffset
		// falling by the path's length a cycle, here the offset that many
		// lengths short of one.
		length := float32(loopLength) * k
		at := float32(cycles - math.Floor(cycles))
		figure()
		path.Dash((1-at)*length, loopDash*length, (1-loopDash)*length)
		p.StrokePath(path, markStroke*k, color)
	})
}

// chevronStarts are where Chevrons's three chevrons start, across the
// viewBox.
var chevronStarts = [3]float32{3.5, 9, 14.5}

// Chevrons creates loading.dev's Chevrons in color: three chevrons pointing
// right that light in turn and fade to a fifth, once a second; at rest all
// three are at 0.6.
func Chevrons(c *ui.Context, size float32, color ui.Color) ui.Element {
	return mark(c, size, time.Second, func(p *ui.Painter, r ui.Rect, path *ui.Path, cycles float64, still bool) {
		k := min(r.W, r.H) / markBox
		x, y := r.X+(r.W-k*markBox)/2, r.Y+(r.H-k*markBox)/2
		for i, start := range chevronStarts {
			alpha := 0.6
			if !still {
				// Each a third of a cycle behind the one before, the first
				// a whole cycle ahead, as a negative animation-delay.
				t := cycles + float64(len(chevronStarts)-i)/float64(len(chevronStarts))
				alpha = 1 - 0.8*(t-math.Floor(t))
			}
			path.Reset().MoveTo(x+start*k, y+6.5*k).LineTo(x+(start+5.5)*k, y+12*k).LineTo(x+start*k, y+17.5*k)
			p.StrokePath(path, markStroke*k, color.Alpha(float32(alpha)))
		}
	})
}
