package host

import (
	"fmt"
	"math"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// monitor is the host's live load, as the reference's Monitor
// (live/host/monitor.rs): the processor with its cores, the accelerator
// where the Node reads one, the memory, the utilization over the last minute, the network beside the chosen volume's
// transfers, the storage and the processes, 14 apart. Until the first sample
// its cards hold their place; a failure shows as a warning over them and
// leaves the last sample out.
func monitor(c *ui.Context, s *State, h *Host, act Actions) {
	p := theme.Of(c)
	column := ui.Column(c.Key("monitor")).Gap(sectionGap)
	// The room inside a card, as laid out last: the cards that put two
	// blocks side by side divide them only while both fit.
	room := column.Bounds().W - 2*panelPad
	column.Children(func() {
		if !h.plain(c) {
			ui.Text(c.Key("preview"), i18n.T(c, "metrics_preview")).FontSize(14).TextColor(p.MutedForeground)
		}
		m := h.Metrics
		if h.Live && h.MetricsError != "" {
			m = nil
			kit.Alert(c, "sampling", kit.Caution, i18n.T(c, "host_sampling_failed"), nil)
		}
		if m == nil {
			if h.MetricsError == "" {
				pending(c, h)
			}
			return
		}
		t := tonesOf(p)
		cpuCard(c, m, t, room)
		gpuCard(c, s, h, m, t, room)
		memoryCard(c, m, t, room)
		trendCard(c, s, m, t)
		ui.Row(c.Key("transfers")).Wrap().Gap(sectionGap).AlignItems(ui.Stretch).Children(func() {
			networkCard(c, m, t)
			diskCard(c, s, h, m, t)
		})
		storageCard(c, s, h, m, t)
		processPanel(c, s, h, m, t, act)
	})
}

// sectionGap is the space between the page's cards (the reference's
// gap_3p5).
const sectionGap = 14

// pending holds the monitor's place until the first sample: the processor's
// and the memory's cards as blocks, then the processes' table.
func pending(c *ui.Context, h *Host) {
	loading := "metrics_loading"
	if h.plain(c) {
		loading = "live_metrics_loading"
	}
	label := i18n.T(c, loading)
	for _, key := range []string{"cpu", "memory"} {
		panel(c, key, false, func() {
			ui.Row(c).Gap(blockGap).Busy(true).Label(label).Children(func() {
				ui.Column(c).Width(300).Gap(8).Children(func() {
					kit.Skeleton(c, kit.SkeletonProps{Width: 72, Height: 12})
					kit.Skeleton(c, kit.SkeletonProps{Width: 128, Height: 32})
					kit.Skeleton(c, kit.SkeletonProps{Width: 160, Height: 12, Secondary: true})
				})
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(8).Children(func() {
					for range 3 {
						kit.Skeleton(c, kit.SkeletonProps{Height: 12, Secondary: true})
					}
				})
			})
		})
	}
	panel(c, "processes", false, func() {
		title(c, i18n.T(c, "metrics_processes"))
		processSkeleton(c)
	})
}

// panelPad is a card's padding at each side: Kit's Card's 16 and the
// content's 4.
const panelPad = 20

// panel is one of the monitor's cards: Kit's Card, its content padded 20
// and 14 apart. A half shares its row with another and wraps under it below
// 380 each.
func panel(c *ui.Context, key string, half bool, body func()) ui.Element {
	box := ui.Column(c.Key(key)).MinWidth(0)
	if half {
		box.Grow(1).Shrink(1).Basis(380)
	}
	return box.Children(func() {
		kit.Card(c, func() {
			ui.Column(c).Padding(20, 4).Gap(sectionGap).Children(body)
		}).Grow(1)
	})
}

// tick is a series' mark before its label: 3 by 12 in its colour.
func tick(c *ui.Context, tone ui.Color) {
	ui.Box(c).Size(3, 12).Shrink(0).Radius(2).Background(tone)
}

// title is a card's title.
func title(c *ui.Context, text string) ui.Element {
	return ui.Text(c, text).FontSize(14).FontWeight(600).SingleLine().TextColor(theme.Of(c).SurfaceTitle)
}

// heading is a metric's label after its series' mark.
func heading(c *ui.Context, label string, tone ui.Color) {
	p := theme.Of(c)
	ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
		tick(c, tone)
		ui.Text(c, label).FontSize(12).SingleLine().TextColor(p.MutedForeground)
	})
}

