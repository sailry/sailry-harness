package host

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Actions are what the page asks of the window: open a project or a
// session, and add a project (the project editor). Revoke removes the
// host's pairing, nil for a host that has none to remove (this computer, the
// preview's).
type Actions struct {
	Revoke      func()
	OpenProject func(id string)
	OpenSession func(id string)
	AddProject  func()
	Update      func(Update)
	OpenPort    func(Port)
	ClosePort   func(Port)
	CreatePort  func(remote, local string, completed func(bool))
	Dialogs     func(*ui.Context)
}

// State is the page's UI state: its dialogs and their drafts.
type State struct {
	updating bool       // the update dialog
	revoking bool       // the Remove pairing confirmation
	ports    portPicker // the port picker and the new forward dialog
	// The failures toasted already, so a redraw does not repeat them.
	failed, metricsFailed, runtimeFailed string
}

// Page is the host's overview: the header with the platform, the status and
// Update, the facts, the projects, the runtime and the running sessions, 800
// wide at most and centered.
func Page(c *ui.Context, s *State, h *Host, act Actions) {
	if h == nil {
		return
	}
	ui.Scroll(c.Key("host-" + h.ID)).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c.Key("content")).FillWidth().MaxWidth(theme.ContentWidth).Margin(0, ui.Auto).Padding(24).Gap(24).Children(func() {
			header(c, s, h, act)
			facts(c, h)
			projects(c, h, act)
			runtime(c, h.Runtime)
			activity(c, h, act)
		})
	})
	s.report(c, h)
	updateDialog(c, s, h, act)
	portDialogs(c, s, h, act)
	revokeDialog(c, s, h, act)
	if act.Dialogs != nil {
		act.Dialogs(c)
	}
}

// Platform is the platform's logo, the processor for one not known.
func Platform(os string) *ui.SVG {
	switch os {
	case "macos":
		return icons.Apple
	case "windows":
		return icons.Windows
	case "linux":
		return icons.Linux
	}
	return icons.Cpu
}

// label is a name of the host's as it shows. The preview's host names itself
// and its processes by i18n keys, and a live host is local_host_name until its
// Node names it. Any other name of a live host (its own and its processes')
// is the device's: it shows as it is, not as the catalog's text for a key it
// happens to spell.
func (h *Host) label(c *ui.Context, text string) string {
	if !h.Live || text == "local_host_name" {
		return i18n.T(c, text)
	}
	return text
}

// header is the host's name, status and platform beside its logo, then its
// port forwards and Update for a host updated from here. It has no Remove
// pairing button (docs/removed-ui.md): the rail's host menu asks for it.
func header(c *ui.Context, s *State, h *Host, act Actions) {
	p := theme.Of(c)
	ui.Row(c.Key("header")).Gap(16).Padding(16, 0).Wrap().Children(func() {
		ui.Icon(c, Platform(h.OS)).FontSize(40).Shrink(0)
		ui.Column(c.Key("name")).Grow(1).Shrink(1).MinWidth(220).Gap(4).Children(func() {
			ui.Text(c, h.label(c, h.Name)).FontSize(20).FontWeight(600).SingleLine()
			status := "workspace_host_status"
			if h.Live {
				status = "live_connected"
				if h.Runtime.Error != "" {
					status = "connections_offline"
				} else if !h.Runtime.Known && h.Runtime.Version == "" {
					status = "connection_connecting"
				}
			}
			ui.Text(c, i18n.T(c, status)).FontSize(14).TextColor(p.MutedForeground)
			ui.Text(c, i18n.T(c, h.Detail)).FontSize(14).TextColor(p.MutedForeground).SingleLine()
		})
		ui.Row(c.Key("actions")).Gap(8).Wrap().Justify(ui.End).Children(func() {
			portsHeader(c, s, h, act)
			if h.Update != nil {
				small(c, "update", icons.ArrowUp, i18n.T(c, "host_update"), false, true).OnClick(func() { s.updating = true })
			}
		})
	})
}

// facts are the host's description list: type, then its counts.
func facts(c *ui.Context, h *Host) {
	typ := "composer_host_remote"
	if h.Local {
		typ = "composer_host_local"
	}
	ui.Column(c.Key("facts")).Gap(8).Children(func() {
		describe(c, i18n.T(c, "workspace_host_type"), i18n.T(c, typ))
		describe(c, i18n.T(c, "projects"), strconv.Itoa(len(h.Projects)))
		describe(c, i18n.T(c, "workspace_worktrees"), strconv.Itoa(h.Worktrees))
		describe(c, i18n.T(c, "workspace_sessions"), strconv.Itoa(h.Sessions))
		describe(c, i18n.T(c, "terminal"), strconv.Itoa(h.Terminals))
		active := "host_active_preview"
		if h.Live {
			active = "workspace_running"
		}
		describe(c, i18n.T(c, active), strconv.Itoa(len(h.Activity)))
	})
}

// describe is a description list's row (Kit DescriptionList, one column,
// unbordered): the muted label over a third, the value after it.
func describe(c *ui.Context, label, value string) {
	p := theme.Of(c)
	ui.Row(c.Key(label)).Gap(12).MinHeight(24).Children(func() {
		ui.Text(c, label).FontSize(14).TextColor(p.MutedForeground).WidthPercent(33).Shrink(0)
		ui.Text(c, value).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).Label(value)
	})
}

// section is a titled part of the page; count is "" for none.
func section(c *ui.Context, key, title, count string, body func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Gap(8).Children(func() {
		ui.Row(c.Key("title")).Gap(8).AlignItems(ui.Baseline).Children(func() {
			ui.Text(c, title).FontSize(14).FontWeight(600)
			if count != "" {
				ui.Text(c, count).FontSize(14).TextColor(p.MutedForeground)
			}
		})
		body()
	})
}

