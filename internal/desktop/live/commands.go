package live

import (
	"github.com/egoist/mygo/ui"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type liveCommands struct {
	busy     map[commandViewKey]bool
	releases map[protocol.SessionID]func()
	outputs  map[commandViewKey]func()
}
type commandViewKey struct {
	session protocol.SessionID
	command protocol.RequestID
}

func (r *Runtime) initCommands() {
	r.commands = &liveCommands{busy: map[commandViewKey]bool{}, releases: map[protocol.SessionID]func(){}, outputs: map[commandViewKey]func(){}}
}
func (r *Runtime) closeCommands() {
	if r.commands == nil {
		return
	}
	for _, release := range r.commands.releases {
		release()
	}
	for _, release := range r.commands.outputs {
		release()
	}
}
func (r *Runtime) projectCommands(cv string, comp *composer.State, activity *composer.Activity) {
	session, ok := r.session(cv)
	if !ok {
		return
	}
	if _, exists := r.commands.releases[session.ID]; !exists {
		release, err := r.controller.ObserveCommands(r.ctx, session.ID, r.changed)
		if err == nil {
			r.commands.releases[session.ID] = release
		}
	}
	if comp.ActivityCallbacks == nil {
		comp.ActivityCallbacks = &composer.ActivityCallbacks{}
	}
	comp.ActivityCallbacks.CommandState = func() (bool, bool) {
		state, known := r.controller.Commands(session.ID)
		return known && state.Connected, known && state.Err != nil
	}
	comp.ActivityCallbacks.CommandError = func(text string) bool {
		id, err := protocol.ParseRequestID(text)
		if err != nil {
			return false
		}
		state, ok := r.controller.CommandOutput(session.ID, id)
		return ok && state.Err != nil
	}
	comp.ActivityCallbacks.CommandBusy = func(text string) bool {
		id, err := protocol.ParseRequestID(text)
		return err == nil && r.commands.busy[commandViewKey{session.ID, id}]
	}
	comp.ActivityCallbacks.ServiceLabel = func(port int) string {
		if forward, _ := r.forward(r.controller.Target().String(), port); forward != nil && forward.State().Kind == "listening" {
			return strconv.Itoa(port) + " → " + strconv.Itoa(int(forward.LocalPort()))
		}
		return strconv.Itoa(port)
	}
	comp.ActivityCallbacks.ServiceBusy = func(port int) bool {
		_, busy := r.forward(r.controller.Target().String(), port)
		return busy
	}
	if state, ok := r.controller.Commands(session.ID); ok {
		for _, value := range state.Items {
			if value.Status.Kind != "running" && value.Status.Kind != "stopping" {
				continue
			}
			command := composer.Command{ID: value.ID.String(), Command: value.Command, Started: time.UnixMilli(int64(value.StartedMs)), Status: composer.CommandRunning}
			if value.Status.Kind == "stopping" {
				command.Status = composer.CommandStopping
			}
			for _, service := range value.Services {
				command.Ports = append(command.Ports, int(service.Port))
			}
			if output, ok := r.controller.CommandOutput(session.ID, value.ID); ok && output.Known {
				command.Output = output.Snapshot.Stdout.Text
				if output.Snapshot.Stderr.Text != "" {
					if command.Output != "" {
						command.Output += "\n"
					}
					command.Output += output.Snapshot.Stderr.Text
				}
				lines := strings.Split(command.Output, "\n")
				if len(lines) > 100 {
					command.Output = strings.Join(lines[len(lines)-100:], "\n")
				}
			}
			activity.Commands = append(activity.Commands, command)
		}
	}
	if id, err := protocol.ParseRequestID(comp.ActiveCommand()); err == nil {
		key := commandViewKey{session.ID, id}
		if _, exists := r.commands.outputs[key]; !exists {
			release, err := r.controller.ObserveCommandOutput(r.ctx, session.ID, id, r.changed)
			if err == nil {
				r.commands.outputs[key] = release
			}
		}
	}
}
func (r *Runtime) pruneCommands() {
	if r.commands == nil {
		return
	}
	for session, release := range r.commands.releases {
		if !r.conversationSeen[session] {
			release()
			delete(r.commands.releases, session)
		}
	}
	active := map[commandViewKey]bool{}
	for session := range r.conversationSeen {
		if id, err := protocol.ParseRequestID(r.App.Composer(session.String()).ActiveCommand()); err == nil {
			active[commandViewKey{session, id}] = true
		}
	}
	for key, release := range r.commands.outputs {
		if !active[key] {
			release()
			delete(r.commands.outputs, key)
		}
	}
}
func (x *conversationActions) stopWorkspaceCommand(text string) {
	session, ok := x.r.session(x.cv.ID)
	if !ok {
		return
	}
	id, err := protocol.ParseRequestID(text)
	if err != nil {
		return
	}
	key := commandViewKey{session.ID, id}
	if x.r.commands.busy[key] {
		return
	}
	x.r.commands.busy[key] = true
	x.r.execute(protocol.Command{Kind: "stop_command", Data: protocol.CommandStopCommandPayload{Session: session.ID, ID: id}}, func(c *ui.Context, out protocol.Output, err error) {
		delete(x.r.commands.busy, key)
		x.r.reportKeys(c, err, "background_stop_failed", "workspace_changed", "background_stop_unknown")
	})
}
func (x *conversationActions) openCommandService(port int) {
	x.forwardCommandService(port)
}
