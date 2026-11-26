package statisticsdesktop

import (
	"cmp"
	"fmt"
	"math"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/chart"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	minuteMs   int64 = 60_000
	halfHourMs int64 = 1_800_000
	dayMs      int64 = 86_400_000
)

type seriesSpec struct {
	id       string
	colorIdx int // index into theme.Palette.Chart (chart_2=1, chart_3=2, chart_4=3)
	labelKey string
}

var tokenSeries = [3]seriesSpec{
	{id: "cache", colorIdx: 2, labelKey: "usage_cache_hits"},     // chart_3
	{id: "input", colorIdx: 1, labelKey: "usage_uncached_input"}, // chart_2
	{id: "output", colorIdx: 3, labelKey: "usage_column_output"}, // chart_4
}

const heatmapColorIdx = 1 // chart_2

func floorDiv(a, b int64) int64 {
	q := a / b
	if (a^b) < 0 && a%b != 0 {
		q--
	}
	return q
}

func shiftedUTC(ms int64, offsetMinutes int32) time.Time {
	return time.UnixMilli(ms + int64(offsetMinutes)*minuteMs).UTC()
}

func calendarDayNumber(ms int64, offsetMinutes int32) int64 {
	return floorDiv(ms+int64(offsetMinutes)*minuteMs, dayMs)
}

func formatCalendarDate(t time.Time) string {
	return fmt.Sprintf("%04d-%02d-%02d", t.Year(), int(t.Month()), t.Day())
}

func formatClock(t time.Time) string {
	return fmt.Sprintf("%02d:%02d", t.Hour(), t.Minute())
}

func formatMonthDay(t time.Time) string {
	return fmt.Sprintf("%d/%d", int(t.Month()), t.Day())
}

func incompleteResponses(m protocol.UsageMetrics) uint64 {
	if m.IncompleteTokenResponses > 0 || m.Tokens != nil {
		return m.IncompleteTokenResponses
	}
	return m.Responses
}

func pointTotalValue(m protocol.UsageMetrics) (uint64, bool) {
	if incompleteResponses(m) > 0 {
		return 0, false
	}
	if m.Tokens != nil {
		return m.Tokens.Input + m.Tokens.Output, true
	}
	if m.Responses == 0 {
		return 0, true
	}
	return 0, false
}

// tokenParts splits usage into disjoint [cached (capped at input), uncached, output].
func tokenParts(tokens *protocol.ConversationUsage) ([3]uint64, bool) {
	if tokens == nil {
		return [3]uint64{}, false
	}
	cached := min(tokens.CachedInput, tokens.Input)
	return [3]uint64{cached, tokens.Input - cached, tokens.Output}, true
}

func heatmapColumns(width float32) int {
	if width <= 0 {
		return 1
	}
	return max(1, int(math.Ceil(float64(width)/18)))
}

func heatmapDays(width float32) int {
	return min(366, heatmapColumns(width)*7)
}

func addUsagePtr(a, b *protocol.ConversationUsage) *protocol.ConversationUsage {
	if a == nil && b == nil {
		return nil
	}
	var out protocol.ConversationUsage
	if a != nil {
		out.Input += a.Input
		out.Output += a.Output
		out.CachedInput += a.CachedInput
		out.Reasoning += a.Reasoning
	}
	if b != nil {
		out.Input += b.Input
		out.Output += b.Output
		out.CachedInput += b.CachedInput
		out.Reasoning += b.Reasoning
	}
	return &out
}