// muted is a section's hint line.
func muted(c *ui.Context, text string) {
	ui.Text(c, text).FontSize(14).TextColor(theme.Of(c).MutedForeground)
}

// projects lists the host's projects, each opening its first
// conversation; with none, the empty card with Add project.
func projects(c *ui.Context, h *Host, act Actions) {
	p := theme.Of(c)
	section(c, "projects", i18n.T(c, "projects"), "", func() {
		if len(h.Projects) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Folder, Title: i18n.T(c, "project_empty"), Card: true, Extra: func() {
				label := i18n.T(c, "project_add")
				kit.Primary(c.Key("add"), label, false).Children(func() {
					ui.Icon(c, icons.Plus).FontSize(16)
					ui.Text(c, label).FontSize(14)
				}).OnClick(act.AddProject)
			}})
			return
		}
		for _, pr := range h.Projects {
			row := ui.ButtonBase(c.Key(pr.ID)).Label(pr.Name).Height(36).PaddingX(12).Gap(8).Radius(p.Radius).Justify(ui.Start).
				Cursor(ui.CursorPointer).TextColor(p.Foreground)
			if row.Hovered() {
				row.Background(p.ListHover)
			}
			row.Children(func() {
				ui.Icon(c, icons.Folder).FontSize(16)
				ui.Text(c, pr.Name).FontSize(14).SingleLine().Shrink(1)
			}).OnClick(func() { act.OpenProject(pr.ID) })
		}
	})
}

// runtime is the Node's facts: a shimmer while they load, unknown ones as
// host_unknown, and the preview's note.
func runtime(c *ui.Context, r Runtime) {
	p := theme.Of(c)
	section(c, "runtime", i18n.T(c, "workspace_runtime"), "", func() {
		if r.Loading {
			loading := "host_runtime_loading"
			if r.Live {
				loading = "live_host_runtime_loading"
			}
			kit.Shimmer(c, i18n.T(c, loading), 14, p.MutedForeground)
		}
		unknown := i18n.T(c, "host_unknown")
		for _, f := range r.Facts() {
			v := f.Value
			if f.Label == "host_uptime" {
				var parts []string
				for _, u := range UptimeParts(r.Uptime) {
					parts = append(parts, i18n.T(c, u[0].(string), "count", u[1]))
				}
				v = strings.Join(parts, " ")
			}
			if v == "" {
				v = unknown
			}
			describe(c, i18n.T(c, f.Label), v)
		}
		note := "workspace_runtime_preview"
		if r.Known {
			note = "host_runtime_preview"
		}
		if !r.Live {
			muted(c, i18n.T(c, note))
		}
	})
}

// activity is the host's running sessions: each a row with the agent or the
// waiting user, title, location, model and status, opening the session.
func activity(c *ui.Context, h *Host, act Actions) {
	p := theme.Of(c)
	rows := h.Activity
	section(c, "activity", i18n.T(c, "workspace_running"), strconv.Itoa(len(rows)), func() {
		if !h.Live {
			muted(c, i18n.T(c, "host_activity_hint"))
		}
		if len(rows) == 0 {
			empty := "workspace_no_running"
			if h.Live {
				empty = "live_host_no_running"
			}
			kit.Result(c, kit.ResultProps{Icon: icons.Dashboard, Title: i18n.T(c, empty), Card: true})
			return
		}
		for _, r := range rows {
			b := ui.ButtonBase(c.Key(r.Session)).Label(r.Title).Padding(8, 12).Gap(12).Radius(p.Radius).Justify(ui.Start).
				Cursor(ui.CursorPointer).TextColor(p.Foreground).Background(p.Secondary)
			if b.Hovered() {
				b.Background(p.SecondaryHover)
			}
			b.Children(func() {
				icon := icons.Bot
				if r.Waiting {
					icon = icons.CircleUser
				}
				ui.Icon(c, icon).FontSize(20).Shrink(0)
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(kit.TitleGap).Children(func() {
					ui.Text(c, r.Title).FontSize(14).SingleLine()
					ui.Text(c, r.Location).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				})
				if r.Model != "" {
					ui.Text(c, r.Model).FontSize(12).SingleLine().MaxWidth(128)
				}
				ui.Text(c, i18n.T(c, r.Status)).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
			}).OnClick(func() { act.OpenSession(r.Session) })
		}
	})
}

// small is Kit's small button with an icon and a label: ghost, or
// outline when outline is set (Button::small).
func small(c *ui.Context, key any, icon *ui.SVG, label string, disabled, outline bool) ui.Element {
	p := theme.Of(c)
	b := kit.GhostLook(c, ui.ButtonBase(c.Key(key)).Label(label).Disabled(disabled), false).Height(28).PaddingX(8).Gap(6)
	if outline {
		b.Border(1, p.Border).Background(p.ButtonSecondary)
		if b.Hovered() {
			b.Background(p.ButtonSecondaryHover)
		}
	}
	if disabled {
		b.Opacity(0.5)
	}
	return b.Children(func() {
		if icon != nil {
			ui.Icon(c, icon).FontSize(14)
		}
		if label != "" {
			ui.Text(c, label).FontSize(14).SingleLine()
		}
	})
}

// report toasts the host's failures once each (Shell::observe_host_errors):
// the runtime's with its detail.
func (s *State) report(c *ui.Context, h *Host) {
	if e := h.Runtime.Error; e != "" && e != s.runtimeFailed {
		key := "host_runtime_error"
		if h.Live {
			key = "live_host_runtime_error"
		}
		kit.Error(c, i18n.T(c, key), e)
	}
	s.runtimeFailed = h.Runtime.Error
}
