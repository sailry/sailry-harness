package plugins

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/chart"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type nativeChartRow struct {
	Label string `json:"label"`
	Value string `json:"value"`
}

func nativeChartColor(view *ui.Context, name string) ui.Color {
	var snapshot struct {
		Colors map[string]string `json:"colors"`
	}
	if json.Unmarshal(ThemeSnapshot(theme.Of(view)), &snapshot) == nil {
		if color, err := nativeColor(snapshot.Colors[name]); err == nil {
			return color
		}
	}
	return theme.Of(view).Primary
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
		return binding.renderHeatmap(view, props)
	case "StackedChart":
		return binding.renderStackedChart(view, props)
	}
	return ui.Element{}, hostInvalid("unknown native chart")
}

func (binding *NativeBinding) renderHeatmap(view *ui.Context, props map[string]json.RawMessage) (ui.Element, error) {
	var source struct {
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
		} `json:"cells"`
	}
	body, _ := hostJSON(props)
	if json.Unmarshal(body, &source) != nil || source.Rows <= 0 || source.Columns <= 0 || len(source.Cells) > 100000 {
		return ui.Element{}, hostInvalid("invalid heatmap bounds")
	}
	for _, cell := range source.Cells {
		if cell.Row < 0 || cell.Row >= source.Rows || cell.Column < 0 || cell.Column >= source.Columns || !finiteNumber(float64(cell.Intensity)) || cell.Intensity < 0 || cell.Intensity > 1 {
			return ui.Element{}, hostInvalid("invalid heatmap cell")
		}
	}
	return ui.Column(view).FillWidth().Children(func() {
		ui.Box(view).FillWidth().AspectRatio(float32(source.Columns) / float32(source.Rows)).MaxHeight(float32(source.Rows) * 24).Children(func() {
			chart.Plot(view, chart.Axes{}, func(area ui.Element) {
				for _, cell := range source.Cells {
					color := theme.Of(view).Muted
					if cell.Intensity > 0 {
						color = nativeChartColor(view, "chart_2").Alpha(cell.Intensity)
					}
					x := chart.Span{Lo: float32(cell.Column) / float32(source.Columns), Hi: (float32(cell.Column) + 0.8) / float32(source.Columns)}
					y := chart.Span{Lo: 1 - (float32(cell.Row)+0.8)/float32(source.Rows), Hi: 1 - float32(cell.Row)/float32(source.Rows)}
					mark := chart.Rect(view, x, y).Background(color).Radius(2)
					lines := []string{cell.Tooltip.Heading, cell.Tooltip.Title}
					for _, row := range cell.Tooltip.Rows {
						lines = append(lines, row.Label+" "+row.Value)
					}
					kit.Tooltip(view, mark, strings.Join(lines, "\n"))
				}
			}).Fill()
		})
		ui.Row(view).FillWidth().Height(28).Children(func() {
			previous := ""
			for _, cell := range source.Cells {
				if cell.Label == "" || cell.LabelGroup != nil && previous == *cell.LabelGroup {
					continue
				}
				if cell.LabelGroup != nil {
					previous = *cell.LabelGroup
				}
				nativeText(view, cell.Label).FontSize(10).TextColor(theme.Of(view).MutedForeground).Grow(1)
			}
		})
	}), nil
}

func (binding *NativeBinding) renderStackedChart(view *ui.Context, props map[string]json.RawMessage) (ui.Element, error) {
	var source struct {
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
		} `json:"buckets"`
		Maximum float64 `json:"maximum"`
		Ticks   []struct {
			Value float64 `json:"value"`
			Label string  `json:"label"`
		} `json:"ticks"`
		Height *float32 `json:"height"`
	}
	body, _ := hostJSON(props)
	if json.Unmarshal(body, &source) != nil || !finiteNumber(source.Maximum) || source.Maximum < 0 || len(source.Buckets) > 10000 || len(source.Series) > 256 {
		return ui.Element{}, hostInvalid("invalid stacked chart bounds")
	}
	height := float32(148)
	if source.Height != nil {
		height = *source.Height
	}
	if !finiteNumber(float64(height)) || height < 96 || height > 480 {
		return ui.Element{}, hostInvalid("invalid stacked chart height")
	}
	for _, bucket := range source.Buckets {
		if len(bucket.Tooltip.Rows) != len(source.Series) {
			return ui.Element{}, hostInvalid("stacked chart tooltip series differ")
		}
		if bucket.Values != nil {
			if len(*bucket.Values) != len(source.Series) {
				return ui.Element{}, hostInvalid("stacked chart series differ")
			}
			for _, value := range *bucket.Values {
				if !finiteNumber(value) || value < 0 {
					return ui.Element{}, hostInvalid("invalid stacked chart value")
				}
			}
		}
	}
	scale := chart.NewLinear(0, max(source.Maximum, 1))
	ticks := []chart.Tick{}
	for _, tick := range source.Ticks {
		if !finiteNumber(tick.Value) {
			return ui.Element{}, hostInvalid("invalid stacked chart tick")
		}
		ticks = append(ticks, chart.Tick{At: scale.At(tick.Value), Value: tick.Value, Label: tick.Label})
	}
	bottom := []chart.Tick{}
	for index, bucket := range source.Buckets {
		if bucket.Label != "" {
			bottom = append(bottom, chart.Tick{At: (float32(index) + 0.5) / float32(len(source.Buckets)), Label: bucket.Label})
		}
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
			tooltip := []string{bucket.Tooltip.Heading}
			for _, row := range bucket.Tooltip.Rows {
				tooltip = append(tooltip, row.Label+" "+row.Value)
			}
			tooltip = append(tooltip, bucket.Tooltip.Footer.Label+" "+bucket.Tooltip.Footer.Value)
			for column, value := range *bucket.Values {
				mark := chart.Rect(view, x, scale.Span(total, total+value)).Background(nativeChartColor(view, source.Series[column].Color))
				mark.Label(fmt.Sprintf("%s %s", bucket.Label, bucket.Tooltip.Rows[column].Value))
				kit.Tooltip(view, mark, strings.Join(tooltip, "\n"))
				total += value
			}
			if total == 0 {
				chart.Rect(view, x, chart.Span{Lo: 0, Hi: 0.005}).Background(theme.Of(view).MutedForeground)
			}
		}
	}).Height(height).FillWidth().Gap(8), nil
}