// figure is a card's main value, 32 bold in the code font, and its unit
// after it.
func figure(c *ui.Context, value, unit string) {
	p := theme.Of(c)
	ui.Row(c).Gap(4).AlignItems(ui.Baseline).Children(func() {
		ui.Text(c, value).FontSize(32).FontWeight(700).Font("monospace").SingleLine().LineHeight(1.2)
		if unit != "" {
			ui.Text(c, unit).FontSize(14).SingleLine().TextColor(p.MutedForeground)
		}
	})
}

// legend is a series' mark, name and value on one line.
func legend(c *ui.Context, label, value string, tone ui.Color) {
	p := theme.Of(c)
	ui.Row(c.Key(label)).Gap(6).AlignItems(ui.Center).Children(func() {
		tick(c, tone)
		ui.Text(c, label).FontSize(12).SingleLine().TextColor(p.MutedForeground)
		ui.Text(c, value).FontSize(12).Font("monospace").SingleLine()
	})
}

// stat is a metric of a card: its heading over its value, and a muted total
// under it, "" for none.
func stat(c *ui.Context, label, value, total string, tone ui.Color) {
	p := theme.Of(c)
	ui.Column(c.Key(label)).Grow(1).Shrink(1).Basis(0).MinWidth(0).Gap(2).Children(func() {
		heading(c, label, tone)
		ui.Text(c, value).FontSize(18).Font("monospace").SingleLine()
		if total != "" {
			ui.Text(c, total).FontSize(12).Font("monospace").SingleLine().TextColor(p.MutedForeground)
		}
	})
}

// dash stands for a value the sample does not have.
const dash = "—"

func orDash(v string) string {
	if v == "" {
		return dash
	}
	return v
}

// The cards' two blocks: the summary's width, the detail's least width
// beside it, and the space between them.
const (
	summaryBasis = 360
	detailBasis  = 440
	blockGap     = 28
)

// detail is a card's second block: beside the summary, after a divider,
// while the card's room holds both; under it, without one, once they wrap.
// Room not yet laid out counts as wide.
func detail(c *ui.Context, key string, room float32) ui.Element {
	e := ui.Column(c.Key(key)).Grow(1).Shrink(1).Basis(detailBasis).MinWidth(0)
	if room <= 0 || room >= summaryBasis+blockGap+detailBasis {
		e.PaddingStart(blockGap).BorderWidth(0, 0, 0, 1).BorderColor(theme.Of(c).Border)
	}
	return e
}

