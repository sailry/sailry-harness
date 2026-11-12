package host

import (
	"math"
	"strconv"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The processes' table is as tall as its rows up to processHeight, past
// which it scrolls them: MyGo's table head is 32 DIP, and a row is its
// 22 DIP cell in the table's 6 DIP padding.
const (
	processHeight = 360
	processHead   = 32
	processCell   = 22
	processRow    = processCell + 12
)

// processPanel is the host's busiest processes, as the reference's
// (live/host/monitor.rs, processes.rs): Kit's toolbar with the count of
// every process and a search, a note when the sample lists fewer than run,
// then MyGo's table of the name, the processor's load over the last minute,
// the memory and the PID, sorted by a click on a header and by the load at
// first. A row is its process (its PID and start), so the system's menu a
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
			{Title: i18n.T(c, "metrics_cpu"), ID: "cpu", Width: 200, Sortable: true},
			{Title: i18n.T(c, "metrics_memory"), ID: "memory", Width: 140, Sortable: true},
			{Title: i18n.T(c, "host_pid"), ID: "pid", Width: 72, Align: ui.End, Sortable: true},
		}
		height := min(processHeight, processHead+float32(len(rows))*processRow+2)
		ui.Themed(c.Key("neutral"), theme.Neutral(c)).Background(ui.Transparent).FillWidth().Height(height).Children(func() {
			processTable(c, s, m, t, columns, rows).Fill().Label(i18n.T(c, "metrics_processes"))
		})
	})
}

// processTable is the processes' table over its sorted rows.
func processTable(c *ui.Context, s *State, m *Metrics, t tones, columns []ui.TableColumn, rows []Process) ui.Element {
	p := theme.Of(c)
	return ui.Table(c.Key("table"), &s.processes, columns, len(rows), func(row, col int) {
		x := rows[row]
		switch col {
		case 0:
			ui.Row(c).Gap(8).MinHeight(processCell).AlignItems(ui.Center).Children(func() {
				initial, _ := utf8.DecodeRuneInString(x.Name)
				ui.Row(c).Size(22, 22).Shrink(0).Radius(11).Center().Background(p.Secondary).Children(func() {
					ui.Text(c, string(initial)).FontSize(11).FontWeight(600).SingleLine().TextColor(p.MutedForeground)
				})
				ui.Text(c, x.Name).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			})
		case 1:
			ui.Row(c).Gap(8).MinHeight(processCell).AlignItems(ui.Center).Children(func() {
				top := 100.0
				for _, v := range x.History {
					if !math.IsNaN(v) {
						top = max(top, v)
					}
				}
				sparkline(c, x.History, top, t.cpu, 96, 20)
				ui.Text(c, orDash(Percent(x.CPU))).FontSize(12).Font("monospace").SingleLine()
			})
		case 2:
			ui.Row(c).Gap(8).MinHeight(processCell).AlignItems(ui.Center).Children(func() {
				share, _ := Usage(x.Memory, m.Memory)
				ui.Progress(c.Key("memory"), share/100).Width(44).Shrink(0).Height(4).Label(x.Name).TextColor(t.memory)
				ui.Text(c, orDash(Bytes(float64(x.Memory)))).FontSize(12).Font("monospace").SingleLine()
			})
		case 3:
			pid := dash
			if x.PID != 0 {
				pid = strconv.FormatUint(uint64(x.PID), 10)
			}
			ui.Text(c, pid).FontSize(12).Font("monospace").SingleLine().TextColor(p.MutedForeground)
		}
	})
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
