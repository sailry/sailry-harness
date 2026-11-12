package host

import (
	"math"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The processes' table is as tall as its rows up to processHeight, past
// which it scrolls them; a row is processRow high, the compact row of the
// application's tables (kit.TableCell).
const (
	processHeight = 360
	processRow    = 36
)

// processPanel is the host's busiest processes, as the reference's
// (live/host/monitor.rs, processes.rs): Kit's toolbar with the count of
// every process and a search, a note when the sample lists fewer than run,
// then MyGo's table in Kit's table look (kit.TableFrame: the frame, the
// filled header, compact separated rows, as a package's tables) of the
// name, the processor's load over the last minute, the memory and the PID,
// the three figures at the end of their columns, sorted by a click on a
// header and by the load at first. A row is its process (its PID and start), so the system's menu a
// right-click opens on it (processMenu) acts on that process however the
// samples sort the rows meanwhile, and the table stays where it is scrolled.
// The table is built under the neutral theme: nothing in it draws the
// accent.
func processPanel(c *ui.Context, s *State, h *Host, m *Metrics, t tones, act Actions) {
	p := theme.Of(c)
	if s.sort.Column == "" {
		s.sort = ui.SortOrder{Column: "cpu", Descending: true}
	}
	s.processes.Sort = &s.sort
	named := make([]Process, len(m.Processes))
	for i, x := range m.Processes {
		x.Name = h.label(c, x.Name)
		named[i] = x
	}
	rows := SortProcesses(named, s.query, s.sort.Column, s.sort.Descending)
	s.processes.Key, s.processes.KeepOffset = func(row int) any { return rows[row].id() }, true
	s.processes.CellMenu = func(menu *ui.Menu, row, _ int) {
		if row < len(rows) {
			s.processMenu(c, menu, h, rows[row], act)
		}
	}
	total := len(m.Processes)
	if m.ProcessCount >= 0 {
		total = m.ProcessCount
	}
	panel(c, "processes", false, func() {
		kit.Toolbar(c, i18n.T(c, "metrics_processes"), total, &s.query, i18n.T(c, "host_process_search"), nil)
		if total > len(m.Processes) && len(m.Processes) > 0 {
			ui.Text(c.Key("limited"), i18n.T(c, "host_process_limited")).FontSize(12).TextColor(p.MutedForeground)
		}
		if len(rows) == 0 {
			empty := "host_process_empty"
			if len(m.Processes) > 0 {
				empty = "host_process_no_matches"
			}
			kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, empty), Card: true})
			return
		}
		columns := []ui.TableColumn{
			{Title: i18n.T(c, "host_process_name"), ID: "name", MinWidth: 160, Sortable: true},
			{Title: i18n.T(c, "metrics_cpu"), ID: "cpu", Width: 200, Align: ui.End, Sortable: true},
			{Title: i18n.T(c, "metrics_memory"), ID: "memory", Width: 120, Align: ui.End, Sortable: true},
			{Title: i18n.T(c, "host_pid"), ID: "pid", Width: 72, Align: ui.End, Sortable: true},
		}
		height := min(processHeight, kit.TableHeight(c, len(rows), processRow))
		kit.TableFrame(c, ui.Column(c.Key("frame")).FillWidth().Height(height)).Children(func() {
			ui.Themed(c.Key("neutral"), theme.Neutral(c)).Background(ui.Transparent).Grow(1).FillWidth().MinHeight(0).Children(func() {
				processTable(c, s, t, columns, rows).Fill().Label(i18n.T(c, "metrics_processes"))
			})
		})
	})
}

// processTable is the processes' table over its sorted rows. A cell is
// plain text: the name has no letter tile before it and the memory no bar
// of its share (docs/removed-ui.md); the load keeps its last minute before
// its figure.
func processTable(c *ui.Context, s *State, t tones, columns []ui.TableColumn, rows []Process) ui.Element {
	p := theme.Of(c)
	return ui.Table(c.Key("table"), &s.processes, columns, len(rows), func(row, col int) {
		x := rows[row]
		cell := kit.TableCell(c, processRow, row+1 == len(rows))
		if col > 0 {
			cell.AlignItems(ui.End)
		}
		cell.Children(func() {
			switch col {
			case 0:
				ui.Text(c, x.Name).FontSize(14).SingleLine()
			case 1:
				ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
					top := 100.0
					for _, v := range x.History {
						if !math.IsNaN(v) {
							top = max(top, v)
						}
					}
					sparkline(c, x.History, top, t.cpu, 96, 20)
					ui.Text(c, orDash(Percent(x.CPU))).FontSize(12).Font("monospace").SingleLine().MinWidth(48).TextAlign(ui.End)
				})
			case 2:
				ui.Text(c, orDash(Bytes(float64(x.Memory)))).FontSize(12).Font("monospace").SingleLine()
			case 3:
				pid := dash
				if x.PID != 0 {
					pid = strconv.FormatUint(uint64(x.PID), 10)
				}
				ui.Text(c, pid).FontSize(12).Font("monospace").SingleLine().TextColor(p.MutedForeground)
			}
		})
	})
}

// processSkeleton holds the processes table's place until the first sample
// in the table's look: the frame, the filled 32 DIP head and four separated
// rows, each of three blocks and one at the end.
func processSkeleton(c *ui.Context) {
	p := theme.Of(c)
	block := func(w float32) { kit.Skeleton(c, kit.SkeletonProps{Width: w, Height: 16}) }
	ui.Column(c.Key("process-skeleton")).FillWidth().Busy(true).Border(1, p.Border).Radius(p.Radius).Clip().Children(func() {
		for i := range 5 {
			r := ui.Row(c.Key(i)).Height(processRow).PaddingX(12).Gap(12).ClipX().AlignItems(ui.Center).Justify(ui.SpaceBetween)
			if i == 0 {
				r.Height(32).Background(p.Muted)
			} else {
				r.BorderWidth(1, 0, 0, 0).BorderColor(p.Border)
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
