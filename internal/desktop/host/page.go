package host

import (
	"strconv"
	"strings"
	"time"

	"github.com/ELY-Open-Source/CrychicUI/feedback"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Actions are what the page asks of the window: open a session. Revoke removes the
// host's pairing, nil for a host that has none to remove (this computer, the
// preview's). StopProcess asks the host's Node to end a process, by force
// when force is set, after the page's confirmation; nil where no Node is
// asked (the preview), which leaves the menu's two items disabled.
type Actions struct {
	Revoke      func()
	StopProcess func(process Process, force bool)
	OpenSession func(id string)
	Update      func(Update)
	OpenPort    func(Port)
	ClosePort   func(Port)
	CreatePort  func(remote, local string, completed func(bool))
	Dialogs     func(*ui.Context)
}

// State is the page's UI state: its dialogs and their drafts, and the
// monitor's choices.
type State struct {
	updating bool       // the update dialog
	revoking bool       // the Remove pairing confirmation
	ports    portPicker // the port picker and the new forward dialog
	// disk is the volume the disk card follows, "" for the root; gpu the
	// accelerator the GPU card follows, "" for the first; query, sort and
	// processes are the processes' search, order and table.
	disk, gpu string
	query     string
	sort      ui.SortOrder
	processes ui.ListState
	// stop is the request to end a process that waits for its confirmation;
	// detail is the process whose details show while detailed.
	stop     processStop
	detail   Process
	detailed bool
	// The failures toasted already, so a redraw does not repeat them.
	failed, metricsFailed, runtimeFailed string
}

// Width is the page's widest: the reference's host page and its statistics
// page are 1040.
const Width = 1040

// Page is the host's page, as the reference's live host page
// (live/host.rs): the header with the platform, the status, the uptimes and
// the forwards, the monitor, then the running sessions, 1040 wide at most
// and centered, 14 apart. It has neither a projects section nor a runtime
// section (AGENTS.md, Removed UI): projects are added from the sidebar, and the
// header's line names the two versions.
func Page(c *ui.Context, s *State, h *Host, act Actions) {
	if h == nil {
		return
	}
	ui.Scroll(c.Key("host-" + h.ID)).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c.Key("content")).FillWidth().MaxWidth(Width).Margin(0, ui.Auto).Padding(24).Gap(sectionGap).Children(func() {
			header(c, s, h, act)
			monitor(c, s, h, act)
			activity(c, h, act)
		})
	})
	s.report(c, h)
	updateDialog(c, s, h, act)
	portDialogs(c, s, h, act)
	revokeDialog(c, s, h, act)
	processDialogs(c, s, h, act)
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
		if name := demoProcesses[text]; name != "" && kit.Demo(c) {
			return name
		}
		return i18n.T(c, text)
	}
	return text
}

// demoProcesses are the programs the preview's example processes run as
// while the demo shows: their catalog names say that they are a preview's.
var demoProcesses = map[string]string{"metrics_worker": "sailry-node", "metrics_indexer": "gopls"}

// plain reports whether the page speaks of the host as a live one: a live
// host, or the preview's while the demo shows, which says nowhere that its
// data is a sample. What the page does is the preview's all the same.
func (h *Host) plain(c *ui.Context) bool { return h.Live || kit.Demo(c) }

// Connection is a live host's connection as its header shows it.
func (h *Host) Connection() feedback.Connection {
	switch {
	case h.Offline || h.Runtime.Error != "":
		return feedback.Offline
	case !h.Runtime.Known && h.Runtime.Version == "":
		return feedback.Reconnecting
	}
	return feedback.Online
}

// header is the host's logo in a round tile, its name beside its status and
// its platform and versions under them, then the computer's and Sailry's
// uptimes and the processes' count, its port forwards, and Update for a host
// updated from here. The platform and the versions show here alone. A live host's status is CrychicUI's
// ConnectionStatus; the preview's is its note. It has no Remove pairing
// button (AGENTS.md, Removed UI): the rail's host menu asks for it.
func header(c *ui.Context, s *State, h *Host, act Actions) {
	p := theme.Of(c)
	ui.Row(c.Key("header")).Wrap().GapX(16).GapY(12).AlignItems(ui.Center).Children(func() {
		ui.Row(c.Key("identity")).Grow(1).Shrink(1).Basis(300).MinWidth(0).Gap(12).AlignItems(ui.Center).Children(func() {
			ui.Row(c.Key("logo")).Size(44, 44).Shrink(0).Radius(22).Center().Background(p.Secondary).Children(func() {
				ui.Icon(c, Platform(h.OS)).FontSize(20).TextColor(p.SecondaryForeground)
			})
			ui.Column(c.Key("name")).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
				ui.Row(c).Gap(10).AlignItems(ui.Center).Children(func() {
					ui.Text(c, h.label(c, h.Name)).FontSize(18).FontWeight(600).SingleLine().Shrink(1).MinWidth(0)
					if h.plain(c) {
						feedback.ConnectionStatus(c, h.Connection(), feedback.ConnectionStatusOptions{
							Online: i18n.T(c, "live_connected"), Reconnecting: i18n.T(c, "connection_connecting"), Offline: i18n.T(c, "connections_offline"),
						}).Shrink(0).AlignSelf(ui.Center)
					}
				})
				if !h.plain(c) {
					ui.Text(c, i18n.T(c, "workspace_host_status")).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				}
				ui.Text(c.Key("platform"), h.detail(c)).FontSize(12).SingleLine().TextColor(p.MutedForeground)
			})
		})
		ui.Row(c.Key("facts")).Wrap().GapX(24).GapY(8).AlignItems(ui.Center).Children(func() {
			processes := dash
			if h.Metrics != nil && h.Metrics.ProcessCount >= 0 && h.MetricsError == "" {
				processes = strconv.Itoa(h.Metrics.ProcessCount)
			}
			headerFact(c, i18n.T(c, "host_uptime"), uptime(c, h.Runtime.Uptime))
			headerFact(c, i18n.T(c, "host_node_uptime"), uptime(c, h.Runtime.NodeUptime))
			headerFact(c, i18n.T(c, "metrics_processes"), processes)
			ui.Row(c.Key("actions")).Gap(8).Wrap().Justify(ui.End).Children(func() {
				portsHeader(c, s, h, act)
				if h.Update != nil {
					small(c, "update", icons.ArrowUp, i18n.T(c, "host_update"), false, true).OnClick(func() { s.updating = true })
				}
			})
		})
	})
}

