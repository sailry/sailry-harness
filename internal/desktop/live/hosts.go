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
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type liveHost struct {
	node        protocol.NodeID
	view        *host.Host
	owner       *Runtime
	info        *protocol.HostInfo
	infoPending bool
	infoNext    time.Time
	infoError   string
	// linkError is why the Link does not reach the host now, "" once it
	// does: a copy key for a protocol mismatch, else the Link's own reason.
	linkError      string
	metricsCancel  context.CancelFunc
	metricsPending bool
	metricsNext    time.Time
	metricsEpoch   uint64
	metricsDemand  bool
	metricsDetail  bool
	metricsError   string
	sample         *protocol.HostMetricsSample
	// metrics is the last sample as the page shows it, whose history the
	// next one continues (host.Metrics.Follow).
	metrics        *host.Metrics
	install        *sshInstallation
	update         *hostUpdate
	active         []hostActivity
	activeOwner    *Runtime
	activeRevision uint64
}

// hostActivity is a session that runs or waits in its Node snapshot.
type hostActivity struct {
	session protocol.Session
	waiting bool
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
		state = &liveHost{owner: owner, view: &host.Host{ID: id, Live: true, Local: local, Detail: "host_unknown", Runtime: host.Runtime{Uptime: -1, NodeUptime: -1}}}
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
			state.linkError = hostLinkError(peer.err)
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

// hostLinkError is what the host's page says of a Link failure: the copy
// key that names which side to update when the protocol versions differ,
// which alone decide compatibility, else the failure's own text.
func hostLinkError(err error) string {
	if err == nil {
		return ""
	}
	var failure *protocol.Fault
	if errors.As(err, &failure) && failure.Details != nil {
		if mismatch, ok := failure.Details.Data.(protocol.FaultDetailsVersionMismatchPayload); ok {
			switch {
			case mismatch.UpdateHost && mismatch.Unverified:
				return "host_version_unknown"
			case mismatch.UpdateHost:
				return "host_version_older"
			case mismatch.Unverified:
				return "controller_version_unknown"
			}
			return "host_version_newer"
		}
	}
	return err.Error()
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
	view.Activity = nil
	view.Runtime.Error = state.infoError
	if state.linkError != "" {
		view.Runtime.Error = state.linkError
	}
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
		// The platform is the header's line, before the versions.
		view.Detail = strings.Join(details, " · ")
		// The computer's uptime and the Node's are two facts: a Node started
		// minutes ago on a computer up for days shows both.
		view.Runtime.Uptime, view.Runtime.NodeUptime = seconds(info.HostUptimeSecs), seconds(info.NodeUptimeSecs)
		view.Runtime.Known = true
	}
	view.Offline = state.owner == nil || !state.owner.controller.Connected()
	if state.owner == nil {
		return
	}
	snapshot, revision := state.owner.snapshotAt()
	// The lanes follow from the Node snapshot alone and are found once for each.
	if state.activeOwner != state.owner || state.activeRevision != revision {
		state.active, state.activeOwner, state.activeRevision = nil, state.owner, revision
		for _, session := range snapshot.Sessions {
			if session.Delegation != nil || session.Archived {
				continue
			}
			if lane := client.AggregateLane(session, snapshot.Sessions); lane == client.LaneRunning || lane == client.LaneWaiting {
				state.active = append(state.active, hostActivity{session: session, waiting: lane == client.LaneWaiting})
			}
		}
	}
	for _, active := range state.active {
		session := active.session
		row := host.Activity{Session: session.ID.String(), Title: session.Activity.Title, Model: session.Config.Model, Waiting: active.waiting, Status: "activity_running"}
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

// seconds is a count of seconds as a duration, the longest one for a count
// past it.
func seconds(count uint64) time.Duration {
	return time.Duration(min(count, uint64((1<<63-1)/int64(time.Second)))) * time.Second
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
	footer := r.App.Settings.SidebarMetrics && r.App.NavShown()
	for id, state := range r.hosts {
		// The host's page is its monitor: it samples in detail while it
		// shows, as the reference's bind_host_monitor, whether or not the
		// summary column is open.
		detailed := r.App.Page == shell.HostPage && r.App.Host == id
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
	root := r.localRuntime()
	if state := root.hosts[id]; state != nil && !state.view.Local && root.connections != nil && root.connections.endpoint != nil {
		actions.Revoke = func() { root.revokeHost(state) }
	}
	if state := root.hosts[id]; state != nil {
		actions.StopProcess = func(process host.Process, force bool) { root.stopHostProcess(state, process, force) }
	}
	actions.Dialogs = func(c *ui.Context) {
		if state := r.hostState(id); state != nil && state.update != nil {
			r.hostUpdateFrame(c, state)
		}
	}
	return actions
}

// revokeHost removes a paired host's pairing, as Settings › Connections
// does: its page and the rail's menu disable Remove pairing meanwhile, and
// the host leaves the list with its peer.
func (r *Runtime) revokeHost(state *liveHost) {
	if state.view.Revoking {
		return
	}
	state.view.Revoking = true
	r.revokeNode(state.view.ID, func() { state.view.Revoking = false })
}

// stopHostProcess asks the host's own Node to end a process the page's
// monitor listed, by force when force is set, as the reference's
// stop_process (live/host/process_actions.rs). The Node signals it only
// while its start is still the listed one. One request runs for a host at a
// time; its outcome is a toast, and a request whose result is not known is
// never sent again.
func (r *Runtime) stopHostProcess(state *liveHost, process host.Process, force bool) {
	owner := state.owner
	if owner == nil || state.view.Stopping || !process.Stoppable() {
		return
	}
	state.view.Stopping = true
	r.async(func() {
		output, err := owner.controller.Do(owner.ctx, protocol.Command{Kind: "stop_host_process", Data: protocol.CommandStopHostProcessPayload{
			Pid: process.PID, StartedAtSecs: process.Started, Force: force,
		}})
		r.enqueue(func(c *ui.Context) {
			state.view.Stopping = false
			outcome, key := processStopOutcome(output, err)
			if outcome == kit.Done {
				kit.Message(c, i18n.T(c, key))
				return
			}
			kit.Toast(c, outcome, i18n.T(c, key))
		})
	})
}

// processStopOutcome is how a stop request ended and its toast's i18n key.
// A refusal the Node gives is a failure with its reason; a process whose
// start changed is a conflict; a result that did not arrive, or is not the
// signal's, is uncertain.
func processStopOutcome(output protocol.Output, err error) (kit.Outcome, string) {
	if err == nil {
		if output.Kind == "host_process_signalled" {
			return kit.Done, "host_process_stop_sent"
		}
		return kit.Uncertain, "host_process_uncertain"
	}
	switch faultCode(err) {
	case protocol.ErrorCodeNotFound:
		return kit.Failed, "host_process_gone"
	case protocol.ErrorCodeConflict:
		return kit.Conflict, "host_process_changed"
	case protocol.ErrorCodePermissionDenied:
		return kit.Failed, "host_process_protected"
	case protocol.ErrorCodeOutcomeUnknown, "":
		return kit.Uncertain, "host_process_uncertain"
	}
	return kit.Failed, "host_process_stop_failed"
}

func (r *Runtime) hostState(id string) *liveHost {
	return r.localRuntime().hosts[id]
}