// cpuCard is the processor: its load, the cores and the load averages
// beside their rings, then every core's load after a divider.
func cpuCard(c *ui.Context, m *Metrics, t tones, room float32) {
	p := theme.Of(c)
	panel(c, "cpu", false, func() {
		ui.Row(c).Wrap().GapX(blockGap).GapY(20).AlignItems(ui.Center).Children(func() {
			ui.Row(c.Key("load")).Basis(summaryBasis).Shrink(1).MinWidth(0).Gap(16).AlignItems(ui.Center).Children(func() {
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(2).Children(func() {
					heading(c, i18n.T(c, "metrics_cpu"), t.cpu)
					value, unit := dash, ""
					if !math.IsNaN(m.CPU) {
						value, unit = fmt.Sprintf("%.1f", m.CPU), "%"
					}
					figure(c, value, unit)
					ui.Row(c.Key("detail")).Wrap().GapX(16).GapY(2).Children(func() {
						if len(m.Cores) > 0 {
							ui.Text(c, i18n.T(c, "host_cores", "count", len(m.Cores))).FontSize(12).SingleLine().TextColor(p.MutedForeground)
						}
						if m.Load != nil {
							ui.Row(c).Gap(4).Children(func() {
								ui.Text(c, i18n.T(c, "host_load")).FontSize(12).SingleLine().TextColor(p.MutedForeground)
								ui.Text(c, Load(m.Load[0])+"  "+Load(m.Load[1])+"  "+Load(m.Load[2])).FontSize(12).Font("monospace").SingleLine()
							})
						}
					})
				})
				if m.Load != nil && len(m.Cores) > 0 {
					var arcs []arc
					for _, v := range m.Load {
						share := min(v/float64(len(m.Cores)), 1)
						arcs = append(arcs, arc{share, t.load(share*100, t.cpu)})
					}
					rings(c, 84, 7, 3, t.track, i18n.T(c, "host_load"), arcs...)
				}
			})
			detail(c, "cores", room).Children(func() {
				if len(m.Cores) == 0 {
					ui.Text(c, i18n.T(c, "host_sampling")).FontSize(12).TextColor(p.MutedForeground)
					return
				}
				cores(c, m.Cores, t)
			})
		})
	})
}

// cores are the logical processors' loads as MyGo progress bars: one column
// up to four, two up to sixteen, four past that, each with its number and
// its percentage.
func cores(c *ui.Context, loads []float64, t tones) {
	p := theme.Of(c)
	columns := 4
	switch {
	case len(loads) <= 4:
		columns = 1
	case len(loads) <= 16:
		columns = 2
	}
	ui.Row(c).Gap(20).AlignItems(ui.Start).Children(func() {
		for col := range columns {
			ui.Column(c.Key(col)).Grow(1).Shrink(1).Basis(0).MinWidth(0).Gap(6).Children(func() {
				for i := col; i < len(loads); i += columns {
					v := loads[i]
					name := i18n.T(c, "host_core", "index", i+1, "value", Percent(v))
					ui.Row(c.Key(i)).Gap(8).AlignItems(ui.Center).Children(func() {
						ui.Text(c, fmt.Sprintf("%02d", i+1)).FontSize(10).Font("monospace").SingleLine().Width(16).TextColor(p.MutedForeground)
						ui.Progress(c.Key("bar"), min(max(v, 0), 100)/100).Grow(1).Shrink(1).MinWidth(0).Height(4).Label(name).TextColor(t.load(v, t.cpu))
						if columns < 4 {
							ui.Text(c, strconv.Itoa(int(math.Round(v)))+"%").FontSize(10).Font("monospace").SingleLine().Width(30).TextAlign(ui.End).TextColor(p.MutedForeground)
						}
					})
				}
			})
		}
	})
}

