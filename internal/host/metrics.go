package host

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// MetricsPanel is the host's metrics in the right column
// (workspace/host/metrics.rs): a header, the preview's note, the host,
// then the processor and memory, the network, the disks and the busiest
// processes; a loading bar until the first sample. A failure shows only
// as a toast.
func MetricsPanel(c *ui.Context, s *State, h *Host) {
	if h == nil {
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("metrics-header")).Height(40).Shrink(0).PaddingX(12).Gap(8).Children(func() {
		ui.Icon(c, icons.ChartPie).FontSize(16)
		ui.Text(c, i18n.T(c, "metrics_title")).FontSize(14).SingleLine().Grow(1).Shrink(1)
	})
	ui.Scroll(c.Key("metrics")).Grow(1).MinHeight(0).Padding(16).Gap(16).Children(func() {
		ui.Text(c, i18n.T(c, "metrics_preview")).FontSize(14).TextColor(p.MutedForeground)
		ui.Text(c, i18n.T(c, h.Name)).FontSize(14).SingleLine()
		m := h.Metrics
		if m == nil {
			if h.MetricsError == "" {
				ui.Progress(c.Key("loading"), -1).FillWidth().Height(4).Label(i18n.T(c, "metrics_loading"))
			}
			return
		}
		unknown := i18n.T(c, "host_unknown")
		or := func(v string) string {
			if v == "" {
				return unknown
			}
			return v
		}
		ui.Column(c.Key("load")).Gap(8).Children(func() {
			fact(c, i18n.T(c, "metrics_cpu"), or(Percent(m.CPU)))
			total := unknown
			if m.Memory > 0 {
				total = Bytes(float64(m.Memory))
			}
			fact(c, i18n.T(c, "metrics_memory"), or(Bytes(float64(m.MemoryUsed)))+" / "+total)
			if v, ok := Usage(m.MemoryUsed, m.Memory); ok {
				ui.Progress(c.Key("memory"), v/100).FillWidth().Height(4).Label(i18n.T(c, "metrics_memory"))
			}
		})
		rate := func(v uint64) string {
			if r := Rate(v, m.Interval); r != "" {
				return i18n.T(c, "metrics_rate", "value", r)
			}
			return unknown
		}
		ui.Column(c.Key("network")).Gap(8).Children(func() {
			ui.Text(c, i18n.T(c, "metrics_network")).FontSize(14).FontWeight(600)
			fact(c, i18n.T(c, "metrics_download"), rate(m.Received))
			fact(c, i18n.T(c, "metrics_upload"), rate(m.Transmitted))
		})
		ui.Column(c.Key("disks")).Gap(12).Children(func() {
			ui.Text(c, i18n.T(c, "metrics_disks")).FontSize(14).FontWeight(600)
			if len(m.Disks) == 0 {
				kit.EmptyGroup(c, icons.HardDrive, i18n.T(c, "metrics_no_disks"))
			}
			for i, d := range m.Disks {
				ui.Column(c.Key(i)).Gap(4).Children(func() {
					ui.Text(c, d.Mount).FontSize(14).SingleLine()
					ui.Text(c, d.FileSystem+" / "+or(Bytes(float64(d.Total-min(d.Available, d.Total))))+" / "+or(Bytes(float64(d.Total)))).
						FontSize(12).SingleLine().TextColor(p.MutedForeground)
					fact(c, i18n.T(c, "metrics_read"), or(Bytes(float64(d.ReadBytes))))
					fact(c, i18n.T(c, "metrics_written"), or(Bytes(float64(d.Written))))
				})
			}
		})
		ui.Column(c.Key("processes")).Gap(12).Children(func() {
			ui.Text(c, i18n.T(c, "metrics_processes")).FontSize(14).FontWeight(600)
			if len(m.Processes) == 0 {
				kit.EmptyGroup(c, icons.Cpu, i18n.T(c, "metrics_no_processes"))
			}
			for i, x := range m.Processes {
				ui.Column(c.Key("process-" + strconv.Itoa(i))).Gap(4).Children(func() {
					ui.Text(c, i18n.T(c, x.Name)).FontSize(14).SingleLine()
					ui.Row(c).Gap(12).Children(func() {
						ui.Text(c, or(Percent(x.CPU))).FontSize(12).TextColor(p.MutedForeground)
						ui.Text(c, or(Bytes(float64(x.Memory)))).FontSize(12).TextColor(p.MutedForeground)
					})
				})
			}
		})
	})
	if h.MetricsError != "" && s.metricsFailed != h.MetricsError {
		kit.Error(c, i18n.T(c, "metrics_error"), h.MetricsError)
	}
	s.metricsFailed = h.MetricsError
}

// fact is a metric's row: the label, the value after it.
func fact(c *ui.Context, label, value string) {
	ui.Row(c.Key(label)).Gap(12).Children(func() {
		ui.Text(c, label).FontSize(14).Grow(1).Shrink(1)
		ui.Text(c, value).FontSize(14).SingleLine()
	})
}
