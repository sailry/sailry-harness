package host

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// MetricsPanel is the host's metrics in the right column, titled by the
// window's header: the preview's note, the host, then the processor and memory, the network, the
// disks and the busiest processes; a loading bar until the first sample. A
// failure shows only as a toast.
func MetricsPanel(c *ui.Context, s *State, h *Host) {
	if h == nil {
		return
	}
	p := theme.Of(c)
	ui.Scroll(c.Key("metrics")).Grow(1).MinHeight(0).Padding(16).Gap(16).Children(func() {
		if !h.Live {
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
				if h.Live {
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
			used := unknown
			if m.Memory > 0 {
				total = Bytes(float64(m.Memory))
				used = or(Bytes(float64(m.MemoryUsed)))
			}
			fact(c, i18n.T(c, "metrics_memory"), used+" / "+total)
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
				kit.Result(c, kit.ResultProps{Icon: icons.HardDrive, Title: i18n.T(c, "metrics_no_disks"), Card: true})
			}
			for i, d := range m.Disks {
				ui.Column(c.Key(i)).Gap(4).Children(func() {
					ui.Text(c, d.Mount).FontSize(14).SingleLine()
					ui.Text(c, d.FileSystem+" / "+or(Bytes(float64(d.Total-min(d.Available, d.Total))))+" / "+or(Bytes(float64(d.Total)))).
						FontSize(12).SingleLine().TextColor(p.MutedForeground)
					readLabel, writtenLabel := "metrics_read", "metrics_written"
					read, written := or(Bytes(float64(d.ReadBytes))), or(Bytes(float64(d.Written)))
					if h.Live {
						readLabel, writtenLabel = "live_metrics_read", "live_metrics_written"
						read, written = i18n.T(c, "metrics_rate", "value", read), i18n.T(c, "metrics_rate", "value", written)
					}
					fact(c, i18n.T(c, readLabel), read)
					fact(c, i18n.T(c, writtenLabel), written)
				})
			}
		})
		ui.Column(c.Key("processes")).Gap(12).Children(func() {
			ui.Text(c, i18n.T(c, "metrics_processes")).FontSize(14).FontWeight(600)
			if len(m.Processes) == 0 {
				kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, "metrics_no_processes"), Card: true})
			}
			for i, x := range m.Processes {
				ui.Column(c.Key("process-" + strconv.Itoa(i))).Gap(4).Children(func() {
					ui.Text(c, h.label(c, x.Name)).FontSize(14).SingleLine()
					ui.Row(c).Gap(12).Children(func() {
						ui.Text(c, or(Percent(x.CPU))).FontSize(12).TextColor(p.MutedForeground)
						ui.Text(c, or(Bytes(float64(x.Memory)))).FontSize(12).TextColor(p.MutedForeground)
					})
				})
			}
		})
	})
	if !h.Live && h.MetricsError != "" && s.metricsFailed != h.MetricsError {
		kit.Error(c, i18n.T(c, "metrics_error"), h.MetricsError)
	}
	s.metricsFailed = h.MetricsError
}

// processSkeleton holds the processes table's place until the first sample:
// five 32 DIP rows, the first on the table head, of three blocks and one at
// the end, half a row high.
func processSkeleton(c *ui.Context) {
	p := theme.Of(c)
	const row = 32
	block := func(w float32) { kit.Skeleton(c, kit.SkeletonProps{Width: w, Height: row / 2}) }
	ui.Column(c.Key("process-skeleton")).FillWidth().Busy(true).Children(func() {
		for i := range 5 {
			r := ui.Row(c.Key(i)).Height(row).PaddingX(12).Gap(12).ClipX().Justify(ui.SpaceBetween)
			if i == 0 {
				r.Background(p.TableHead)
			} else {
				r.BorderWidth(1, 0, 0, 0).BorderColor(p.TableRowBorder)
			}
			r.Children(func() {
				ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(12).ClipX().Children(func() {
					block(96)
					block(192)
					block(64)
				})
				block(96)
			})
		}
	})
}

// fact is a metric's row: the label, the value after it.
func fact(c *ui.Context, label, value string) {
	ui.Row(c.Key(label)).Gap(12).Children(func() {
		ui.Text(c, label).FontSize(14).Grow(1).Shrink(1)
		ui.Text(c, value).FontSize(14).SingleLine()
	})
}
