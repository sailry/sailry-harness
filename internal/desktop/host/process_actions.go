package host

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// processStop is the request to end a process that waits for its
// confirmation: the process as the menu was opened on it.
type processStop struct {
	open    bool
	process Process
	force   bool
}

// processID names a process's row across samples and sorts: its PID and its
// start, and for the preview's examples, which have neither, its name.
type processID struct {
	pid     uint32
	started uint64
	name    string
}

func (x Process) id() processID {
	if x.PID == 0 {
		return processID{name: x.Name}
	}
	return processID{pid: x.PID, started: x.Started}
}

// Stoppable reports whether the process is one a Node may be asked to end:
// listed with its start, and neither the system's first process nor a
// preview's example (process_menu in live/host/process_actions.rs).
func (x Process) Stoppable() bool { return x.PID > 1 && x.Started != 0 }

// processMenu fills a process row's right-click menu, the system's own, as
// the reference's (live/host/process_actions.rs): its details, its PID and
// name to the clipboard, then End process and Force quit, which ask before
// they send. A Windows host has no request to end, only the forced stop.
// The two are disabled for a process that cannot be ended, while a request
// runs, and on a host whose window cannot send one.
func (s *State) processMenu(c *ui.Context, m *ui.Menu, h *Host, x Process, act Actions) {
	if m.Item(i18n.T(c, "host_process_details")).Chosen() {
		s.detail, s.detailed = x, true
	}
	if m.Item(i18n.T(c, "host_process_copy_pid")).Disabled(x.PID == 0).Chosen() {
		c.WriteClipboard(strconv.FormatUint(uint64(x.PID), 10))
	}
	if m.Item(i18n.T(c, "host_process_copy_name")).Chosen() {
		c.WriteClipboard(x.Name)
	}
	m.Separator()
	blocked := act.StopProcess == nil || h.Stopping || !x.Stoppable()
	if h.OS != "windows" && m.Item(i18n.T(c, "host_process_stop")).Disabled(blocked).Chosen() {
		s.stop = processStop{open: true, process: x}
	}
	if m.Item(i18n.T(c, "host_process_force_stop")).Disabled(blocked).Chosen() {
		s.stop = processStop{open: true, process: x, force: true}
	}
}

// processDialogs are the process menu's dialogs: the details, and the
// confirmation of End process and Force quit, which names the process and
// what follows once and sends the request on its action
// (Actions.StopProcess). The request's outcome is the window's toast.
func processDialogs(c *ui.Context, s *State, h *Host, act Actions) {
	if act.StopProcess == nil || h.Stopping {
		s.stop.open = false
	}
	if s.stop.open {
		title, consequence := "host_process_stop", "host_process_stop_confirm"
		if s.stop.force {
			title, consequence = "host_process_force_stop", "host_process_force_confirm"
		}
		x := s.stop.process
		message := x.Name + " · PID " + strconv.FormatUint(uint64(x.PID), 10) + "\n" + i18n.T(c, consequence)
		if kit.AlertDialog(c.Key("process-stop"), &s.stop.open, i18n.T(c, title), message, i18n.T(c, "settings_cancel"), i18n.T(c, title)) == 1 {
			act.StopProcess(x, s.stop.force)
		}
	}
	if !s.detailed {
		return
	}
	p := theme.Of(c)
	w, _ := c.Size()
	title := i18n.T(c, "host_process_details")
	x := s.detail
	pid := dash
	if x.PID != 0 {
		pid = strconv.FormatUint(uint64(x.PID), 10)
	}
	kit.Dialog(c.Key("process-details"), &s.detailed, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
		ui.Column(c.Key("facts")).Gap(6).Children(func() {
			for _, f := range []Fact{
				{"host_process_name", x.Name}, {"host_pid", pid},
				{"metrics_cpu", orDash(Percent(x.CPU))}, {"metrics_memory", orDash(Bytes(float64(x.Memory)))},
			} {
				ui.Row(c.Key(f.Label)).Gap(12).AlignItems(ui.Start).Children(func() {
					ui.Text(c, i18n.T(c, f.Label)).FontSize(14).Width(96).Shrink(0).TextColor(p.MutedForeground)
					ui.Text(c, f.Value).FontSize(14).Grow(1).Shrink(1).MinWidth(0).Selectable().TextColor(p.PopoverForeground)
				})
			}
		})
	}).Width(min(420, w-48)).Gap(12).Label(title)
}