// gpuCard is the accelerator's load beside its ring, with its name, then the
// load over the last minute. A host with several chooses the one the card
// and the utilization follow. A sample without one has no such card: the
// Node reads only macOS's.
func gpuCard(c *ui.Context, s *State, h *Host, m *Metrics, t tones, room float32) {
	g, ok := m.GPU(s.gpu)
	if !ok {
		return
	}
	s.gpu = g.ID
	p := theme.Of(c)
	label := i18n.T(c, "metrics_gpu")
	panel(c, "gpu", false, func() {
		ui.Row(c).Wrap().GapX(blockGap).GapY(20).AlignItems(ui.Center).Children(func() {
			ui.Row(c.Key("load")).Basis(summaryBasis).Shrink(1).MinWidth(0).Gap(16).AlignItems(ui.Center).Children(func() {
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(2).Children(func() {
					heading(c, label, t.gpu)
					value, unit := dash, ""
					if !math.IsNaN(g.Usage) {
						value, unit = fmt.Sprintf("%.1f", g.Usage), "%"
					}
					figure(c, value, unit)
					if len(m.GPUs) > 1 {
						names, ids := make([]string, len(m.GPUs)), make([]string, len(m.GPUs))
						for i, x := range m.GPUs {
							names[i], ids[i] = x.Name, x.ID
						}
						kit.Select(c.Key("choice"), "host-gpu-"+h.ID, label, &s.gpu, names, false, ids)
					} else {
						ui.Text(c, g.Name).FontSize(12).SingleLine().TextColor(p.MutedForeground)
					}
				})
				if !math.IsNaN(g.Usage) {
					ring(c, 84, 9, t.track, label, func() {
						ui.Text(c, strconv.Itoa(int(math.Round(g.Usage)))+"%").FontSize(14).FontWeight(600).Font("monospace").SingleLine()
					}, arc{g.Usage / 100, t.load(g.Usage, t.gpu)})
				}
			})
			detail(c, "history", room).Children(func() {
				areaChart(c, "chart", 56, 100, nil, nil, label, series{gpuHistory(m, g.ID), t.gpu})
			})
		})
	})
}

// gpuHistory is an accelerator's load over the history, NaN where a sample
// had none.
func gpuHistory(m *Metrics, id string) []float64 {
	loads := make([]float64, len(m.History))
	for i, point := range m.History {
		v, sampled := point.GPUs[id]
		if !sampled {
			v = math.NaN()
		}
		loads[i] = v
	}
	return loads
}

// memoryCard is the memory: what is used of the whole beside its ring of
// used and cached, then the used, cached and swap sizes over the last
// minute's use.
func memoryCard(c *ui.Context, m *Metrics, t tones, room float32) {
	p := theme.Of(c)
	panel(c, "memory", false, func() {
		ui.Row(c).Wrap().GapX(blockGap).GapY(20).AlignItems(ui.Center).Children(func() {
			ui.Row(c.Key("use")).Basis(summaryBasis).Shrink(1).MinWidth(0).Gap(16).AlignItems(ui.Center).Children(func() {
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(2).Children(func() {
					heading(c, i18n.T(c, "metrics_memory"), t.memory)
					if m.Memory == 0 {
						figure(c, dash, "")
						ui.Text(c, i18n.T(c, "host_metric_unavailable")).FontSize(12).TextColor(p.MutedForeground)
						return
					}
					value, unit := Scaled(float64(m.MemoryUsed))
					figure(c, value, unit)
					ui.Text(c, i18n.T(c, "host_memory_total", "value", Bytes(float64(m.Memory)))).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				})
				if used, ok := Usage(m.MemoryUsed, m.Memory); ok {
					cached, _ := Usage(m.MemoryCached, m.Memory)
					ring(c, 84, 9, t.track, i18n.T(c, "metrics_memory"), func() {
						ui.Text(c, strconv.Itoa(int(math.Round(used)))+"%").FontSize(14).FontWeight(600).Font("monospace").SingleLine()
					}, arc{used / 100, t.load(used, t.memory)}, arc{cached / 100, t.cache})
				}
			})
			detail(c, "history", room).Gap(12).Children(func() {
				ui.Row(c.Key("legend")).Wrap().GapX(20).GapY(6).Children(func() {
					legend(c, i18n.T(c, "host_memory_used"), orDash(Bytes(float64(m.MemoryUsed))), t.memory)
					legend(c, i18n.T(c, "host_memory_cached"), orDash(Bytes(float64(m.MemoryCached))), t.cache)
					swap := dash
					if m.Swap > 0 {
						swap = Bytes(float64(m.SwapUsed)) + " / " + Bytes(float64(m.Swap))
					}
					legend(c, i18n.T(c, "host_memory_swap"), swap, t.swap)
				})
				var use []float64
				for _, point := range m.History {
					use = append(use, point.Memory)
				}
				areaChart(c, "chart", 56, 100, nil, nil, i18n.T(c, "metrics_memory"), series{use, t.memory})
			})
		})
	})
}

