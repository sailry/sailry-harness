package host

import (
	"math"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/chart"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The monitor's charts are MyGo's ui/chart: its Plot with the axes, and its
// Line, Area and Sector paths painted in the plot's Draw. Kit and the
// verified CrychicUI components have no chart (docs/crychic.md).

// tones are the monitor's series colours, from the theme's status and base
// colours (the reference's Tones).
type tones struct {
	cpu, memory, cache, swap  ui.Color
	download, upload          ui.Color
	write, read               ui.Color
	track, grid               ui.Color
	warning, danger, positive ui.Color
}

func tonesOf(p *theme.Palette) tones {
	return tones{
		cpu: p.Success, memory: p.Warning, cache: p.Warning.Alpha(0.5), swap: p.MutedForeground.Alpha(0.5),
		download: p.Blue, upload: p.Info, write: p.Warning, read: p.Success,
		track: p.MutedForeground.Alpha(0.16), grid: p.Border,
		warning: p.Warning, danger: p.Danger, positive: p.Success,
	}
}

// load is a load's colour: the series' own, the warning from 80%.
func (t tones) load(percent float64, own ui.Color) ui.Color {
	if percent >= 80 {
		return t.warning
	}
	return own
}

// capacity is a volume's colour by how full it is: the warning from 70%,
// the danger from 85%.
func (t tones) capacity(percent float64) ui.Color {
	switch {
	case percent >= 85:
		return t.danger
	case percent >= 70:
		return t.warning
	}
	return t.positive
}

// series is one line of a chart: its values, oldest first, NaN where a
// sample had none, and its colour.
type series struct {
	values []float64
	color  ui.Color
}

// fractions places n values evenly along a plot whose last value is at its
// right edge and which holds HistoryLimit of them: a short history starts
// part way along, as it fills.
func fractions(n int) []float32 {
	xs := make([]float32, n)
	for i := range xs {
		xs[i] = 1 - float32(n-1-i)/float32(HistoryLimit-1)
	}
	return xs
}

// heights maps values onto a plot from 0 to top; NaN stays NaN.
func heights(values []float64, top float64) []float32 {
	ys := make([]float32, len(values))
	for i, v := range values {
		if math.IsNaN(v) || top <= 0 {
			ys[i] = float32(math.NaN())
			continue
		}
		ys[i] = float32(min(max(v/top, 0), 1))
	}
	return ys
}

// areaChart is a plot of series over the history: grid lines at the left
// ticks, each series a line over its area at 12%, from 0 to top. label names
// it for assistive technology.
func areaChart(c *ui.Context, key any, height float32, top float64, left, bottom []chart.Tick, label string, all ...series) ui.Element {
	p := theme.Of(c)
	t := tonesOf(p)
	axes := chart.Axes{Left: left, Bottom: bottom, Label: func(_ chart.Side, tick chart.Tick) {
		ui.Text(c, tick.Label).FontSize(10).Font("monospace").NoWrap().TextColor(p.MutedForeground)
	}}
	return ui.Box(c.Key(key)).FillWidth().Height(height).Children(func() {
		chart.Plot(c, axes, func(area ui.Element) {
			area.Draw(func(pt *ui.Painter, r ui.Rect) {
				for _, tick := range left {
					y := chart.Y(r, tick.At)
					pt.Line(r.X, y, r.X+r.W, y, 1, t.grid)
				}
				if len(left) == 0 {
					pt.Line(r.X, r.Y+r.H, r.X+r.W, r.Y+r.H, 1, t.grid)
				}
				for _, s := range all {
					xs, ys := fractions(len(s.values)), heights(s.values, top)
					pt.FillPath(chart.Area(r, xs, nil, ys, chart.CurveMonotone), s.color.Alpha(0.12))
					pt.StrokePath(chart.Line(r, xs, ys, chart.CurveMonotone), 1.5, s.color)
				}
			})
		}).Fill().Gap(6).Label(label)
	})
}

// sparkline is a small line of values from 0 to top, without axes.
func sparkline(c *ui.Context, values []float64, top float64, color ui.Color, w, h float32) ui.Element {
	return ui.Box(c).Size(w, h).Shrink(0).Draw(func(pt *ui.Painter, r ui.Rect) {
		xs, ys := fractions(len(values)), heights(values, top)
		pt.FillPath(chart.Area(r, xs, nil, ys, chart.CurveLinear), color.Alpha(0.12))
		pt.StrokePath(chart.Line(r, xs, ys, chart.CurveLinear), 1, color)
	})
}

// arc is a part of a ring: its share of the circle from 0 to 1, and its
// colour.
type arc struct {
	share float64
	color ui.Color
}

// ring is a donut size across and width thick: the arcs in order from
// twelve o'clock over a track. center builds what stands in its middle, nil
// for nothing.
func ring(c *ui.Context, size, width float32, track ui.Color, label string, center func(), arcs ...arc) ui.Element {
	return ui.Box(c).Size(size, size).Shrink(0).Center().Label(label).Draw(func(pt *ui.Painter, r ui.Rect) {
		drawRing(pt, r, size/2, width, track, arcs)
	}).Children(func() {
		if center != nil {
			center()
		}
	})
}

func drawRing(pt *ui.Painter, r ui.Rect, outer, width float32, track ui.Color, arcs []arc) {
	cx, cy := r.X+r.W/2, r.Y+r.H/2
	pt.FillPath(chart.Sector(cx, cy, outer-width, outer, chart.Slice{Start: 0, End: 2 * math.Pi}), track)
	at := 0.0
	for _, a := range arcs {
		share := min(max(a.share, 0), 1-at)
		if share <= 0 || math.IsNaN(share) {
			continue
		}
		pt.FillPath(chart.Sector(cx, cy, outer-width, outer, chart.Slice{Start: 2 * math.Pi * at, End: 2 * math.Pi * (at + share)}), a.color)
		at += share
	}
}

// rings are concentric rings size across, each width thick and gap apart,
// outermost first: the load averages against the processors.
func rings(c *ui.Context, size, width, gap float32, track ui.Color, label string, arcs ...arc) ui.Element {
	return ui.Box(c).Size(size, size).Shrink(0).Label(label).Draw(func(pt *ui.Painter, r ui.Rect) {
		outer := size / 2
		for _, a := range arcs {
			drawRing(pt, r, outer, width, track, []arc{a})
			outer -= width + gap
		}
	})
}

// percentTicks are a utilization chart's left ticks: every quarter.
func percentTicks() []chart.Tick {
	scale := chart.NewLinear(0, 100)
	var ticks []chart.Tick
	for _, v := range []float64{0, 25, 50, 75, 100} {
		ticks = append(ticks, chart.Tick{At: scale.At(v), Value: v, Label: strconv.Itoa(int(v)) + "%"})
	}
	return ticks
}

// rateTicks are a transfer chart's left ticks: none, half and the top, as
// rates.
func rateTicks(top float64) []chart.Tick {
	var ticks []chart.Tick
	for _, f := range []float64{0, 0.5, 1} {
		ticks = append(ticks, chart.Tick{At: float32(f), Value: top * f, Label: Bytes(top*f) + "/s"})
	}
	return ticks
}

// clockTicks are a chart's bottom ticks: up to count of the history's
// times, evenly apart, the last at the right edge.
func clockTicks(history []Point, count int) []chart.Tick {
	if len(history) == 0 || count < 2 {
		return nil
	}
	xs := fractions(len(history))
	var ticks []chart.Tick
	last := -1
	for i := range count {
		at := (len(history) - 1) * i / (count - 1)
		if at == last {
			continue
		}
		last = at
		ticks = append(ticks, chart.Tick{At: xs[at], Label: Clock(history[at].At)})
	}
	return ticks
}

// ceiling is a transfer chart's top: the largest value, at least 1 KiB,
// rounded up to a power of two so the axis does not move with every
// sample.
func ceiling(all ...[]float64) float64 {
	top := 1024.0
	for _, values := range all {
		for _, v := range values {
			if !math.IsNaN(v) {
				top = max(top, v)
			}
		}
	}
	return math.Pow(2, math.Ceil(math.Log2(top)))
}
