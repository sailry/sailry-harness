package host

import (
	"math"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/chart"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The monitor's charts are MyGo's ui/chart: its Plot with the axes, its
// Line, Area and Sector paths painted in the plot's Draw, and its RuleX and
// Point marks at the sample under the pointer. Kit has no chart, and
// CrychicUI's take no live series with gaps for missing samples (NaN panics).

// tones are the monitor's series colours, from the theme's status and chart
// colours (the reference's Tones). The upload is the chart violet, where
// the reference draws it in the download's blue.
type tones struct {
	cpu, gpu, memory, cache   ui.Color
	swap                      ui.Color
	download, upload          ui.Color
	write, read               ui.Color
	track, grid               ui.Color
	warning, danger, positive ui.Color
}

func tonesOf(p *theme.Palette) tones {
	return tones{
		cpu: p.Success, gpu: p.Chart[1], memory: p.Warning, cache: p.Warning.Alpha(0.5), swap: p.MutedForeground.Alpha(0.5),
		download: p.Chart[1], upload: p.Chart[3], write: p.Warning, read: p.Success,
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
// sample had none, its colour, and the name the chart's tip gives it.
type series struct {
	values []float64
	color  ui.Color
	name   string
}

// readout is what a chart's tip tells of the sample under the pointer: the
// samples the series were taken from, for their times, how a value is
// written, and the room the widest value takes, so the tip keeps its width
// as the values change.
type readout struct {
	history []Point
	text    func(v float64) string
	width   float32
}

// percents reads a utilization chart's values as its legend writes them:
// 12.5%.
func percents(history []Point) readout {
	return readout{history, func(v float64) string { return orDash(Percent(v)) }, 56}
}

// rates reads a transfer chart's values as its figures write them:
// 512.0 KiB/s.
func rates(c *ui.Context, history []Point) readout {
	return readout{history, func(v float64) string { return rate(c, v) }, 104}
}

// sizes reads the values of a chart of a share of total bytes as sizes:
// 8.0 GiB.
func sizes(history []Point, total uint64) readout {
	return readout{history, func(v float64) string { return orDash(Bytes(v / 100 * float64(total))) }, 80}
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
// it for assistive technology. While the pointer is over the plot, a guide
// stands at the nearest sample and a tip beside it reads that sample
// (hover).
func areaChart(c *ui.Context, key any, height float32, top float64, left, bottom []chart.Tick, label string, read readout, all ...series) ui.Element {
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
			hover(c, area, top, read, all)
		}).Fill().Gap(6).Label(label)
	})
}

// hover reads the sample under the pointer while it is over area, a plot's
// area: a guide at the sample, a dot on each series there, and beside the
// guide a tip of the sample's time over each series' mark, name and value,
// on Kit's floating surface in the window's overlay. The sample is the one
// nearest the pointer in every frame, so the guide stays under a resting
// pointer as the history moves on and only the tip's text changes; the tip
// goes when the pointer leaves, and neither it nor the marks take the
// pointer, which scrolls the page through them.
//
// MyGo's chart.Select is not used: it keeps the chosen index while the
// pointer rests, so the guide would drift with a history that still fills,
// and it makes every chart a stop of Tab.
func hover(c *ui.Context, area ui.Element, top float64, read readout, all []series) {
	x, _, over := area.PointerPosition()
	w := area.Bounds().W
	if !over || w <= 0 || len(read.history) == 0 || kit.TipsHidden(c) {
		return
	}
	xs := fractions(len(read.history))
	at := chart.Nearest(xs, x/w)
	p := theme.Of(c)
	guide := chart.RuleX(c.Key("guide"), xs[at], 1).Background(p.MutedForeground.Alpha(0.5)).PassThrough()
	for i, s := range all {
		if at >= len(s.values) {
			continue
		}
		if y := heights(s.values[at:at+1], top)[0]; !math.IsNaN(float64(y)) {
			chart.Point(c.Key(i), xs[at], y, 6, 6).Radius(3).Background(s.color).PassThrough()
		}
	}
	ui.Overlay(c, func() {
		tip := ui.Column(c)
		kit.Anchored(tip, guide, kit.TipLayer)
		kit.Attach(kit.Floating(c, tip), guide, kit.Right, 8)
		tip.Radius(p.Radius).Padding(6, 10).Gap(4).IgnoreInput(true).Children(func() {
			ui.Text(c, Clock(read.history[at].At)).FontSize(12).Font("monospace").SingleLine().TextColor(p.MutedForeground)
			for i, s := range all {
				if at >= len(s.values) {
					continue
				}
				ui.Row(c.Key(i)).Gap(6).AlignItems(ui.Center).Children(func() {
					tick(c, s.color)
					ui.Text(c, s.name).FontSize(12).SingleLine().Grow(1).TextColor(p.MutedForeground)
					ui.Text(c, read.text(s.values[at])).FontSize(14).Font("monospace").SingleLine().MinWidth(read.width).TextAlign(ui.End)
				})
			}
		})
	})
}

// sparkline is a small line of values from 0 to top, without axes or a
// tip: the processes' loads, which the reference does not read on hover
// either.
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