// trendCard is the processor's and the memory's use over the last minute,
// from 0 to 100%, the samples' times under it, with the chosen
// accelerator's between them where the sample has one.
func trendCard(c *ui.Context, s *State, m *Metrics, t tones) {
	panel(c, "trend", false, func() {
		var cpu, memory []float64
		for _, point := range m.History {
			cpu, memory = append(cpu, point.CPU), append(memory, point.Memory)
		}
		g, hasGPU := m.GPU(s.gpu)
		ui.Row(c.Key("head")).Wrap().GapX(20).GapY(6).AlignItems(ui.Center).Children(func() {
			title(c, i18n.T(c, "host_utilization")).Grow(1)
			legend(c, i18n.T(c, "metrics_cpu"), orDash(Percent(m.CPU)), t.cpu)
			if hasGPU {
				legend(c, i18n.T(c, "metrics_gpu"), orDash(Percent(g.Usage)), t.gpu)
			}
			used, ok := Usage(m.MemoryUsed, m.Memory)
			value := dash
			if ok {
				value = Percent(used)
			}
			legend(c, i18n.T(c, "metrics_memory"), value, t.memory)
		})
		all := []series{{cpu, t.cpu}, {memory, t.memory}}
		if hasGPU {
			all = []series{{cpu, t.cpu}, {gpuHistory(m, g.ID), t.gpu}, {memory, t.memory}}
		}
		areaChart(c, "chart", 180, 100, percentTicks(), clockTicks(m.History, 5), i18n.T(c, "host_utilization"), all...)
	})
}

// rate writes bytes a second, the dash for none.
func rate(c *ui.Context, v float64) string {
	if text := Bytes(v); text != "" {
		return i18n.T(c, "metrics_rate", "value", text)
	}
	return dash
}

// networkCard is the network: the download and upload rates over their
// totals since the computer started, then both over the last minute.
func networkCard(c *ui.Context, m *Metrics, t tones) {
	p := theme.Of(c)
	panel(c, "network", true, func() {
		ui.Row(c.Key("head")).Gap(8).AlignItems(ui.Center).Children(func() {
			title(c, i18n.T(c, "metrics_network")).Grow(1)
			if m.Totals {
				ui.Text(c, i18n.T(c, "host_network_total")).FontSize(12).SingleLine().TextColor(p.MutedForeground)
			}
		})
		down, up := math.NaN(), math.NaN()
		if m.Interval > 0 {
			down, up = float64(m.Received)/m.Interval.Seconds(), float64(m.Transmitted)/m.Interval.Seconds()
		}
		var totalDown, totalUp string
		if m.Totals {
			totalDown, totalUp = Bytes(float64(m.ReceivedTotal)), Bytes(float64(m.TransmittedTotal))
		}
		ui.Row(c.Key("stats")).Gap(20).Children(func() {
			stat(c, i18n.T(c, "metrics_download"), rate(c, down), totalDown, t.download)
			stat(c, i18n.T(c, "metrics_upload"), rate(c, up), totalUp, t.upload)
		})
		var received, sent []float64
		for _, point := range m.History {
			received, sent = append(received, point.Received), append(sent, point.Transmitted)
		}
		top := ceiling(received, sent)
		areaChart(c, "chart", 96, top, rateTicks(top), nil, i18n.T(c, "metrics_network"), series{received, t.download}, series{sent, t.upload})
	})
}

