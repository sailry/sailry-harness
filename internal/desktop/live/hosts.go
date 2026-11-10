package live

import (
	"context"
	"errors"
	"runtime"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type liveHost struct {
	node           protocol.NodeID
	view           *host.Host
	owner          *Runtime
	info           *protocol.HostInfo
	infoPending    bool
	infoNext       time.Time
	infoError      string
	metricsCancel  context.CancelFunc
	metricsPending bool
	metricsNext    time.Time
	metricsEpoch   uint64
	metricsDemand  bool
	metricsDetail  bool
	metricsError   string
	sample         *protocol.HostMetricsSample
	install        *sshInstallation
	update         *hostUpdate
}

func (r *Runtime) initHosts() {
	if r.parent != nil {
		return
	}
	if r.hosts == nil {
		r.hosts = map[string]*liveHost{}
	}
	r.App.Hooks.HostActions = r.hostActions
	r.projectHosts(r.App.Data)
}

func (r *Runtime) closeHosts() {
	if r.parent != nil {
		return
	}
	for _, state := range r.hosts {
		if state.metricsCancel != nil {
			state.metricsCancel()
			state.metricsCancel = nil
		}
		state.metricsEpoch++
	}
}

func (r *Runtime) ensureHost(id string, owner *Runtime, local bool) *liveHost {
	state := r.hosts[id]
	if state == nil {
		state = &liveHost{owner: owner, view: &host.Host{ID: id, Live: true, Local: local, Detail: "host_unknown", Runtime: host.Runtime{Live: true, Uptime: -1}}}
		if local {
			state.view.Name = "local_host_name"
		} else {
			state.view.Name = id
			state.view.Update = &host.Update{Phase: host.Idle, Progress: -1}
			state.update = &hostUpdate{}
		}
		r.hosts[id] = state
	}
	if state.owner != owner {
		if state.metricsCancel != nil {
			state.metricsCancel()
			state.metricsCancel = nil
		}
		state.metricsEpoch++
		state.metricsPending, state.infoPending = false, false
		state.metricsNext, state.infoNext = time.Time{}, time.Time{}
		state.owner = owner
	}
	if owner != nil {
		state.node = owner.controller.Target()
	}
	return state
}

func (r *Runtime) projectHosts(d *viewmodel.Data) {
	if r.parent != nil || d == nil {
		return
	}
	if r.hosts == nil {
		r.hosts = map[string]*liveHost{}
	}
	localID := r.controller.Target().String()
	local := r.ensureHost(localID, r, true)
	local.view.Runtime.Version = link.ProductVersion
	local.view.Runtime.Timezone = time.Now().Format("MST -07:00")
	local.view.Runtime.ProtocolMin, local.view.Runtime.ProtocolMax = int(protocol.VERSION), int(protocol.VERSION)
	if local.view.OS == "" {
		local.view.OS = hostOS(runtime.GOOS)
	}
	order := []string{localID}
	if r.network != nil {
		devices := map[string]viewmodel.Device{}
		for _, device := range r.projectDevices() {
			devices[device.ID] = device
		}
		for _, peer := range r.network.snapshot() {
			id := peer.address.ID.String()
			device := devices[id]
			if !device.Execution {
				continue
			}
			state := r.ensureHost(id, r.bindings[id], false)
			state.node = peer.address.ID
			if device.Name != "" {
				state.view.Name = device.Name
			}
			if device.Platform != "" {
				state.view.OS = hostOS(device.Platform)
			}
			if peer.info.Name != nil && strings.TrimSpace(*peer.info.Name) != "" {
				state.view.Name = *peer.info.Name
			}
			if peer.info.SoftwareVersion != nil {
				state.view.Runtime.Version = *peer.info.SoftwareVersion
			}
			if peer.info.Platform != "" {
				state.view.OS = hostOS(peer.info.Platform)
			}
			if peer.info.Protocol != nil {
				state.view.Runtime.ProtocolMin, state.view.Runtime.ProtocolMax = int(*peer.info.Protocol), int(*peer.info.Protocol)
			}
			if peer.err != nil {
				state.infoError = peer.err.Error()
			}
			order = append(order, id)
		}
	}
	present := map[string]*host.Host{}
	d.Hosts = nil
	for _, id := range order {
		state := r.hosts[id]
		r.projectHost(state, d)
		present[id] = state.view
		d.Hosts = append(d.Hosts, viewmodel.Host{ID: id, Name: state.view.Name, Local: state.view.Local, Update: state.view.Update != nil && hostUpdateAvailable(state.view.Runtime.Version)})
	}
	for id, state := range r.hosts {
		if present[id] == nil && (state.install == nil || !state.install.active) {
			if state.metricsCancel != nil {
				state.metricsCancel()
			}
			delete(r.hosts, id)
		}
	}
	if present[r.App.Host] == nil {
		r.App.Host = localID
	}
	r.App.SetHosts(present)
}

func hostOS(value string) string {
	switch strings.ToLower(value) {
	case "darwin", "macos", "mac os", "mac os x":
		return "macos"
	case "windows":
		return "windows"
	case "linux":
		return "linux"
	}
	return ""
}

func (r *Runtime) projectHost(state *liveHost, d *viewmodel.Data) {
	view := state.view
	view.Projects, view.Activity = nil, nil
	view.Worktrees, view.Sessions, view.Terminals = 0, 0, 0
	view.Runtime.Loading = state.infoPending && state.info == nil
	view.Runtime.Error = state.infoError
	if view.Update != nil && view.Update.Phase == host.Idle {
		view.Update.Current = view.Runtime.Version
	}
	if state.info != nil {
		info := state.info
		if info.Name != nil && strings.TrimSpace(*info.Name) != "" {
			view.Name = *info.Name
		}
		if info.Os != nil {
			view.OS = hostOS(*info.Os)
		}
		var details []string
		for _, part := range []*string{info.Os, info.OsVersion, info.Architecture} {
			if part != nil && strings.TrimSpace(*part) != "" {
				details = append(details, *part)
			}
		}
		view.Detail = strings.Join(details, " · ")
		view.Runtime.System = view.Detail
		view.Runtime.Uptime = time.Duration(min(info.HostUptimeSecs, uint64((1<<63-1)/int64(time.Second)))) * time.Second
		view.Runtime.Known = true
	}
	if state.owner == nil {
		return
	}
	snapshot := state.owner.controller.Snapshot()
	view.Worktrees = len(snapshot.Worktrees)
	view.Sessions = len(snapshot.Sessions)
	for _, terminal := range snapshot.Terminals {
		if terminal.Status.Kind != "closed" {
			view.Terminals++
		}
	}
	for _, project := range d.Projects {
		if project.Host == view.ID {
			view.Projects = append(view.Projects, host.Project{ID: project.ID, Name: project.Name})
		}
	}
	for _, session := range snapshot.Sessions {
		if session.Delegation != nil || session.Archived {
			continue
		}
		lane := client.AggregateLane(session, snapshot.Sessions)
		if lane != client.LaneRunning && lane != client.LaneWaiting {
			continue
		}
		row := host.Activity{Session: session.ID.String(), Title: session.Activity.Title, Model: session.Config.Model, Waiting: lane == client.LaneWaiting, Status: "activity_running"}
		if row.Waiting {
			row.Status = "activity_waiting"
		}
		if session.Project != nil {
			for _, project := range snapshot.Projects {
				if project.ID == *session.Project {
					row.Location = project.Name
					break
				}
			}
		}
		if conversation := d.Find(row.Session); conversation != nil && conversation.Node == view.ID && conversation.Worktree != "" {
			if row.Location != "" {
				row.Location += " / "
			}
			row.Location += conversation.Worktree
		}
		view.Activity = append(view.Activity, row)
	}
}

func (r *Runtime) hostsFrame(c *ui.Context) {
	if r.parent != nil {
		return
	}
	if r.hosts == nil {
		r.initHosts()
	}
	r.projectHosts(r.App.Data)
	now := c.Now()
	selected := ""
	if owner := r.selectedRuntime(); owner != nil {
		selected = owner.controller.Target().String()
	}
	if r.App.Page == shell.HostPage {
		selected = r.App.Host
	}
	footer := r.App.Settings.SidebarMetrics && r.App.NavOpen
	width, _ := c.Size()
	nav := float32(0)
	if r.App.NavOpen {
		nav = shell.ClampNav(r.App.NavWidth)
	}
	panelVisible := shell.PanelFits(width, nav, shell.ClampPanel(r.App.PanelWidth, width, nav))
	for id, state := range r.hosts {
		detailed := r.App.Page == shell.HostPage && r.App.Host == id && r.App.Metrics && panelVisible
		demand := detailed || footer && selected == id
		if state.owner != nil && (state.info == nil || r.App.Page == shell.HostPage && r.App.Host == id) {
			if !state.infoPending && !now.Before(state.infoNext) {
				r.readHostInfo(state)
			}
			if r.App.Page == shell.HostPage && r.App.Host == id {
				c.After(30 * time.Second)
			}
		}
		r.hostMetricsFrame(c, state, demand, detailed)
		if state.install != nil {
			r.sshFrame(c, state.install)
		}
		if state.update != nil && state.update.form && (r.App.Page != shell.HostPage || r.App.Host != id) {
			state.update.form = false
			if state.install != nil && state.install.active {
				r.cancelSSH(state.install)
				state.view.Update.Phase = host.Staged
			}
		}
	}
	r.projectSidebarMetrics(selected, footer)
}

func (r *Runtime) readHostInfo(state *liveHost) {
	if state.owner == nil || state.infoPending {
		return
	}
	state.infoPending = true
	state.view.Runtime.Loading = state.info == nil
	owner := state.owner
	r.async(func() {
		output, err := owner.controller.Do(owner.ctx, protocol.Command{Kind: "inspect_host"})
		info, ok := detail[protocol.HostInfo](output.Data)
		if err == nil && (output.Kind != "host_info" || !ok || info.Node != owner.controller.Target()) {
			err = errors.New("Host information response belongs to another Node")
		}
		r.enqueue(func(c *ui.Context) {
			if r.hosts[state.view.ID] != state || state.owner != owner {
				return
			}
			state.infoPending, state.infoNext = false, c.Now().Add(30*time.Second)
			if err != nil {
				state.infoError = err.Error()
			} else {
				state.info, state.infoError = &info, ""
			}
			r.projectHosts(r.App.Data)
		})
	})
}

func (r *Runtime) hostActions(id string) host.Actions {
	actions := r.portActions(id)
	actions.Update = func(expected host.Update) { r.advanceHostUpdate(id, expected) }
	actions.Dialogs = func(c *ui.Context) {
		if state := r.hostState(id); state != nil && state.update != nil {
			r.hostUpdateFrame(c, state)
		}
	}
	return actions
}

func (r *Runtime) hostState(id string) *liveHost {
	return r.localRuntime().hosts[id]
}
