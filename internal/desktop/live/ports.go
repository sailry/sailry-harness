package live

import (
	"encoding/json"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type portKey struct {
	node string
	port uint16
}

// forwardRestores is how many times a forward that lost its Host is created
// again each time the Host returns.
const forwardRestores = 3

type desktopForward struct {
	forward *client.Forwarder
	release func()
	// source is the announced service the forward was opened from, kept to
	// create the forward again.
	source *client.ServiceSource
	// restores counts the attempts made since the Host returned.
	restores int
}

// restoreStep is what a frame does with one forward. A forward fails when
// its Host's connection is lost and never recovers by itself; it is created
// again on its local port once the Host is connected, at most forwardRestores
// times, and the count starts over when the Host is lost again. A forward
// that closed (the user or its service ended it) is left alone.
func restoreStep(kind string, connected bool, restores int) (attempt bool, next int) {
	if kind != "failed" {
		return false, restores
	}
	if !connected {
		return false, 0
	}
	return restores < forwardRestores, restores
}

type livePorts struct {
	mu       sync.Mutex
	closed   bool
	forwards map[portKey]desktopForward
	busy     map[portKey]bool
}

func (r *Runtime) initPorts() {
	r.ports = &livePorts{forwards: map[portKey]desktopForward{}, busy: map[portKey]bool{}}
}

func (r *Runtime) closePorts() {
	if r.parent != nil || r.ports == nil {
		return
	}
	r.ports.mu.Lock()
	r.ports.closed = true
	forwards := r.ports.forwards
	r.ports.forwards = map[portKey]desktopForward{}
	r.ports.mu.Unlock()
	for _, item := range forwards {
		item.release()
		_ = item.forward.Close()
	}
}

func (r *Runtime) forward(node string, port int) (*client.Forwarder, bool) {
	state := r.localRuntime().ports
	if state == nil {
		return nil, false
	}
	state.mu.Lock()
	defer state.mu.Unlock()
	key := portKey{node: node, port: uint16(port)}
	return state.forwards[key].forward, state.busy[key]
}

// restoreForwards creates again the forwards that failed with their Host's
// connection. The row shows the forward as not listening until then, and
// stays so when the attempts are used up.
func (r *Runtime) restoreForwards() {
	state := r.ports
	state.mu.Lock()
	failed := map[portKey]int{}
	for key, item := range state.forwards {
		if !state.closed && !state.busy[key] && item.forward.State().Kind == "failed" {
			failed[key] = item.restores
		}
	}
	state.mu.Unlock()
	for key, restores := range failed {
		owner := r.runtimeFor(key.node)
		connected := owner != nil && owner.controller.Connected() && (key.node == "" || owner.controller.Target().String() == key.node)
		attempt, next := restoreStep("failed", connected, restores)
		state.mu.Lock()
		item, found := state.forwards[key]
		if !found || state.closed || state.busy[key] || item.forward.State().Kind != "failed" {
			state.mu.Unlock()
			continue
		}
		if attempt {
			next++
			state.busy[key] = true
		}
		item.restores = next
		state.forwards[key] = item
		state.mu.Unlock()
		if !attempt {
			continue
		}
		if !r.async(func() {
			forward, err := r.connectForward(owner, key.port, item.forward.LocalPort(), item.source)
			state.mu.Lock()
			delete(state.busy, key)
			current, found := state.forwards[key]
			restored := err == nil && found && current.forward == item.forward && !state.closed
			if restored {
				current.release()
				state.forwards[key] = desktopForward{forward: forward, release: forward.OnChange(r.changed), source: item.source}
			}
			state.mu.Unlock()
			if !restored && forward != nil {
				_ = forward.Close()
			}
			r.changed()
		}) {
			state.mu.Lock()
			delete(state.busy, key)
			state.mu.Unlock()
		}
	}
}

func (r *Runtime) connectForward(owner *Runtime, remote, local uint16, source *client.ServiceSource) (*client.Forwarder, error) {
	if source == nil {
		return owner.controller.ForwardPort(r.ctx, remote, local)
	}
	encoded, err := json.Marshal(source)
	if err != nil {
		return nil, err
	}
	return owner.controller.ForwardService(r.ctx, encoded, local)
}

func (r *Runtime) portsFrame() {
	r.restoreForwards()
	state := r.ports
	state.mu.Lock()
	ports := map[string][]host.Port{}
	for key, item := range state.forwards {
		ports[key.node] = append(ports[key.node], host.Port{
			Remote: int(key.port), Local: int(item.forward.LocalPort()),
			Listening: item.forward.State().Kind == "listening",
		})
	}
	state.mu.Unlock()
	for _, peer := range r.App.Data.Hosts {
		values := ports[peer.ID]
		slices.SortFunc(values, func(first, second host.Port) int { return first.Remote - second.Remote })
		r.App.SetHostPorts(peer.ID, values)
	}
}

func (r *Runtime) portActions(node string) host.Actions {
	root := r.localRuntime()
	return host.Actions{
		OpenPort: func(port host.Port) {
			if forward, _ := root.forward(node, port.Remote); forward != nil && forward.State().Kind == "listening" {
				root.openForward(forward, nil)
			}
		},
		ClosePort: func(port host.Port) { root.closeForward(node, uint16(port.Remote)) },
		CreatePort: func(remote, local string, completed func(bool)) {
			remotePort, remoteErr := strconv.ParseUint(strings.TrimSpace(remote), 10, 16)
			localPort := uint64(0)
			var localErr error
			if strings.TrimSpace(local) != "" {
				localPort, localErr = strconv.ParseUint(strings.TrimSpace(local), 10, 16)
			}
			if remoteErr != nil || localErr != nil || remotePort == 0 {
				root.enqueue(func(c *ui.Context) {
					kit.Toast(c, kit.Failed, i18n.T(c, "port_invalid"))
					completed(false)
				})
				return
			}
			root.startForward(node, uint16(remotePort), uint16(localPort), nil, false, completed)
		},
	}
}

func (r *Runtime) closeForward(node string, port uint16) {
	state := r.ports
	state.mu.Lock()
	key := portKey{node: node, port: port}
	item, found := state.forwards[key]
	delete(state.forwards, key)
	state.mu.Unlock()
	if found {
		item.release()
		r.async(func() { _ = item.forward.Close(); r.changed() })
	}
	r.changed()
}

func (r *Runtime) startForward(node string, remote, local uint16, source *client.ServiceSource, open bool, completed func(bool)) {
	state := r.ports
	key := portKey{node: node, port: remote}
	state.mu.Lock()
	existing := state.forwards[key].forward
	if state.closed || state.busy[key] {
		state.mu.Unlock()
		if completed != nil {
			completed(false)
		}
		return
	}
	if existing != nil && existing.State().Kind == "listening" {
		state.mu.Unlock()
		if open {
			r.openForward(existing, source)
		} else {
			r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "port_exists")) })
		}
		if completed != nil {
			completed(open)
		}
		return
	}
	state.busy[key] = true
	state.mu.Unlock()
	owner := r.runtimeFor(node)
	if owner == nil || node != "" && owner.controller.Target().String() != node {
		state.mu.Lock()
		delete(state.busy, key)
		state.mu.Unlock()
		r.enqueue(func(c *ui.Context) {
			kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
			if completed != nil {
				completed(false)
			}
		})
		return
	}
	if !r.async(func() {
		forward, err := r.connectForward(owner, remote, local, source)
		state.mu.Lock()
		delete(state.busy, key)
		if err == nil && !state.closed {
			if previous, found := state.forwards[key]; found {
				previous.release()
			}
			state.forwards[key] = desktopForward{forward: forward, release: forward.OnChange(r.changed), source: source}
		}
		closed := state.closed
		state.mu.Unlock()
		if closed && forward != nil {
			_ = forward.Close()
			return
		}
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				// The port parsed; what failed is the route or the local listener.
				kit.Toast(c, kit.Failed, i18n.T(c, "port_failed"))
			} else if open {
				r.openForward(forward, source)
			}
			if completed != nil {
				completed(err == nil)
			}
			r.changed()
		})
	}) {
		state.mu.Lock()
		delete(state.busy, key)
		state.mu.Unlock()
	}
}

