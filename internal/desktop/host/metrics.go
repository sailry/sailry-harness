package host

import (
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// summaryProcesses is how many of the busiest processes the summary lists.
const summaryProcesses = 5

// MetricsPanel is the host's metrics summary in the right column, titled by
// the window's header and closed until the user opens it: the preview's
// note, the host, then the processor and the memory as a value over a bar
// and the last minute's line, the network's rates over theirs, the real
// volumes each with how full it is, and the busiest
// processes. A loading bar holds its place until the first sample. A
// failure shows only as a toast.
func MetricsPanel(c *ui.Context, s *State, h *Host) {
	if h == nil {
		return
	}
	p := theme.Of(c)
	t := tonesOf(p)
	ui.Scroll(c.Key("metrics")).Grow(1).MinHeight(0).Padding(16).Gap(16).Children(func() {
		if !h.plain(c) {
			ui.Text(c, i18n.T(c, "metrics_preview")).FontSize(14).TextColor(p.MutedForeground)
		}
		ui.Text(c, h.label(c, h.Name)).FontSize(14).SingleLine()
		m := h.Metrics
		if h.Live && h.MetricsError != "" {
			m = nil
		}
		if m == nil {
			if h.MetricsError == "" {
				loading := "metrics_loading"
				if h.plain(c) {
					loading = "live_metrics_loading"
				}
				ui.Progress(c.Key("loading"), -1).FillWidth().Height(4).Label(i18n.T(c, loading))
				ui.Column(c.Key("processes")).Gap(12).Children(func() {
					ui.Text(c, i18n.T(c, "metrics_processes")).FontSize(14).FontWeight(600)
					processSkeleton(c)
				})
			}
			return
		}
		var cpu, memory, received, sent []float64
		for _, point := range m.History {
			cpu, memory = append(cpu, point.CPU), append(memory, point.Memory)
			received, sent = append(received, point.Received), append(sent, point.Transmitted)
		}
		meter(c, "cpu", i18n.T(c, "metrics_cpu"), orDash(Percent(m.CPU)), m.CPU, cpu, t.load(m.CPU, t.cpu))
		used, known := Usage(m.MemoryUsed, m.Memory)
		value := dash
		if known {
			value = Bytes(float64(m.MemoryUsed)) + " / " + Bytes(float64(m.Memory))
		} else {
			used = math.NaN()
		}
		meter(c, "memory", i18n.T(c, "metrics_memory"), value, used, memory, t.load(used, t.memory))
		down, up := math.NaN(), math.NaN()
		if m.Interval > 0 {
			down, up = float64(m.Received)/m.Interval.Seconds(), float64(m.Transmitted)/m.Interval.Seconds()
		}
		ui.Column(c.Key("network")).Gap(8).Children(func() {
			ui.Text(c, i18n.T(c, "metrics_network")).FontSize(14).FontWeight(600)
			top := ceiling(received, sent)
			fact(c, i18n.T(c, "metrics_download"), rate(c, down), t.download)
			fact(c, i18n.T(c, "metrics_upload"), rate(c, up), t.upload)
			areaChart(c, "chart", 40, top, nil, nil, i18n.T(c, "metrics_network"), series{received, t.download}, series{sent, t.upload})
		})
		ui.Column(c.Key("disks")).Gap(12).Children(func() {
			ui.Text(c, i18n.T(c, "metrics_disks")).FontSize(14).FontWeight(600)
			volumes := m.Disks
			if len(volumes) == 0 {
				kit.Result(c, kit.ResultProps{Icon: icons.HardDrive, Title: i18n.T(c, "metrics_no_disks"), Card: true})
			}
			for _, d := range volumes {
				ui.Column(c.Key(d.Mount)).Gap(4).Children(func() {
					space, percent := d.Used()
					fact(c, d.Mount, Percent(percent), ui.Color{})
					ui.Progress(c.Key("capacity"), percent/100).FillWidth().Height(4).Label(d.Mount).TextColor(t.capacity(percent))
					ui.Text(c, d.FileSystem+" · "+Bytes(float64(space))+" / "+Bytes(float64(d.Total))).
						FontSize(12).SingleLine().TextColor(p.MutedForeground)
				})
			}
		})
		ui.Column(c.Key("processes")).Gap(8).Children(func() {
			ui.Text(c, i18n.T(c, "metrics_processes")).FontSize(14).FontWeight(600)
			if len(m.Processes) == 0 {
				kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, "metrics_no_processes"), Card: true})
			}
			rows := SortProcesses(m.Processes, "", "cpu", true)
			for i, x := range rows[:min(len(rows), summaryProcesses)] {
				ui.Row(c.Key(i)).Gap(12).Children(func() {
					ui.Text(c, h.label(c, x.Name)).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
					ui.Text(c, orDash(Percent(x.CPU))).FontSize(12).Font("monospace").SingleLine().TextColor(p.MutedForeground)
					ui.Text(c, orDash(Bytes(float64(x.Memory)))).FontSize(12).Font("monospace").SingleLine().Width(72).TextAlign(ui.End).TextColor(p.MutedForeground)
				})
			}
		})
	})
	if !h.Live && h.MetricsError != "" && s.metricsFailed != h.MetricsError {
		key := "metrics_error"
		if h.plain(c) {
			key = "live_metrics_error"
		}
		kit.Error(c, i18n.T(c, key), h.MetricsError)
	}
	s.metricsFailed = h.MetricsError
}

// meter is a load in the summary: its label and value, a MyGo progress bar
// of its percentage, and the last minute's line under it.
func meter(c *ui.Context, key, label, value string, percent float64, history []float64, tone ui.Color) {
	ui.Column(c.Key(key)).Gap(8).Children(func() {
		fact(c, label, value, ui.Color{})
		if !math.IsNaN(percent) {
			ui.Progress(c.Key("bar"), min(max(percent, 0), 100)/100).FillWidth().Height(4).Label(label).TextColor(tone)
		}
		areaChart(c, "chart", 32, 100, nil, nil, label, series{history, tone})
	})
}

// fact is a metric's row: the label, after its series' mark when tone is
// one, and the value at the end.
func fact(c *ui.Context, label, value string, tone ui.Color) {
	ui.Row(c.Key(label)).Gap(8).AlignItems(ui.Center).Children(func() {
		if tone != (ui.Color{}) {
			tick(c, tone)
		}
		ui.Text(c, label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		ui.Text(c, value).FontSize(14).Font("monospace").SingleLine()
	})
}
