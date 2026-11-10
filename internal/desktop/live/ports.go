package live

import (
	"encoding/json"
	"fmt"
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

type desktopForward struct {
	forward *client.Forwarder
	release func()
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

func (r *Runtime) portsFrame() {
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
				root.openForward(forward)
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
			r.openForward(existing)
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
		var forward *client.Forwarder
		var err error
		if source == nil {
			forward, err = owner.controller.ForwardPort(r.ctx, remote, local)
		} else {
			var encoded []byte
			encoded, err = json.Marshal(source)
			if err == nil {
				forward, err = owner.controller.ForwardService(r.ctx, encoded, local)
			}
		}
		state.mu.Lock()
		delete(state.busy, key)
		if err == nil && !state.closed {
			if previous, found := state.forwards[key]; found {
				previous.release()
			}
			state.forwards[key] = desktopForward{forward: forward, release: forward.OnChange(r.changed)}
		}
		closed := state.closed
		state.mu.Unlock()
		if closed && forward != nil {
			_ = forward.Close()
			return
		}
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				r.report(c, err, "port_invalid")
			} else if open {
				r.openForward(forward)
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

func (r *Runtime) openForward(forward *client.Forwarder) {
	if r.openService != nil {
		r.openService(fmt.Sprintf("http://127.0.0.1:%d", forward.LocalPort()))
	}
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