// mergeDays sums day points across multiple Nodes while retaining incomplete
// token totals as subtotals.
func mergeDays(lists [][]protocol.UsagePoint) []protocol.UsagePoint {
	byStart := map[int64]*protocol.UsagePoint{}
	for _, list := range lists {
		for _, pt := range list {
			cur := byStart[pt.StartMs]
			if cur == nil {
				cp := protocol.UsagePoint{
					StartMs: pt.StartMs,
					Metrics: pt.Metrics,
					Models:  make(map[string]protocol.ConversationUsage, len(pt.Models)),
				}
				if pt.Metrics.Tokens != nil {
					tCopy := *pt.Metrics.Tokens
					cp.Metrics.Tokens = &tCopy
				}
				for name, usage := range pt.Models {
					cp.Models[name] = usage
				}
				byStart[pt.StartMs] = &cp
				continue
			}
			inc := incompleteResponses(cur.Metrics) + incompleteResponses(pt.Metrics)
			known := addUsagePtr(cur.Metrics.Tokens, pt.Metrics.Tokens)
			cur.Metrics.Responses += pt.Metrics.Responses
			cur.Metrics.Tokens = known
			cur.Metrics.IncompleteTokenResponses = inc
			for name, usage := range pt.Models {
				prev := cur.Models[name]
				prev.Input += usage.Input
				prev.Output += usage.Output
				prev.CachedInput += usage.CachedInput
				prev.Reasoning += usage.Reasoning
				cur.Models[name] = prev
			}
		}
	}
	out := make([]protocol.UsagePoint, 0, len(byStart))
	for _, pt := range byStart {
		out = append(out, *pt)
	}
	slices.SortFunc(out, func(a, b protocol.UsagePoint) int {
		return cmp.Compare(a.StartMs, b.StartMs)
	})
	return out
}

type heatmapCell struct {
	column     int
	row        int
	intensity  float32
	label      string
	labelGroup string
	tip        kit.ChartTip
}

type heatmapGrid struct {
	rows    int
	columns int
	cells   []heatmapCell
}

var heatmapLevels = [5]float32{0, 0.25, 0.45, 0.7, 1}

func buildHeatmap(points []protocol.UsagePoint, t translateFunc, width float32, startMs, endMs int64, offsetMinutes int32) heatmapGrid {
	first := calendarDayNumber(startMs, offsetMinutes)
	last := calendarDayNumber(endMs-1, offsetMinutes)
	columns := heatmapColumns(width)
	spanDays := max(int64(1), last-first+1)
	rows := max(7, int(math.Ceil(float64(spanDays)/float64(columns))))

	type modelCount struct {
		name   string
		tokens uint64
	}
	type dayInfo struct {
		known  bool
		tokens uint64
		models []modelCount
	}

	days := make(map[int64]dayInfo, len(points))
	var maximum uint64
	for _, pt := range points {
		day := calendarDayNumber(pt.StartMs, offsetMinutes)
		if day < first || day > last {
			continue
		}
		tok, known := pointTotalValue(pt.Metrics)
		var models []modelCount
		if known {
			if tok > maximum {
				maximum = tok
			}
			for name, usage := range pt.Models {
				v := usage.Input + usage.Output
				if v > 0 {
					models = append(models, modelCount{name: name, tokens: v})
				}
			}
			slices.SortFunc(models, func(a, b modelCount) int {
				if a.tokens != b.tokens {
					return cmp.Compare(b.tokens, a.tokens)
				}
				return cmp.Compare(a.name, b.name)
			})
			if len(models) > 8 {
				var other uint64
				for _, m := range models[8:] {
					other += m.tokens
				}
				models = append(models[:8], modelCount{name: t("usage_other_models"), tokens: other})
			}
		}
		days[day] = dayInfo{known: known, tokens: tok, models: models}
	}

	totalCells := columns * rows
	cells := make([]heatmapCell, totalCells)
	unknown := t("composer_metric_unknown")
	tokensLabel := t("composer_tokens")

	for pos := 0; pos < totalCells; pos++ {
		day := last - int64(totalCells) + 1 + int64(pos)
		info, exists := days[day]
		level := 0
		if exists && info.known && info.tokens > 0 && maximum > 0 {
			level = min(4, int(math.Ceil(math.Sqrt(float64(info.tokens)/float64(maximum))*4)))
		}
		date := time.UnixMilli(day * dayMs).UTC()
		inRange := day >= first
		lbl := ""
		if inRange {
			lbl = t("usage_month", "month", int(date.Month()))
		}
		tip := kit.ChartTip{
			Heading: formatCalendarDate(date),
		}
		if exists {
			valText := unknown
			if info.known {
				valText = formatCompact(info.tokens)
			}
			tip.Title = valText + " " + tokensLabel
			if len(info.models) > 0 {
				tip.Rows = make([]kit.ChartTipRow, len(info.models))
				for i, m := range info.models {
					tip.Rows[i] = kit.ChartTipRow{Label: m.name, Value: formatCompact(m.tokens)}
				}
			}
		}
		cells[pos] = heatmapCell{
			column:     pos / rows,
			row:        pos % rows,
			intensity:  heatmapLevels[level],
			label:      lbl,
			labelGroup: fmt.Sprintf("%d-%d", date.Year(), int(date.Month())),
			tip:        tip,
		}
	}
	return heatmapGrid{rows: rows, columns: columns, cells: cells}
}