// diskCard is the chosen volume's transfers: its mount, the write and read
// rates, then both over the last minute. The storage card chooses the
// volume.
func diskCard(c *ui.Context, s *State, h *Host, m *Metrics, t tones) {
	p := theme.Of(c)
	panel(c, "disk", true, func() {
		d, ok := m.Volume(s.disk)
		ui.Row(c.Key("head")).Gap(8).AlignItems(ui.Center).Children(func() {
			title(c, i18n.T(c, "metrics_disks")).Grow(1)
			if ok {
				kit.Tag(c, d.Mount)
			}
		})
		if !ok {
			ui.Text(c, i18n.T(c, "metrics_no_disks")).FontSize(12).TextColor(p.MutedForeground)
			return
		}
		ui.Row(c.Key("stats")).Gap(20).Children(func() {
			stat(c, i18n.T(c, "host_disk_write"), transfer(c, h, d.Written), "", t.write)
			stat(c, i18n.T(c, "host_disk_read"), transfer(c, h, d.ReadBytes), "", t.read)
		})
		var read, written []float64
		for _, point := range m.History {
			v, sampled := point.Disks[d.Mount]
			if !sampled {
				v = [2]float64{math.NaN(), math.NaN()}
			}
			read, written = append(read, v[0]), append(written, v[1])
		}
		top := ceiling(read, written)
		areaChart(c, "chart", 96, top, rateTicks(top), nil, i18n.T(c, "metrics_disks"), series{written, t.write}, series{read, t.read})
	})
}

// transfer writes a volume's read or written bytes: a rate for a live host,
// whose Node samples them each second, and the preview's example total as a
// size.
func transfer(c *ui.Context, h *Host, v uint64) string {
	if h.Live {
		return rate(c, float64(v))
	}
	return orDash(Bytes(float64(v)))
}

// storageCard is the host's volumes: each a tile of
// its mount, file system, used of total and how full it is over a MyGo
// progress bar, with its read and write beside the mount. A tile chooses
// the volume the disk card follows.
func storageCard(c *ui.Context, s *State, h *Host, m *Metrics, t tones) {
	p := theme.Of(c)
	volumes := m.Disks
	chosen, _ := m.Volume(s.disk)
	panel(c, "storage", false, func() {
		ui.Row(c.Key("head")).Gap(8).AlignItems(ui.Center).Children(func() {
			title(c, i18n.T(c, "host_storage")).Grow(1)
			ui.Text(c, i18n.T(c, "host_partition_count", "count", len(volumes))).FontSize(12).SingleLine().TextColor(p.MutedForeground)
		})
		if len(volumes) == 0 {
			ui.Text(c, i18n.T(c, "metrics_no_disks")).FontSize(12).TextColor(p.MutedForeground)
			return
		}
		ui.Row(c.Key("volumes")).Wrap().Gap(8).AlignItems(ui.Stretch).Children(func() {
			for _, d := range volumes {
				used, percent := d.Used()
				detail := fmt.Sprintf("%s · %s / %s · %d%%", d.FileSystem, Bytes(float64(used)), Bytes(float64(d.Total)), int(math.Round(percent)))
				tile := kit.GhostLook(c, ui.ButtonBase(c.Key(d.Mount)).Label(d.Mount).Description(detail).Selected(d.Mount == chosen.Mount), d.Mount == chosen.Mount).
					Grow(1).Shrink(1).Basis(400).MinWidth(0).Padding(12, 14).Justify(ui.Start)
				tile.Children(func() {
					ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(6).Children(func() {
						ui.Row(c).Gap(12).AlignItems(ui.Center).Children(func() {
							ui.Text(c, d.Mount).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
							ui.Text(c, i18n.T(c, "host_read_short")+" "+transfer(c, h, d.ReadBytes)+"  "+i18n.T(c, "host_write_short")+" "+transfer(c, h, d.Written)).
								FontSize(12).Font("monospace").SingleLine().Shrink(0).TextColor(p.MutedForeground)
						})
						ui.Text(c, detail).FontSize(12).Font("monospace").SingleLine().TextColor(p.MutedForeground)
						ui.Progress(c.Key("capacity"), percent/100).FillWidth().Height(4).Label(d.Mount).TextColor(t.capacity(percent))
					})
				}).OnClick(func() { s.disk = d.Mount })
			}
		})
	})
}
