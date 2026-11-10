package sidebar

import (
	"fmt"
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type Metrics struct {
	CPU, Memory *float64
}

func metricsFooter(c *ui.Context, metrics *Metrics) {
	if metrics == nil {
		return
	}
	palette := theme.Of(c)
	ui.Row(c.Key("sidebar-metrics")).Shrink(0).Padding(8, 12).Gap(12).BorderWidth(1, 0, 0, 0).BorderColor(palette.Border).Children(func() {
		for _, metric := range []struct {
			key   string
			value *float64
		}{{"metrics_cpu", metrics.CPU}, {"metrics_memory", metrics.Memory}} {
			value, text := 0.0, "—"
			if metric.value != nil && !math.IsNaN(*metric.value) && !math.IsInf(*metric.value, 0) {
				value = min(max(*metric.value, 0), 100)
				text = fmt.Sprintf("%.0f%%", value)
			}
			label := i18n.T(c, metric.key)
			ui.Column(c.Key(metric.key)).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
				ui.Row(c).Justify(ui.SpaceBetween).Gap(4).Children(func() {
					ui.Text(c, label).FontSize(12).TextColor(palette.MutedForeground).SingleLine()
					ui.Text(c, text).FontSize(12).TextColor(palette.MutedForeground).SingleLine()
				})
				ui.Progress(c.Key("progress"), value/100).FillWidth().Height(3).Label(label + " · " + text)
			})
		}
	})
}