type trendBucket struct {
	start      time.Time
	first      time.Time
	last       time.Time
	incomplete bool
	values     [3]uint64
}

func buildBuckets(points []protocol.UsagePoint, unit string, offsetMinutes int32) []trendBucket {
	grouped := map[int64]*trendBucket{}
	for _, pt := range points {
		date := shiftedUTC(pt.StartMs, offsetMinutes)
		startMs := pt.StartMs
		if unit == "week" {
			wd := (int(date.Weekday()) + 6) % 7 // Monday = 0
			monday := time.Date(date.Year(), date.Month(), date.Day()-wd, 0, 0, 0, 0, time.UTC)
			startMs = monday.UnixMilli() - int64(offsetMinutes)*minuteMs
		}
		bucket := grouped[startMs]
		if bucket == nil {
			bucket = &trendBucket{
				start: shiftedUTC(startMs, offsetMinutes),
				first: date,
				last:  date,
			}
			grouped[startMs] = bucket
		}
		if date.Before(bucket.first) {
			bucket.first = date
		}
		if date.After(bucket.last) {
			bucket.last = date
		}
		if bucket.incomplete {
			continue
		}
		if incompleteResponses(pt.Metrics) > 0 {
			bucket.incomplete = true
			continue
		}
		if p, ok := tokenParts(pt.Metrics.Tokens); ok {
			bucket.values[0] += p[0]
			bucket.values[1] += p[1]
			bucket.values[2] += p[2]
		}
	}
	out := make([]trendBucket, 0, len(grouped))
	for _, b := range grouped {
		out = append(out, *b)
	}
	slices.SortFunc(out, func(a, b trendBucket) int {
		return cmp.Compare(a.start.UnixMilli(), b.start.UnixMilli())
	})
	return out
}

type axisTick struct {
	value float64
	label string
}

// computeAxis builds a rounded Y axis with nice steps from [1, 2, 2.5, 5, 10]*10^k
// and at most four divisions.
func computeAxis(maximum uint64) (float64, []axisTick) {
	if maximum == 0 {
		return 0, []axisTick{{value: 0, label: "0"}, {value: 1, label: "1"}}
	}
	peak := float64(maximum)
	raw := peak / 4
	power := math.Pow(10, math.Floor(math.Log10(raw)))
	step := 10 * power
	for _, factor := range []float64{1, 2, 2.5, 5, 10} {
		cand := factor * power
		if cand >= raw {
			step = cand
			break
		}
	}
	if step < 1 {
		step = 1
	}
	divisions := max(1, int(math.Ceil(peak/step)))
	ticks := make([]axisTick, divisions+1)
	for i := 0; i <= divisions; i++ {
		val := step * float64(i)
		var lbl string
		if val < 1000 {
			lbl = strconv.FormatInt(int64(math.Round(val)), 10)
		} else {
			lbl = strings.Replace(formatCompact(uint64(math.Round(val))), ".0", "", 1)
		}
		ticks[i] = axisTick{value: val, label: lbl}
	}
	return step * float64(divisions), ticks
}