// detail is the header's line under the name: the platform, then the host's
// version and its protocol, each after its label, as
// "darwin · 27.2 · arm64 · Host version 0.1.6 · Protocol version v1". A
// version not known is left out.
func (h *Host) detail(c *ui.Context) string {
	parts := []string{h.label(c, h.Detail)}
	if version := strings.TrimSpace(h.Runtime.Version); version != "" {
		parts = append(parts, i18n.T(c, "host_version")+" "+version)
	}
	if protocol := Protocol(h.Runtime.ProtocolMin, h.Runtime.ProtocolMax); protocol != "" {
		parts = append(parts, i18n.T(c, "host_protocol")+" "+protocol)
	}
	return strings.Join(parts, " · ")
}

// headerFact is a fact of the header: the muted label over its value in
// the code font.
func headerFact(c *ui.Context, label, value string) {
	p := theme.Of(c)
	ui.Column(c.Key(label)).Gap(2).Children(func() {
		ui.Text(c, label).FontSize(12).SingleLine().TextColor(p.MutedForeground)
		ui.Text(c, value).FontSize(14).Font("monospace").SingleLine().Label(value)
	})
}

// uptime writes a duration in its two largest units, host_unknown for one
// not known.
func uptime(c *ui.Context, d time.Duration) string {
	var parts []string
	for _, u := range UptimeParts(d) {
		parts = append(parts, i18n.T(c, u[0].(string), "count", u[1]))
	}
	if len(parts) == 0 {
		return i18n.T(c, "host_unknown")
	}
	return strings.Join(parts, " ")
}

// entry is a row of a section's card that opens something: its icon, its
// content, and a chevron at the end.
func entry(c *ui.Context, key any, label string, icon *ui.SVG, content func()) ui.Element {
	p := theme.Of(c)
	return ui.ButtonBase(c.Key(key)).Label(label).Padding(kit.RowPad, 0).Gap(12).MinHeight(kit.RowMinHeight).Justify(ui.Start).
		Cursor(ui.CursorPointer).TextColor(p.SurfaceForeground).Children(func() {
		ui.Row(c).Size(kit.RowIcon, kit.RowIcon).Shrink(0).Center().Children(func() { ui.Icon(c, icon).FontSize(18) })
		content()
		ui.Icon(c, icons.ChevronRight).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
	})
}

// activity is the host's running and waiting sessions, a Kit group of
// rows: each the agent or the waiting user, the title over the location,
// the model and the status, opening the session. A host with none has no
// such section, as the reference's.
func activity(c *ui.Context, h *Host, act Actions) {
	rows := h.Activity
	if len(rows) == 0 {
		return
	}
	p := theme.Of(c)
	description := ""
	if !h.plain(c) {
		description = i18n.T(c, "host_activity_hint")
	}
	kit.Group(c, "activity", i18n.T(c, "workspace_running"), description, nil, func() {
		kit.Card(c, func() {
			for _, r := range rows {
				icon := icons.Bot
				if r.Waiting {
					icon = icons.CircleUser
				}
				entry(c, r.Session, r.Title, icon, func() {
					ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(kit.TitleGap).Children(func() {
						ui.Text(c, r.Title).FontSize(kit.RowFont).SingleLine()
						if r.Location != "" {
							ui.Text(c, r.Location).FontSize(kit.RowFont).SingleLine().TextColor(p.Description())
						}
					})
					if r.Model != "" {
						ui.Text(c, r.Model).FontSize(12).SingleLine().MaxWidth(160).Shrink(1).TextColor(p.MutedForeground)
					}
					tone := kit.Positive
					if r.Waiting {
						tone = kit.Caution
					}
					kit.Badge(c, i18n.T(c, r.Status), tone).Shrink(0)
				}).OnClick(func() { act.OpenSession(r.Session) })
			}
		})
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
// the runtime's with its detail, which is a copy key for a protocol
// mismatch (feedback.rs fault_message) and the Node's own text otherwise.
func (s *State) report(c *ui.Context, h *Host) {
	if e := h.Runtime.Error; e != "" && e != s.runtimeFailed {
		key := "host_runtime_error"
		if h.plain(c) {
			key = "live_host_runtime_error"
		}
		kit.Error(c, i18n.T(c, key), i18n.T(c, e))
	}
	s.runtimeFailed = h.Runtime.Error
}
