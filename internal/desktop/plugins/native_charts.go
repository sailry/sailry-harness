package plugins

import (
	"encoding/json"
	"fmt"
	"sync"
	"weak"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/chart"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type nativeChartRow struct {
	Label string `json:"label"`
	Value string `json:"value"`
}

// chartColors are the theme snapshot's colors of the last palette asked
// for: charts ask for them per series on every frame.
var chartColors struct {
	mu      sync.Mutex
	palette weak.Pointer[theme.Palette]
	colors  map[string]string
}

func nativeChartColor(view *ui.Context, name string) ui.Color {
	palette := theme.Of(view)
	key := weak.Make(palette)
	chartColors.mu.Lock()
	if chartColors.palette != key || chartColors.colors == nil {
		var snapshot struct {
			Colors map[string]string `json:"colors"`
		}
		_ = json.Unmarshal(ThemeSnapshot(palette), &snapshot)
		chartColors.palette, chartColors.colors = key, snapshot.Colors
		if chartColors.colors == nil {
			chartColors.colors = map[string]string{}
		}
	}
	value := chartColors.colors[name]
	chartColors.mu.Unlock()
	if color, err := nativeColor(value); err == nil {
		return color
	}
	return palette.Primary
}

// chartSource is an element's chart source, parsed and checked once per
// element when it has a cache.
func chartSource[Source any](element NativeElement, props map[string]json.RawMessage, parse func(map[string]json.RawMessage) (*Source, error)) (*Source, error) {
	type parsed struct {
		source *Source
		err    error
	}
	if element.cache == nil {
		return parse(props)
	}
	if kept, ok := element.cache.chart.(parsed); ok {
		return kept.source, kept.err
	}
	source, err := parse(props)
	element.cache.chart = parsed{source, err}
	return source, err
}

func (binding *NativeBinding) renderChart(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	_, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	switch element.Export {
	case "ProgressBar":
		value := nativeProp(props, "value", float64(0))
		if !finiteNumber(value) {
			return ui.Element{}, hostInvalid("invalid progress value")
		}
		return ui.Progress(view, min(max(value, 0), 100)/100).Label(nativeProp(props, "label", "")).FillWidth().TextColor(nativeChartColor(view, nativeProp(props, "color", "primary"))), nil
	case "Heatmap":
		source, err := chartSource(element, props, parseHeatmap)
		if err != nil {
			return ui.Element{}, err
		}
		return binding.renderHeatmap(view, source), nil
	case "StackedChart":
		source, err := chartSource(element, props, parseStackedChart)
		if err != nil {
			return ui.Element{}, err
		}
		return binding.renderStackedChart(view, source), nil
	}
	return ui.Element{}, hostInvalid("unknown native chart")
}

type heatmapSource struct {
	Rows    int `json:"rows"`
	Columns int `json:"columns"`
	Cells   []struct {
		Column     int     `json:"column"`
		Row        int     `json:"row"`
		Intensity  float32 `json:"intensity"`
		Label      string  `json:"label"`
		LabelGroup *string `json:"label_group"`
		Tooltip    struct {
			Heading string           `json:"heading"`
			Title   string           `json:"title"`
			Rows    []nativeChartRow `json:"rows"`
		} `json:"tooltip"`
		tip kit.ChartTip
	} `json:"cells"`
}

func parseHeatmap(props map[string]json.RawMessage) (*heatmapSource, error) {
	var source heatmapSource
	body, _ := hostJSON(props)
	if json.Unmarshal(body, &source) != nil || source.Rows <= 0 || source.Columns <= 0 || len(source.Cells) > 100000 {
		return nil, hostInvalid("invalid heatmap bounds")
	}
	for index := range source.Cells {
		cell := &source.Cells[index]
		if cell.Row < 0 || cell.Row >= source.Rows || cell.Column < 0 || cell.Column >= source.Columns || !finiteNumber(float64(cell.Intensity)) || cell.Intensity < 0 || cell.Intensity > 1 {
			return nil, hostInvalid("invalid heatmap cell")
		}
		cell.tip = kit.ChartTip{Heading: cell.Tooltip.Heading, Title: cell.Tooltip.Title}
		for _, row := range cell.Tooltip.Rows {
			cell.tip.Rows = append(cell.tip.Rows, kit.ChartTipRow{Label: row.Label, Value: row.Value})
		}
	}
	return &source, nil
}

func (binding *NativeBinding) renderHeatmap(view *ui.Context, source *heatmapSource) ui.Element {
	palette := theme.Of(view)
	active := nativeChartColor(view, "chart_2")
	// Limit the plot's width before resolving its aspect ratio so short
	// calendars keep the same horizontal and vertical cell sizes.
	return ui.Column(view).FillWidth().MaxWidth(float32(source.Columns) * 24).Children(func() {
		ui.Box(view).FillWidth().AspectRatio(float32(source.Columns) / float32(source.Rows)).Children(func() {
			chart.Plot(view, chart.Axes{}, func(area ui.Element) {
				for _, cell := range source.Cells {
					color := palette.Muted
					if cell.Intensity > 0 {
						color = active.Alpha(cell.Intensity)
					}
					x := chart.Span{Lo: float32(cell.Column) / float32(source.Columns), Hi: (float32(cell.Column) + 0.8) / float32(source.Columns)}
					y := chart.Span{Lo: 1 - (float32(cell.Row)+0.8)/float32(source.Rows), Hi: 1 - float32(cell.Row)/float32(source.Rows)}
					mark := chart.Rect(view, x, y).Background(color).Radius(2)
					kit.ChartTooltip(view, mark, cell.tip)
				}
			}).Fill()
		})
		ui.Box(view).FillWidth().Height(24).Children(func() {
			previous := ""
			lastColumn := -2
			for _, cell := range source.Cells {
				if cell.Label == "" || cell.LabelGroup != nil && previous == *cell.LabelGroup {
					continue
				}
				if cell.LabelGroup != nil {
					previous = *cell.LabelGroup
				}
				if cell.Column-lastColumn < 2 {
					continue
				}
				lastColumn = cell.Column
				label := nativeText(view, cell.Label).FontSize(10).TextColor(palette.MutedForeground).Absolute().Top(4)
				if cell.Column == source.Columns-1 {
					label.Right(0)
				} else {
					label.LeftPercent(100 * float32(cell.Column) / float32(source.Columns))
				}
			}
		})
	})
}