func trendHeading(bucket trendBucket, unit string) string {
	if unit == "half_hour" {
		end := bucket.start.Add(time.Duration(halfHourMs) * time.Millisecond)
		return fmt.Sprintf("%s %s–%s", formatCalendarDate(bucket.start), formatClock(bucket.start), formatClock(end))
	}
	first := formatCalendarDate(bucket.first)
	last := formatCalendarDate(bucket.last)
	if first == last {
		return first
	}
	return first + " – " + last
}

func trendLabels(grouped []trendBucket, unit string) []string {
	labels := make([]string, len(grouped))
	if unit == "half_hour" {
		for i, b := range grouped {
			if b.start.Minute() == 0 && b.start.Hour()%3 == 0 {
				labels[i] = formatClock(b.start)
			}
		}
		return labels
	}
	step := max(1, int(math.Ceil(float64(len(grouped))/7)))
	for i, b := range grouped {
		if (len(grouped)-1-i)%step == 0 {
			labels[i] = formatMonthDay(b.start)
		}
	}
	return labels
}

type trendBar struct {
	label      string
	incomplete bool
	values     [3]float64
	heading    string
	rowLabels  [3]string
	rowValues  [3]string
	footerLbl  string
	footerVal  string
}

type trendData struct {
	maximum float64
	ticks   []axisTick
	buckets []trendBar
	height  float32
}

func buildTrend(days, recent []protocol.UsagePoint, unit string, offsetMinutes int32, t translateFunc) trendData {
	source := days
	if unit == "half_hour" {
		source = recent
	}
	grouped := buildBuckets(source, unit, offsetMinutes)
	var peak uint64
	for _, b := range grouped {
		if b.incomplete {
			continue
		}
		sum := b.values[0] + b.values[1] + b.values[2]
		if sum > peak {
			peak = sum
		}
	}
	maximum, ticks := computeAxis(peak)
	unknown := t("composer_metric_unknown")
	tokensLabel := t("composer_tokens")
	names := trendLabels(grouped, unit)
	bars := make([]trendBar, len(grouped))
	for i, b := range grouped {
		bar := trendBar{
			label:      names[i],
			incomplete: b.incomplete,
			heading:    trendHeading(b, unit),
			footerLbl:  tokensLabel,
		}
		for s := 0; s < 3; s++ {
			bar.rowLabels[s] = t(tokenSeries[s].labelKey)
		}
		if b.incomplete {
			bar.rowValues = [3]string{unknown, unknown, unknown}
			bar.footerVal = unknown
		} else {
			sum := b.values[0] + b.values[1] + b.values[2]
			for s := 0; s < 3; s++ {
				bar.values[s] = float64(b.values[s])
				bar.rowValues[s] = formatCompact(b.values[s])
			}
			bar.footerVal = formatCompact(sum)
		}
		bars[i] = bar
	}
	return trendData{
		maximum: maximum,
		ticks:   ticks,
		buckets: bars,
		height:  200,
	}
}