func (r *Runtime) openForward(forward *client.Forwarder, source *client.ServiceSource) {
	if r.openService == nil {
		return
	}
	announced := ""
	if source != nil {
		announced = source.Service.Url
	}
	r.openService(forwardURL(announced, forward.LocalPort()))
}

// forwardURL is the address a forwarded service opens at: the URL its
// command announced with only the host and port replaced by the local
// listener, so its scheme, path, query and fragment survive. A forward
// without an announced URL opens the listener's root over HTTP.
func forwardURL(announced string, port uint16) string {
	host := "127.0.0.1:" + strconv.Itoa(int(port))
	if target, err := url.Parse(announced); err == nil && (target.Scheme == "http" || target.Scheme == "https") {
		target.Host, target.User = host, nil
		return target.String()
	}
	return "http://" + host
}

func (x *conversationActions) forwardCommandService(port int) {
	session, found := x.r.session(x.cv.ID)
	if !found || !x.r.controller.Connected() {
		return
	}
	state, found := x.r.controller.Commands(session.ID)
	if !found || !state.Connected {
		return
	}
	for _, command := range state.Items {
		if command.Status.Kind != "running" && command.Status.Kind != "stopping" {
			continue
		}
		for _, service := range command.Services {
			if int(service.Port) == port {
				source := &client.ServiceSource{Session: session.ID, Command: command.ID, Service: service}
				x.r.localRuntime().startForward(x.r.controller.Target().String(), service.Port, 0, source, true, nil)
				return
			}
		}
	}
}
