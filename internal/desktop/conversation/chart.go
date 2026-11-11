package conversation

import (
	"math"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	mc "github.com/egoist/mygo/ui/chart"
	"github.com/firede/limu/diagram"
	"github.com/firede/limu/diagram/pie"
	"github.com/firede/limu/diagram/xychart"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Data charts of a reply (content/visuals): a Mermaid xychart or pie is
// drawn here from limu's parsed model with MyGo's ui/chart, in the theme,
// with the value of the mark under the pointer. Every other diagram, and a
// chart whose data cannot be drawn, is CrychicUI's picture (visuals.go;
// docs/crychic.md says why its chart components are not the painters).

const (
	// chartWidth is the widest a chart is, chartHeight the height of a
	// plot with its axis labels, and chartTall the most a chart of many
	// rows takes; pieSize is a pie's box.
	chartWidth  = 560
	chartHeight = 200
	chartTall   = 260
	pieSize     = 180
	// The most a chart draws; more is the diagram's picture.
	maxPoints = 366
	maxSeries = 8
	maxSlices = 12
	// axisFont is the size of axis labels, axis titles and the legend.
	axisFont = 11
	// barWidth is the widest a bar is, however few there are.
	barWidth = 40
	// tickRunes, tipRunes and labelRunes are the most characters of a
	// label on an axis, in a tip, and in a legend or read out.
	tickRunes  = 14
	tipRunes   = 20
	labelRunes = 60
)

// series is a row of a chart's values, one per label, NaN where the
// source gives none.
type series struct {
	name   string
	line   bool
	values []float64
}

// dataChart is the checked data of a Mermaid xychart or pie.
type dataChart struct {
	pie    bool
	title  string
	labels []string // the categories, or a pie's slices
	ticks  []string // the labels as an axis shows them
	series []series // a pie has one
	// horizontal runs the categories down and the values across.
	horizontal bool
	// category and value are the axes' titles.
	category, value string
	// lo and hi are the value axis, fixed when the source set them.
	lo, hi float64
	fixed  bool
	// showData adds a pie's values to its legend; total is their sum.
	showData bool
	total    float64
}

// chartOf is the chart of a diagram limu prepared: nil for a diagram that
// is not a data chart, for data limu cannot parse, and for data the
// painters cannot show honestly.
func chartOf(p *diagram.Prepared) *dataChart {
	switch p.Type {
	case diagram.XYChart:
		if m, err := xychart.ParsePrepared(p); err == nil {
			return xyChart(m, p.Config)
		}
	case diagram.Pie:
		if m, err := pie.ParsePrepared(p); err == nil {
			return pieChart(m)
		}
	}
	return nil
}

// xyChart checks an xychart's model. It is not drawn for no series or
// more than maxSeries, no category or more than maxPoints, a value that is
// not finite, no value at all, series of different lengths on a numeric
// axis, or a range past what a float64 holds. A category without a value
// is a gap.
func xyChart(m *xychart.Model, config diagram.Config) *dataChart {
	if m == nil || len(m.Plots) == 0 || len(m.Plots) > maxSeries {
		return nil
	}
	d := &dataChart{title: m.Title, category: clip(m.XAxis.Title, labelRunes), value: clip(m.YAxis.Title, labelRunes)}
	orientation := m.Orientation
	if xy, ok := config["xyChart"].(map[string]any); ok && orientation == "" {
		orientation, _ = xy["chartOrientation"].(string)
	}
	d.horizontal = orientation == "horizontal"
	banded := m.XAxis.Type == "band"
	labels := m.XAxis.Categories
	if !banded {
		labels = nil
		for _, p := range m.Plots {
			if len(p.Data) > len(labels) {
				labels = make([]string, len(p.Data))
				for i, at := range p.Data {
					labels[i] = at.X
					if v, err := strconv.ParseFloat(at.X, 64); err == nil {
						labels[i] = number(v)
					}
				}
			}
		}
	}
	n := len(labels)
	if n == 0 || n > maxPoints {
		return nil
	}
	for _, l := range labels {
		d.labels, d.ticks = append(d.labels, clip(l, labelRunes)), append(d.ticks, clip(l, tickRunes))
	}
	lo, hi := math.Inf(1), math.Inf(-1)
	bars := false
	for _, p := range m.Plots {
		if !banded && len(p.Data) != n {
			return nil
		}
		s := series{name: clip(p.Title, labelRunes), line: p.Type == "line", values: make([]float64, n)}
		for i := range s.values {
			v := math.NaN()
			if i < len(p.Data) {
				v = p.Data[i].Value
			}
			if math.IsInf(v, 0) {
				return nil
			}
			if s.values[i] = v; !math.IsNaN(v) {
				lo, hi = min(lo, v), max(hi, v)
			}
		}
		bars = bars || !s.line
		d.series = append(d.series, s)
	}
	if lo > hi {
		return nil
	}
	if y := m.YAxis; m.HasSetYAxis && y.Min < y.Max && !math.IsInf(y.Min, 0) && !math.IsInf(y.Max, 0) {
		d.lo, d.hi, d.fixed = y.Min, y.Max, true
	} else {
		// Bars are read from zero; a flat line keeps zero in view too.
		if bars || lo == hi {
			lo, hi = min(lo, 0), max(hi, 0)
		}
		if lo == hi {
			hi = 1
		}
		d.lo, d.hi = lo, hi
	}
	if math.IsInf(d.hi-d.lo, 0) {
		return nil
	}
	return d
}

// pieChart checks a pie's model. It is not drawn for no slice or more
// than maxSlices, a value that is negative or not finite, or a sum of
// zero.
func pieChart(m *pie.Model) *dataChart {
	if m == nil || len(m.Sections) == 0 || len(m.Sections) > maxSlices {
		return nil
	}
	d := &dataChart{pie: true, title: m.Title, showData: m.ShowData}
	s := series{values: make([]float64, len(m.Sections))}
	for i, part := range m.Sections {
		if !(part.Value >= 0) || math.IsInf(part.Value, 0) {
			return nil
		}
		d.labels = append(d.labels, clip(part.Label, labelRunes))
		s.values[i] = part.Value
		d.total += part.Value
	}
	if !(d.total > 0) || math.IsInf(d.total, 0) {
		return nil
	}
	d.series = []series{s}
	return d
}

// clip is s on one line within n characters, a longer one cut short with
// an ellipsis.
func clip(s string, n int) string {
	if len(s) > 4*n+64 {
		// Enough bytes for n characters and the spaces between them.
		s = strings.ToValidUTF8(s[:4*n+64], "")
	}
	s = strings.Join(strings.Fields(s), " ")
	if utf8.RuneCountInString(s) <= n {
		return s
	}
	return string([]rune(s)[:n-1]) + "…"
}

// number writes a value in full, without the digits a float64 adds.
func number(v float64) string {
	if a := math.Abs(v); a != 0 && (a >= 1e15 || a < 1e-4) {
		return strconv.FormatFloat(v, 'g', 4, 64)
	}
	if v = math.Round(v*1e4) / 1e4; v == 0 {
		v = 0 // no -0
	}
	return strconv.FormatFloat(v, 'f', -1, 64)
}

// axisNumbers writes the ticks of an axis from lo to hi, all in one unit:
// thousands as k, millions as M and billions as B where the axis reaches
// ten thousand, as Count does.
func axisNumbers(lo, hi float64) func(float64) string {
	per, unit := 1.0, ""
	switch reach := max(math.Abs(lo), math.Abs(hi)); {
	case reach >= 1e15:
		return number
	case reach >= 1e9:
		per, unit = 1e9, "B"
	case reach >= 1e6:
		per, unit = 1e6, "M"
	case reach >= 1e4:
		per, unit = 1e3, "k"
	}
	return func(v float64) string {
		if v == 0 {
			return "0"
		}
		return number(v/per) + unit
	}
}

// share is a slice's part of its pie, in percent.
func (d *dataChart) share(i int) string {
	pct := 100 * d.series[0].values[i] / d.total
	digits := 0
	if pct > 0 && pct < 10 {
		digits = 1
	}
	return strings.TrimSuffix(strconv.FormatFloat(pct, 'f', digits, 64), ".0") + "%"
}

// describe tells assistive technology the values at a label, as the tip
// shows them: "W45: 2015", or "W45: Pro 65, Free 54".
func (d *dataChart) describe(i int) string {
	if d.pie {
		return d.labels[i] + ": " + number(d.series[0].values[i]) + " (" + d.share(i) + ")"
	}
	var parts []string
	for _, s := range d.series {
		if v := s.values[i]; !math.IsNaN(v) {
			parts = append(parts, strings.TrimSpace(s.name+" "+number(v)))
		}
	}
	return strings.TrimPrefix(d.labels[i]+": "+strings.Join(parts, ", "), ": ")
}

// seriesColor is the color of the i-th series or slice: the emblem tints
// (blue, violet, teal, amber, gray), their lighter end in dark mode, and
// from the sixth on a step toward the background each lap.
func seriesColor(p *theme.Palette, i int) ui.Color {
	end := 1
	if p.Dark {
		end = 0
	}
	c := theme.Tints[i%len(theme.Tints)][end]
	if lap := i / len(theme.Tints) % 3; lap > 0 {
		c = c.Mix(kit.Opaque(p.Background), 0.35*float32(lap))
	}
	return c
}

// textWidth estimates s at the axis font: a full-width character takes
// the font's size and any other a little over half of it. The axis is laid
// out with the real widths; this only decides which labels to leave out.
func textWidth(s string) float32 {
	var w float32
	for _, r := range s {
		if r >= 0x2E80 {
			w += axisFont
		} else {
			w += axisFont * 0.58
		}
	}
	return w
}

// pick is the mark a chart shows the value of: the one under the pointer,
// or the one the arrow keys moved to.
type pick struct {
	at   int
	over bool
	x, y float32
}

// choose makes e, a plot's area or a pie, pick one of n marks: under is
// the mark at a point of e, or -1, while the pointer is over it, and none
// once it leaves. e takes the keyboard focus as one stop of Tab, where
// the arrows, Home and End move the pick and Escape clears it. It reads a
// pick to assistive technology as e's value.
func choose(c *ui.Context, e ui.Element, n int, under func(x, y float32) int, describe func(i int) string) int {
	e.Focusable()
	st := ui.Local(e, "chart.pick", func() pick { return pick{at: -1} })
	if st.at >= n {
		st.at = -1
	}
	last, moved := st.at, false
	move := func(i int) { st.at, moved = max(0, min(n-1, i)), true }
	switch {
	case e.Shortcut(0, ui.KeyRight), e.Shortcut(0, ui.KeyDown):
		move(st.at + 1)
	case e.Shortcut(0, ui.KeyLeft), e.Shortcut(0, ui.KeyUp):
		if st.at < 0 {
			move(n - 1)
		} else {
			move(st.at - 1)
		}
	case e.Shortcut(0, ui.KeyHome):
		move(0)
	case e.Shortcut(0, ui.KeyEnd):
		move(n - 1)
	case st.at >= 0 && e.Shortcut(0, ui.KeyEscape):
		st.at = -1
	}
	// The pointer picks as it comes and moves, not while it rests where
	// it was as the keys move the pick.
	if x, y, over := e.PointerPosition(); over {
		if !moved && (!st.over || x != st.x || y != st.y) {
			st.at = under(x, y)
		}
		st.over, st.x, st.y = true, x, y
	} else if st.over || !e.Focused() {
		st.over, st.at = false, -1
	}
	if st.at >= 0 {
		desc := describe(st.at)
		e.Value(desc)
		if moved && st.at != last {
			c.Announce(desc)
		}
	}
	return st.at
}

// tip builds the value of the picked mark on Kit's floating surface, in
// the plot: the pointer goes through it.
func tip(c *ui.Context, margin [4]float32, build func()) {
	p := theme.Of(c)
	box := ui.Column(c.Key("tip")).Shrink(0).Margin(margin[:]...).Padding(4, 8).Gap(2).IgnoreInput(true)
	kit.Floating(c, box).Radius(p.Radius).Children(build)
}

// tipLine is a line of a tip: a swatch for one of several series, the
// muted name, cut short, and the value. A tip is as wide as its lines.
func tipLine(c *ui.Context, swatch *ui.Color, name, value string) {
	p := theme.Of(c)
	ui.Row(c).Gap(6).Shrink(0).AlignItems(ui.Center).Children(func() {
		if swatch != nil {
			ui.Box(c).Size(8, 8).Shrink(0).Radius(2).Background(*swatch)
		}
		if name = clip(name, tipRunes); name != "" {
			ui.Text(c, name).FontSize(12).NoWrap().Shrink(0).TextColor(p.MutedForeground)
		}
		if value != "" {
			ui.Text(c, value).FontSize(12).NoWrap().Shrink(0).FontWeight(500)
		}
	})
}

// build draws the chart at most chartWidth wide in a reply of width
// (none before the first layout), named name for assistive technology.
func (d *dataChart) build(c *ui.Context, name string, width float32) {
	if d.pie {
		d.buildPie(c, name)
		return
	}
	d.buildXY(c, name, width)
}

// buildXY is an xychart: bars side by side per category and lines over
// them on one value axis, light grid lines at its ticks, small muted axis
// labels (categories that would overlap are left out), the source's axis
// titles, and a legend of the named series when there are several.
func (d *dataChart) buildXY(c *ui.Context, name string, width float32) {
	p := theme.Of(c)
	n := len(d.labels)
	if width <= 0 || width > chartWidth {
		width = chartWidth
	}
	height := float32(chartHeight)
	if d.horizontal {
		height = min(max(float32(n)*24+28, 120), chartTall)
	}
	bars := 0
	for _, s := range d.series {
		if !s.line {
			bars++
		}
	}
	// The room a category has along its axis, and the room its label
	// takes there: the plot is what the value axis and the labels leave.
	step, room := (width-48)/float32(n), float32(0)
	if d.horizontal {
		step, room = (height-24)/float32(n), axisFont+4
	} else {
		for _, l := range d.ticks {
			room = max(room, textWidth(l)+10)
		}
	}
	band := mc.NewBand(d.labels...)
	switch {
	case bars > 0 && step >= 6:
		band = band.Padding(0.3, 0.15)
	case bars > 0 && step >= 3:
		band = band.Padding(0.15, 0.1)
	}
	scale := mc.NewLinear(d.lo, d.hi)
	if !d.fixed {
		scale = scale.Nice(4)
	}
	scale.Format = axisNumbers(scale.Lo, scale.Hi)
	values := scale.Ticks(4)
	every := max(1, int(math.Ceil(float64(room/step))))
	var categories []mc.Tick
	for i := 0; i < n; i += every {
		at := band.At(i)
		if d.horizontal {
			at = 1 - at // the first category on top
		}
		categories = append(categories, mc.Tick{At: at, Value: float64(i), Label: d.ticks[i]})
	}
	axes := mc.Axes{Left: values, Bottom: categories}
	above, below := d.value, d.category
	if d.horizontal {
		axes.Left, axes.Bottom = categories, values
		above, below = below, above
	}
	axes.Label = func(_ mc.Side, t mc.Tick) {
		ui.Text(c, t.Label).NoWrap().FontSize(axisFont).TextColor(p.MutedForeground)
	}
	title := func(s string) ui.Element {
		return ui.Text(c, s).FontSize(axisFont).SingleLine().MinWidth(0).TextColor(p.MutedForeground)
	}
	ui.Column(c.Key("chart")).FillWidth().MaxWidth(chartWidth).MinWidth(0).Gap(4).Children(func() {
		if above != "" {
			title(above)
		}
		mc.Plot(c, axes, func(area ui.Element) {
			area.Role(ui.RoleImage).Label(name)
			hot := choose(c, area, n, func(x, y float32) int {
				b := area.Bounds()
				at, best := x/max(b.W, 1), 0
				if d.horizontal {
					at = y / max(b.H, 1)
				}
				for i := range n {
					if abs32(band.At(i)-at) < abs32(band.At(best)-at) {
						best = i
					}
				}
				return best
			}, d.describe)
			area.Draw(func(pt *ui.Painter, r ui.Rect) { d.paint(pt, r, p, band, scale, values, bars, hot) })
			if hot >= 0 {
				d.tipXY(c, area, band, scale, hot)
			}
		}).FillWidth().Height(height).Gap(6)
		if below != "" {
			title(below).AlignSelf(ui.Center)
		}
		d.legend(c)
	})
}

func abs32(v float32) float32 {
	if v < 0 {
		return -v
	}
	return v
}

// paint draws an xychart's plot in r: the grid, the picked category's
// band (bars) or rule (lines), the bars and the lines with their points.
func (d *dataChart) paint(pt *ui.Painter, r ui.Rect, p *theme.Palette, band mc.Band, scale mc.Linear, ticks []mc.Tick, bars, hot int) {
	// at is the point k along the categories and f up the values.
	at := func(k, f float32) (x, y float32) {
		if d.horizontal {
			return r.X + f*r.W, r.Y + k*r.H
		}
		return r.X + k*r.W, r.Y + (1-f)*r.H
	}
	// span is the box between two such points.
	span := func(k0, f0, k1, f1 float32) ui.Rect {
		x0, y0 := at(k0, f0)
		x1, y1 := at(k1, f1)
		return ui.Rect{X: min(x0, x1), Y: min(y0, y1), W: abs32(x1 - x0), H: abs32(y1 - y0)}
	}
	unit := func(f float32) float32 { return min(max(f, 0), 1) }
	// wide is a bar along the categories: its share of the band, at most
	// barWidth, the bars of a category together in the middle of the band.
	extent := r.W
	if d.horizontal {
		extent = r.H
	}
	wide := float32(0)
	if bars > 0 {
		wide = min(band.Width()/float32(bars), barWidth/max(extent, 1))
	}
	for _, t := range ticks {
		x0, y0 := at(0, t.At)
		x1, y1 := at(1, t.At)
		pt.Line(x0, y0, x1, y1, 1, p.Border)
	}
	if hot >= 0 {
		if b := band.Band(hot); bars > 0 {
			// Around the bars: half the room between bands, at most 10 DIPs.
			half := wide*float32(bars)/2 + min(band.Width()*band.Inner/(2*(1-band.Inner)), 10/max(extent, 1))
			pt.Fill(span(b.Mid()-half, 0, b.Mid()+half, 1), p.Hover, 4)
		} else {
			x0, y0 := at(b.Mid(), 0)
			x1, y1 := at(b.Mid(), 1)
			pt.Line(x0, y0, x1, y1, 1, p.MutedForeground.Alpha(0.5))
		}
	}
	// Marks past a range the source set stop at the plot's edge, with
	// room for a point on it.
	pt.Clip(ui.Rect{X: r.X - 5, Y: r.Y - 5, W: r.W + 10, H: r.H + 10}, 0, func() {
		base := unit(scale.At(min(max(0, scale.Lo), scale.Hi)))
		k := 0
		for s, sr := range d.series {
			if sr.line {
				continue
			}
			ink := seriesColor(p, s)
			for i, v := range sr.values {
				if math.IsNaN(v) {
					continue
				}
				lo := band.Band(i).Mid() - wide*float32(bars)/2
				box := span(lo+wide*float32(k), base, lo+wide*float32(k+1), unit(scale.At(v)))
				col := ink
				if hot >= 0 && hot != i {
					col = ink.Alpha(0.45)
				}
				pt.Fill(box, col, min(2, box.W/2, box.H/2))
			}
			k++
		}
		ground := kit.Opaque(p.Background)
		for s, sr := range d.series {
			if !sr.line {
				continue
			}
			ink := seriesColor(p, s)
			var path ui.Path
			drawn, down := false, false
			for i, v := range sr.values {
				if math.IsNaN(v) {
					down = false
					continue
				}
				if x, y := at(band.At(i), scale.At(v)); down {
					path.LineTo(x, y)
					drawn = true
				} else {
					path.MoveTo(x, y)
				}
				down = true
			}
			if drawn {
				pt.StrokePath(&path, 2, ink)
			}
			last := len(sr.values) - 1
			for i, v := range sr.values {
				if math.IsNaN(v) {
					continue
				}
				// Few points show each; of many, the picked one and one
				// with no neighbor, which has no line.
				alone := (i == 0 || math.IsNaN(sr.values[i-1])) && (i == last || math.IsNaN(sr.values[i+1]))
				size := float32(6)
				switch {
				case i == hot:
					size = 8
				case last >= 24 && !alone:
					continue
				}
				x, y := at(band.At(i), scale.At(v))
				dot := func(size float32, c ui.Color) {
					pt.Fill(ui.Rect{X: x - size/2, Y: y - size/2, W: size, H: size}, c, size/2)
				}
				dot(size+3, ground)
				dot(size, ink)
			}
		}
	})
}

// tipXY shows the picked category's values inside the plot: over the mark
// of a single series, after the end of a horizontal bar, and beside the
// category, clear of its marks, for several series.
func (d *dataChart) tipXY(c *ui.Context, area ui.Element, band mc.Band, scale mc.Linear, i int) {
	p := theme.Of(c)
	top, rows := float32(0), 0
	for _, s := range d.series {
		if v := s.values[i]; !math.IsNaN(v) {
			top = max(top, min(max(scale.At(v), 0), 1))
			rows++
		}
	}
	if rows == 0 {
		return
	}
	content := func() {
		if len(d.series) == 1 {
			tipLine(c, nil, d.labels[i], number(d.series[0].values[i]))
			return
		}
		if d.labels[i] != "" {
			tipLine(c, nil, d.labels[i], "")
		}
		for s, sr := range d.series {
			if v := sr.values[i]; !math.IsNaN(v) {
				ink := seriesColor(p, s)
				tipLine(c, &ink, sr.name, number(v))
			}
		}
	}
	k := band.At(i)
	if d.horizontal {
		// After the longest bar's end, within the plot's height; where
		// the bar leaves no room, under its row or over it.
		row := band.Band(i)
		after := func(y float32, v ui.Align) {
			mc.Anchor(c, top, 1-y, ui.Start, v, func() { tip(c, [4]float32{0, 0, 0, 8}, content) })
		}
		switch {
		case top > 0.6 && k < 0.5:
			mc.Anchor(c, 1, 1-row.Hi, ui.End, ui.Start, func() { tip(c, [4]float32{6, 0, 0, 0}, content) })
		case top > 0.6:
			mc.Anchor(c, 1, 1-row.Lo, ui.End, ui.End, func() { tip(c, [4]float32{0, 0, 6, 0}, content) })
		case k < 0.3:
			after(row.Lo, ui.Start)
		case k > 0.7:
			after(row.Hi, ui.End)
		default:
			after(k, ui.Center)
		}
		return
	}
	if len(d.series) > 1 {
		if k < 0.5 {
			mc.Anchor(c, k, 1, ui.Start, ui.Start, func() { tip(c, [4]float32{0, 0, 0, 12}, content) })
		} else {
			mc.Anchor(c, k, 1, ui.End, ui.Start, func() { tip(c, [4]float32{0, 12, 0, 0}, content) })
		}
		return
	}
	if h := area.Bounds().H; h > 0 {
		top = min(top, 1-34/h) // the room a tip of one line takes
	}
	side := ui.Center
	switch {
	case k < 0.2:
		side = ui.Start
	case k > 0.8:
		side = ui.End
	}
	mc.Anchor(c, k, max(top, 0), side, ui.End, func() { tip(c, [4]float32{0, 0, 8, 0}, content) })
}

// legend names the series under the plot when there are several and the
// source named any: a square for bars, a stroke for a line.
func (d *dataChart) legend(c *ui.Context) {
	named := false
	for _, s := range d.series {
		named = named || s.name != ""
	}
	if len(d.series) < 2 || !named {
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("legend")).Wrap().GapX(12).GapY(2).MinWidth(0).Children(func() {
		for i, s := range d.series {
			if s.name == "" {
				continue
			}
			ui.Row(c.Key(i)).Gap(5).MinWidth(0).AlignItems(ui.Center).Children(func() {
				mark := ui.Box(c).Shrink(0).Background(seriesColor(p, i))
				if s.line {
					mark.Size(10, 2).Radius(1)
				} else {
					mark.Size(8, 8).Radius(2)
				}
				ui.Text(c, s.name).FontSize(axisFont).SingleLine().MinWidth(0).TextColor(p.MutedForeground)
			})
		}
	})
}

// buildPie is a pie with its legend beside it: each slice's label and
// share, and its value where the source asks to show the data. The slice
// under the pointer, or of the legend row under it, stands out.
func (d *dataChart) buildPie(c *ui.Context, name string) {
	p := theme.Of(c)
	values := d.series[0].values
	n := len(values)
	shown := 0
	for _, v := range values {
		if v > 0 {
			shown++
		}
	}
	pad := 0.02
	if shown < 2 {
		pad = 0
	}
	slices := mc.Pie(values, pad)
	const outer = pieSize/2 - 2
	ui.Row(c.Key("chart")).FillWidth().MaxWidth(chartWidth).MinWidth(0).Gap(20).AlignItems(ui.Center).Children(func() {
		wheel := ui.Box(c.Key("pie")).Size(pieSize, pieSize).Shrink(0).Role(ui.RoleImage).Label(name)
		hot := choose(c, wheel, n, func(x, y float32) int {
			if i, ok := mc.SliceAt(slices, x-pieSize/2, y-pieSize/2, 0, outer); ok && values[i] > 0 {
				return i
			}
			return -1
		}, d.describe)
		// row is the legend row under the pointer, set as the legend is
		// built and read as the pie is painted.
		row := ui.Local(wheel, "chart.row", func() int { return -1 })
		wheel.Draw(func(pt *ui.Painter, r ui.Rect) {
			lit := hot
			if lit < 0 {
				lit = *row
			}
			for i, s := range slices {
				if values[i] <= 0 {
					continue
				}
				ink := seriesColor(p, i)
				if lit >= 0 && lit != i {
					ink = ink.Alpha(0.4)
				}
				pt.FillPath(mc.Sector(r.X+r.W/2, r.Y+r.H/2, 0, outer, s), ink)
			}
		})
		if hot >= 0 {
			wheel.Children(func() {
				x, y := mc.Polar(pieSize/2, pieSize/2, 0.6*outer, slices[hot].Mid())
				mc.Anchor(c, x/pieSize, 1-max(y, 34)/pieSize, ui.Center, ui.End, func() {
					tip(c, [4]float32{0, 0, 6, 0}, func() { tipLine(c, nil, d.labels[hot], number(values[hot])+" · "+d.share(hot)) })
				})
			})
		}
		*row = -1
		entry := func(i int) {
			line := ui.Row(c.Key(i)).Height(20).Gap(6).MinWidth(0).AlignItems(ui.Center)
			text := p.MutedForeground
			if line.Hovered() {
				*row = i
			}
			if hot == i || *row == i {
				text = p.Foreground
			}
			line.Children(func() {
				ui.Box(c).Size(8, 8).Shrink(0).Radius(2).Background(seriesColor(p, i))
				ui.Text(c, d.labels[i]).FontSize(12).SingleLine().Shrink(1).MinWidth(0).TextColor(text)
				value := d.share(i)
				if d.showData {
					value = number(values[i]) + " · " + value
				}
				ui.Text(c, value).FontSize(12).NoWrap().Shrink(0).TextColor(p.MutedForeground)
			})
		}
		// Up to six slices in one column, more in two.
		half := n
		if n > 6 {
			half = (n + 1) / 2
		}
		ui.Row(c.Key("legend")).Grow(1).MinWidth(0).Gap(16).Children(func() {
			for from := 0; from < n; from += half {
				ui.Column(c.Key(from)).Shrink(1).MinWidth(0).Children(func() {
					for i := from; i < min(from+half, n); i++ {
						entry(i)
					}
				})
			}
		})
	})
}