func renderStackedChart(c *ui.Context, data trendData) ui.Element {
	p := theme.Of(c)
	colors := [3]ui.Color{
		p.Chart[tokenSeries[0].colorIdx],
		p.Chart[tokenSeries[1].colorIdx],
		p.Chart[tokenSeries[2].colorIdx],
	}
	scale := chart.NewLinear(0, max(data.maximum, 1))
	leftTicks := make([]chart.Tick, len(data.ticks))
	for i, tk := range data.ticks {
		leftTicks[i] = chart.Tick{At: scale.At(tk.value), Value: tk.value, Label: tk.label}
	}
	bottomTicks := make([]chart.Tick, 0, len(data.buckets))
	for i, b := range data.buckets {
		if b.label != "" && len(data.buckets) > 0 {
			bottomTicks = append(bottomTicks, chart.Tick{
				At:    (float32(i) + 0.5) / float32(len(data.buckets)),
				Label: b.label,
			})
		}
	}

	return chart.Plot(c, chart.Axes{Left: leftTicks, Bottom: bottomTicks}, func(area ui.Element) {
		for _, tk := range leftTicks {
			chart.RuleY(c, tk.At, 1).Background(p.Border)
		}
		if len(data.buckets) == 0 {
			return
		}
		width := area.Bounds().W
		band := float32(1) / float32(len(data.buckets))
		barW := band * 0.8
		if width > 0 {
			barW = min(barW, 28/width)
		}
		for i, b := range data.buckets {
			middle := (float32(i) + 0.5) * band
			x := chart.Span{Lo: middle - barW/2, Hi: middle + barW/2}
			tip := kit.ChartTip{
				Heading: b.heading,
				Rows: []kit.ChartTipRow{
					{Label: b.rowLabels[0], Value: b.rowValues[0], Swatch: colors[0]},
					{Label: b.rowLabels[1], Value: b.rowValues[1], Swatch: colors[1]},
					{Label: b.rowLabels[2], Value: b.rowValues[2], Swatch: colors[2]},
				},
				Footer: &kit.ChartTipRow{Label: b.footerLbl, Value: b.footerVal},
			}
			if b.incomplete {
				// Omitted from peak and bars, but still exposes the "—" tooltip on hover.
				hit := chart.Rect(c.Key(fmt.Sprintf("incomplete/%d", i)), x, chart.Span{Lo: 0, Hi: 1}).Background(ui.Transparent)
				kit.ChartTooltip(c, hit, tip)
				continue
			}
			total := 0.0
			for s := 0; s < 3; s++ {
				val := b.values[s]
				if val > 0 {
					mark := chart.Rect(c.Key(fmt.Sprintf("bar/%d/%d", i, s)), x, scale.Span(total, total+val)).Background(colors[s])
					mark.Label(b.label + " " + b.rowValues[s])
					kit.ChartTooltip(c, mark, tip)
					total += val
				}
			}
			if total == 0 {
				mark := chart.Rect(c.Key(fmt.Sprintf("zero/%d", i)), x, chart.Span{Lo: 0, Hi: 0.005}).Background(p.MutedForeground)
				kit.ChartTooltip(c, mark, tip)
			}
		}
	}).Height(data.height).FillWidth().Gap(8)
}

func renderHeatmap(c *ui.Context, grid heatmapGrid) ui.Element {
	p := theme.Of(c)
	active := p.Chart[heatmapColorIdx]
	return ui.Column(c).FillWidth().MaxWidth(float32(grid.columns) * 24).Children(func() {
		ui.Box(c.Key("plot")).FillWidth().AspectRatio(float32(grid.columns) / float32(grid.rows)).Children(func() {
			chart.Plot(c, chart.Axes{}, func(_ ui.Element) {
				for i, cell := range grid.cells {
					color := p.Muted
					if cell.intensity > 0 {
						color = active.Alpha(cell.intensity)
					}
					x := chart.Span{
						Lo: float32(cell.column) / float32(grid.columns),
						Hi: (float32(cell.column) + 0.8) / float32(grid.columns),
					}
					y := chart.Span{
						Lo: 1 - (float32(cell.row)+0.8)/float32(grid.rows),
						Hi: 1 - float32(cell.row)/float32(grid.rows),
					}
					mark := chart.Rect(c.Key(i), x, y).Background(color).Radius(2)
					kit.ChartTooltip(c, mark, cell.tip)
				}
			}).Fill()
		})
		ui.Box(c.Key("months")).FillWidth().Height(24).Children(func() {
			previous := ""
			lastColumn := -2
			for i, cell := range grid.cells {
				if cell.label == "" || previous == cell.labelGroup {
					continue
				}
				previous = cell.labelGroup
				if cell.column-lastColumn < 2 {
					continue
				}
				lastColumn = cell.column
				lbl := ui.Text(c.Key(i), cell.label).FontSize(10).SingleLine().TextColor(p.MutedForeground).Absolute().Top(4)
				if cell.column == grid.columns-1 {
					lbl.Right(0)
				} else {
					lbl.LeftPercent(100 * float32(cell.column) / float32(grid.columns))
				}
			}
		})
	})
}