type stackedChartSource struct {
	Series []struct {
		ID    string `json:"id"`
		Color string `json:"color"`
	} `json:"series"`
	Buckets []struct {
		Label   string     `json:"label"`
		Values  *[]float64 `json:"values"`
		Tooltip struct {
			Heading string           `json:"heading"`
			Rows    []nativeChartRow `json:"rows"`
			Footer  nativeChartRow   `json:"footer"`
		} `json:"tooltip"`
		// marks are the bar segments' labels, by series.
		marks []string
	} `json:"buckets"`
	Maximum float64 `json:"maximum"`
	Ticks   []struct {
		Value float64 `json:"value"`
		Label string  `json:"label"`
	} `json:"ticks"`
	Height       *float32 `json:"height"`
	height       float32
	scale        chart.Linear
	ticks, under []chart.Tick
}

func parseStackedChart(props map[string]json.RawMessage) (*stackedChartSource, error) {
	var source stackedChartSource
	body, _ := hostJSON(props)
	if json.Unmarshal(body, &source) != nil || !finiteNumber(source.Maximum) || source.Maximum < 0 || len(source.Buckets) > 10000 || len(source.Series) > 256 {
		return nil, hostInvalid("invalid stacked chart bounds")
	}
	source.height = float32(148)
	if source.Height != nil {
		source.height = *source.Height
	}
	if !finiteNumber(float64(source.height)) || source.height < 96 || source.height > 480 {
		return nil, hostInvalid("invalid stacked chart height")
	}
	for index := range source.Buckets {
		bucket := &source.Buckets[index]
		if len(bucket.Tooltip.Rows) != len(source.Series) {
			return nil, hostInvalid("stacked chart tooltip series differ")
		}
		if bucket.Values != nil {
			if len(*bucket.Values) != len(source.Series) {
				return nil, hostInvalid("stacked chart series differ")
			}
			for column, value := range *bucket.Values {
				if !finiteNumber(value) || value < 0 {
					return nil, hostInvalid("invalid stacked chart value")
				}
				bucket.marks = append(bucket.marks, fmt.Sprintf("%s %s", bucket.Label, bucket.Tooltip.Rows[column].Value))
			}
		}
	}
	source.scale = chart.NewLinear(0, max(source.Maximum, 1))
	source.ticks = []chart.Tick{}
	for _, tick := range source.Ticks {
		if !finiteNumber(tick.Value) {
			return nil, hostInvalid("invalid stacked chart tick")
		}
		source.ticks = append(source.ticks, chart.Tick{At: source.scale.At(tick.Value), Value: tick.Value, Label: tick.Label})
	}
	source.under = []chart.Tick{}
	for index, bucket := range source.Buckets {
		if bucket.Label != "" {
			source.under = append(source.under, chart.Tick{At: (float32(index) + 0.5) / float32(len(source.Buckets)), Label: bucket.Label})
		}
	}
	return &source, nil
}

func (binding *NativeBinding) renderStackedChart(view *ui.Context, source *stackedChartSource) ui.Element {
	scale, ticks, bottom, height := source.scale, source.ticks, source.under, source.height
	colors := make([]ui.Color, len(source.Series))
	for index, series := range source.Series {
		colors[index] = nativeChartColor(view, series.Color)
	}
	return chart.Plot(view, chart.Axes{Left: ticks, Bottom: bottom}, func(area ui.Element) {
		for _, tick := range ticks {
			chart.RuleY(view, tick.At, 1).Background(theme.Of(view).Border)
		}
		width := area.Bounds().W
		for index, bucket := range source.Buckets {
			if bucket.Values == nil {
				continue
			}
			band := float32(1) / float32(len(source.Buckets))
			bar := band * 0.8
			if width > 0 {
				bar = min(bar, 28/width)
			}
			middle := (float32(index) + 0.5) * band
			x := chart.Span{Lo: middle - bar/2, Hi: middle + bar/2}
			total := float64(0)
			tip := kit.ChartTip{Heading: bucket.Tooltip.Heading, Footer: &kit.ChartTipRow{Label: bucket.Tooltip.Footer.Label, Value: bucket.Tooltip.Footer.Value}}
			for column, row := range bucket.Tooltip.Rows {
				tip.Rows = append(tip.Rows, kit.ChartTipRow{Label: row.Label, Value: row.Value, Swatch: colors[column]})
			}
			for column, value := range *bucket.Values {
				mark := chart.Rect(view, x, scale.Span(total, total+value)).Background(colors[column])
				mark.Label(bucket.marks[column])
				kit.ChartTooltip(view, mark, tip)
				total += value
			}
			if total == 0 {
				chart.Rect(view, x, chart.Span{Lo: 0, Hi: 0.005}).Background(theme.Of(view).MutedForeground)
			}
		}
	}).Height(height).FillWidth().Gap(8)
}
